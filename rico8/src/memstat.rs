//! Memory accounting, measured inside the cart.
//!
//! [`used_bytes`] is the high-water mark of committed linear memory: the
//! highest address the allocator has ever handed out (on wasm a pointer is a
//! linear-memory offset, so `ptr + size` is the top of the committed region),
//! floored at `__heap_base` so the always-resident shadow-stack reserve and
//! statics count even before the first allocation. It never recedes — wasm
//! never returns pages — and it includes freed-but-stranded fragments, since
//! they sit below the high-water. That makes it a far better gauge than a live
//! byte count, though still not an exact OOM line: the allocator keeps a small
//! "wilderness" reserve above the last allocation, so the cap can be a hair
//! away when this reads just under it.

#[cfg(feature = "std")]
use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// The linear-memory cap, in bytes (matches the runtime's `MAX_MEMORY`).
pub const CAP: usize = 131_072;

/// Live heap bytes (allocated minus freed), maintained by [`TrackingAlloc`].
static HEAP_LIVE: AtomicUsize = AtomicUsize::new(0);
/// Highest `ptr + size` the allocator has ever returned — the high-water of
/// committed linear memory. Monotonic: frees never lower it.
#[cfg(any(feature = "std", target_arch = "wasm32"))]
static PEAK_END: AtomicUsize = AtomicUsize::new(0);

/// Record an allocation's extent against both counters.
#[cfg(feature = "std")]
fn note_alloc(ptr: *mut u8, size: usize) {
    HEAP_LIVE.fetch_add(size, Relaxed);
    PEAK_END.fetch_max(ptr as usize + size, Relaxed);
}

/// A `#[global_allocator]` that delegates to the system allocator and tracks
/// the cart's memory footprint. The `rico8` crate installs it for `std` carts.
pub struct TrackingAlloc;

#[cfg(feature = "std")]
unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = std::alloc::System.alloc(layout);
        if !p.is_null() {
            note_alloc(p, layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = std::alloc::System.alloc_zeroed(layout);
        if !p.is_null() {
            note_alloc(p, layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        std::alloc::System.dealloc(ptr, layout);
        // PEAK_END is never lowered: wasm keeps the page committed.
        HEAP_LIVE.fetch_sub(layout.size(), Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = std::alloc::System.realloc(ptr, layout, new_size);
        if !p.is_null() {
            HEAP_LIVE.fetch_sub(layout.size(), Relaxed);
            note_alloc(p, new_size);
        }
        p
    }
}

/// Live heap bytes currently allocated by the cart (allocated minus freed).
pub fn heap_live() -> usize {
    HEAP_LIVE.load(Relaxed)
}

/// The always-resident base below the heap: the shadow-stack reserve plus
/// statics (`__heap_base`). Counts against the cap even before any allocation.
#[cfg(target_arch = "wasm32")]
fn base_bytes() -> usize {
    #[allow(non_upper_case_globals)]
    extern "C" {
        static __heap_base: u8;
    }
    // SAFETY: `__heap_base` is a linker-defined symbol; we read only its address.
    unsafe { &__heap_base as *const u8 as usize }
}

/// Committed-memory high-water in bytes: the top of the heap region the
/// allocator has reached, floored at the base, capped at 128 K.
#[cfg(target_arch = "wasm32")]
pub fn used_bytes() -> usize {
    base_bytes().max(PEAK_END.load(Relaxed)).min(CAP)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn used_bytes() -> usize {
    0
}

/// [`used_bytes`] as a fraction `0.0..1.0` of the 128 K cap.
pub fn used_fraction() -> f32 {
    used_bytes() as f32 / CAP as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "std")]
    #[test]
    fn tracking_alloc_counts_live_heap() {
        use core::alloc::GlobalAlloc;
        let a = TrackingAlloc;
        let layout = Layout::from_size_align(4096, 8).unwrap();
        let before = heap_live();
        // SAFETY: standard alloc/dealloc round-trip with a valid layout.
        unsafe {
            let p = a.alloc(layout);
            assert!(!p.is_null());
            assert_eq!(heap_live(), before + 4096);
            a.dealloc(p, layout);
            assert_eq!(heap_live(), before);
        }
    }
}
