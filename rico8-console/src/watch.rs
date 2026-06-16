//! Watching a project/cart's on-disk files so external edits are picked up
//! without clobbering in-console edits.
//!
//! The shell mirrors `src/lib.rs` and `assets.rico8` (or a PNG cart's assets)
//! in memory. Both the mirror and the file on disk can change, so before
//! adopting a disk change we compare three things — the baseline we last
//! synced, what is on disk now, and what we hold in memory — and decide.

/// What to do after comparing disk against our baseline and in-memory copy.
#[derive(Debug, PartialEq, Eq)]
pub enum Reconcile {
    /// Disk still matches the baseline: nothing changed (e.g. a bare `touch`).
    Unchanged,
    /// Disk changed and the in-memory copy is clean: adopt the disk bytes.
    Adopt(Vec<u8>),
    /// Disk changed *and* the in-memory copy has unsaved edits: a conflict we
    /// must not resolve by clobbering either side.
    Conflict,
}

/// Compare `disk` against the `baseline` we last synced and the current
/// `in_memory` copy. See [`Reconcile`].
pub fn reconcile(baseline: &[u8], disk: &[u8], in_memory: &[u8]) -> Reconcile {
    if disk == baseline {
        Reconcile::Unchanged
    } else if in_memory == baseline {
        Reconcile::Adopt(disk.to_vec())
    } else {
        Reconcile::Conflict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_when_disk_matches_baseline() {
        assert_eq!(reconcile(b"a", b"a", b"a"), Reconcile::Unchanged);
        // Even if memory is dirty, a disk that equals the baseline is no event.
        assert_eq!(reconcile(b"a", b"a", b"b"), Reconcile::Unchanged);
    }

    #[test]
    fn adopt_when_disk_changed_and_memory_clean() {
        assert_eq!(reconcile(b"a", b"b", b"a"), Reconcile::Adopt(b"b".to_vec()));
    }

    #[test]
    fn conflict_when_both_changed() {
        assert_eq!(reconcile(b"a", b"b", b"c"), Reconcile::Conflict);
    }
}
