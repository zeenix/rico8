//! A small bounded undo/redo stack shared by the editors.
//!
//! Each editor keeps a [`History`] of snapshots of the data it edits — the
//! sprite sheet, the map, the SFX bank, the song, or the code buffer. A
//! *gesture* (a single key press, or a whole mouse drag) is bracketed by
//! [`History::begin`], which snapshots the pre-edit state, and
//! [`History::commit`], which records that snapshot for undo — but only when
//! the data actually changed, so no-op gestures never consume a slot.
//! [`History::undo`] / [`History::redo`] swap the live data with the
//! neighbouring snapshot. At most [`MAX_HISTORY`] steps are kept each way.

/// How many undo (and redo) steps each editor keeps.
pub const MAX_HISTORY: usize = 10;

pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    /// Snapshot taken at the start of the in-progress gesture, if any.
    pending: Option<T>,
}

impl<T: Clone + PartialEq> History<T> {
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
        }
    }

    /// Snapshot `current` as the start of a gesture. Idempotent until the next
    /// commit, so it is safe to call on every frame of a held drag.
    pub fn begin(&mut self, current: &T) {
        if self.pending.is_none() {
            self.pending = Some(current.clone());
        }
    }

    /// Close the open gesture, recording its snapshot for undo when `current`
    /// differs from it. A gesture that changed nothing is discarded.
    pub fn commit(&mut self, current: &T) {
        if let Some(prev) = self.pending.take() {
            if prev != *current {
                push_capped(&mut self.undo, prev);
                self.redo.clear();
            }
        }
    }

    /// Undo the last committed change, restoring it through `current`. Returns
    /// whether anything was undone.
    pub fn undo(&mut self, current: &mut T) -> bool {
        self.pending = None;
        match self.undo.pop() {
            Some(prev) => {
                push_capped(&mut self.redo, current.clone());
                *current = prev;
                true
            }
            None => false,
        }
    }

    /// Redo the last undone change, restoring it through `current`. Returns
    /// whether anything was redone.
    pub fn redo(&mut self, current: &mut T) -> bool {
        self.pending = None;
        match self.redo.pop() {
            Some(next) => {
                push_capped(&mut self.undo, current.clone());
                *current = next;
                true
            }
            None => false,
        }
    }

    /// Forget all history — used when the edited data is replaced wholesale
    /// (e.g. the code buffer is reloaded with different text).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
    }
}

impl<T: Clone + PartialEq> Default for History<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Push onto a stack, dropping the oldest entry past [`MAX_HISTORY`].
fn push_capped<T>(stack: &mut Vec<T>, item: T) {
    stack.push(item);
    if stack.len() > MAX_HISTORY {
        stack.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_then_redo_round_trips() {
        let mut h = History::new();
        let mut v = 0;
        h.begin(&v);
        v = 1;
        h.commit(&v);
        assert!(h.undo(&mut v));
        assert_eq!(v, 0);
        assert!(h.redo(&mut v));
        assert_eq!(v, 1);
    }

    #[test]
    fn no_op_gesture_records_nothing() {
        let mut h = History::new();
        let mut v = 5;
        h.begin(&v);
        h.commit(&v); // unchanged
        assert!(!h.undo(&mut v));
        assert_eq!(v, 5);
    }

    #[test]
    fn begin_is_idempotent_within_a_gesture() {
        let mut h = History::new();
        let mut v = 0;
        h.begin(&v); // snapshots 0
        v = 1;
        h.begin(&v); // ignored: gesture already open
        v = 2;
        h.commit(&v);
        h.undo(&mut v);
        assert_eq!(v, 0, "the whole drag is one undo step");
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut h = History::new();
        let mut v = 0;
        h.begin(&v);
        v = 1;
        h.commit(&v);
        h.undo(&mut v); // back to 0, redo holds 1
        h.begin(&v);
        v = 2;
        h.commit(&v);
        assert!(!h.redo(&mut v), "redo dropped by the new edit");
        assert_eq!(v, 2);
    }

    #[test]
    fn keeps_at_most_max_history_steps() {
        let mut h = History::new();
        let mut v = 0;
        for i in 1..=(MAX_HISTORY + 5) {
            h.begin(&v);
            v = i as i32;
            h.commit(&v);
        }
        // Only the last MAX_HISTORY steps survive.
        let mut count = 0;
        while h.undo(&mut v) {
            count += 1;
        }
        assert_eq!(count, MAX_HISTORY);
    }
}
