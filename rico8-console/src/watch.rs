//! Watching a project/cart's on-disk files so external edits are picked up
//! without clobbering in-console edits.
//!
//! The shell mirrors `src/lib.rs` and `assets.rico8` (or a PNG cart's assets)
//! in memory. Both the mirror and the file on disk can change, so before
//! adopting a disk change we compare three things — the baseline we last
//! synced, what is on disk now, and what we hold in memory — and decide.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

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

/// What a [`FileWatch::poll`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum FileChange {
    /// Nothing to do (mtime unchanged, or disk still equals the baseline).
    None,
    /// Adopt these bytes into the in-memory copy.
    Adopt(Vec<u8>),
    /// Both disk and memory changed; left untouched.
    Conflict,
}

/// Watches one file the shell mirrors in memory (`src/lib.rs`, `assets.rico8`).
///
/// Polling is mtime-gated: the file's contents are only read when its mtime has
/// advanced past the last synced point, so steady-state cost is one `stat`.
pub struct FileWatch {
    path: PathBuf,
    /// mtime as of the last sync, or `None` if the file did not exist then.
    synced_mtime: Option<SystemTime>,
    /// The bytes as of the last sync (load / save / adopt).
    baseline: Vec<u8>,
    /// An unresolved conflict is outstanding.
    conflict: bool,
}

impl FileWatch {
    /// Start watching `path`, treating `baseline` as the currently-synced
    /// content. The current mtime becomes the synced point, so a later edit is
    /// what triggers the first event.
    pub fn new(path: PathBuf, baseline: Vec<u8>) -> Self {
        let synced_mtime = file_mtime(&path);
        Self {
            path,
            synced_mtime,
            baseline,
            conflict: false,
        }
    }

    /// True while disk and memory both changed and neither has been chosen.
    pub fn in_conflict(&self) -> bool {
        self.conflict
    }

    /// The currently-synced baseline bytes (what disk held at last sync).
    pub fn baseline(&self) -> &[u8] {
        &self.baseline
    }

    /// Re-baseline after the shell wrote the file itself (save) or after a
    /// fresh load, so rico8's own writes are never seen as external edits.
    pub fn mark_synced(&mut self, baseline: Vec<u8>) {
        self.baseline = baseline;
        self.synced_mtime = file_mtime(&self.path);
        self.conflict = false;
    }

    /// Compare the file against the baseline and the caller's `in_memory` copy.
    /// On [`FileChange::Adopt`] the baseline advances to the disk bytes; on
    /// [`FileChange::Conflict`] the conflict latch is set. The mtime is always
    /// absorbed so the same disk state never fires twice.
    pub fn poll(&mut self, in_memory: &[u8]) -> FileChange {
        let mtime = match file_mtime(&self.path) {
            Some(m) => m,
            None => return FileChange::None,
        };
        let advanced = self.synced_mtime.map(|prev| mtime > prev).unwrap_or(true);
        if !advanced {
            return FileChange::None;
        }
        self.synced_mtime = Some(mtime);
        let disk = match fs::read(&self.path) {
            Ok(b) => b,
            Err(_) => return FileChange::None,
        };
        match reconcile(&self.baseline, &disk, in_memory) {
            Reconcile::Unchanged => FileChange::None,
            Reconcile::Adopt(bytes) => {
                self.baseline = bytes.clone();
                self.conflict = false;
                FileChange::Adopt(bytes)
            }
            Reconcile::Conflict => {
                self.conflict = true;
                FileChange::Conflict
            }
        }
    }
}

/// The file's modified-time, or `None` if it does not exist / has no mtime.
fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Tracks the newest mtime across a crate's source so any external edit — to
/// `src/lib.rs` or any other module, or `Cargo.toml` — triggers one rebuild.
/// (`cargo` itself decides what to recompile; this only answers "did anything
/// change".)
pub struct SourceTreeWatch {
    dir: PathBuf,
    newest: Option<SystemTime>,
}

impl SourceTreeWatch {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            newest: newest_source_mtime(dir),
        }
    }

    /// True if any watched source file is newer than the last poll. Absorbs the
    /// new high-water mark so the same change never fires twice.
    pub fn poll(&mut self) -> bool {
        let now = newest_source_mtime(&self.dir);
        match (now, self.newest) {
            (Some(now), Some(prev)) if now > prev => {
                self.newest = Some(now);
                true
            }
            (Some(now), None) => {
                self.newest = Some(now);
                true
            }
            _ => false,
        }
    }

    /// Re-baseline to the current tree (after rico8 wrote source itself).
    pub fn sync(&mut self) {
        self.newest = newest_source_mtime(&self.dir);
    }
}

/// Newest mtime among `dir/src/**/*.rs` and `dir/Cargo.toml`, or `None`.
fn newest_source_mtime(dir: &Path) -> Option<SystemTime> {
    let mut newest = file_mtime(&dir.join("Cargo.toml"));
    visit_rs(&dir.join("src"), &mut |m| {
        newest = match (newest, m) {
            (Some(a), b) if b > a => Some(b),
            (None, b) => Some(b),
            (cur, _) => cur,
        };
    });
    newest
}

/// Call `f` with the mtime of every `*.rs` file under `dir`, recursively.
fn visit_rs(dir: &Path, f: &mut impl FnMut(SystemTime)) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_rs(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            if let Some(m) = file_mtime(&path) {
                f(m);
            }
        }
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

    /// A temp dir that removes itself on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("rico8-watch-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Force `path`'s mtime to advance past whatever it is now, so the test
    /// does not depend on filesystem mtime resolution.
    fn write_newer(path: &Path, contents: &[u8]) {
        fs::write(path, contents).unwrap();
        let later = SystemTime::now() + std::time::Duration::from_secs(10);
        let _ = filetime_set(path, later);
    }

    /// Minimal mtime setter without pulling in the `filetime` crate: set it
    /// through the standard library's `File::set_modified` (stable since 1.75).
    fn filetime_set(path: &Path, t: SystemTime) -> std::io::Result<()> {
        let f = fs::OpenOptions::new().write(true).open(path)?;
        f.set_modified(t)
    }

    #[test]
    fn filewatch_no_event_until_mtime_advances() {
        let tmp = TempDir::new("noevent");
        let p = tmp.0.join("lib.rs");
        fs::write(&p, b"v1").unwrap();
        let mut w = FileWatch::new(p.clone(), b"v1".to_vec());
        // No mtime change since construction.
        assert_eq!(w.poll(b"v1"), FileChange::None);
    }

    #[test]
    fn filewatch_adopts_external_change_when_memory_clean() {
        let tmp = TempDir::new("adopt");
        let p = tmp.0.join("lib.rs");
        fs::write(&p, b"v1").unwrap();
        let mut w = FileWatch::new(p.clone(), b"v1".to_vec());
        write_newer(&p, b"v2");
        assert_eq!(w.poll(b"v1"), FileChange::Adopt(b"v2".to_vec()));
        // Baseline advanced: a second poll with the adopted content is quiet.
        assert_eq!(w.poll(b"v2"), FileChange::None);
        assert!(!w.in_conflict());
    }

    #[test]
    fn filewatch_conflicts_when_both_changed() {
        let tmp = TempDir::new("conflict");
        let p = tmp.0.join("lib.rs");
        fs::write(&p, b"v1").unwrap();
        let mut w = FileWatch::new(p.clone(), b"v1".to_vec());
        write_newer(&p, b"disk");
        // In-memory is "mem" (dirty) while disk became "disk".
        assert_eq!(w.poll(b"mem"), FileChange::Conflict);
        assert!(w.in_conflict());
    }

    #[test]
    fn filewatch_mark_synced_clears_conflict_and_rebases() {
        let tmp = TempDir::new("synced");
        let p = tmp.0.join("lib.rs");
        fs::write(&p, b"v1").unwrap();
        let mut w = FileWatch::new(p.clone(), b"v1".to_vec());
        write_newer(&p, b"disk");
        assert_eq!(w.poll(b"mem"), FileChange::Conflict);
        // The shell resolves by saving "mem" to disk, then marks synced.
        write_newer(&p, b"mem");
        w.mark_synced(b"mem".to_vec());
        assert!(!w.in_conflict());
        assert_eq!(w.poll(b"mem"), FileChange::None);
    }

    #[test]
    fn source_tree_detects_any_rs_or_cargo_change() {
        let tmp = TempDir::new("srctree");
        let dir = &tmp.0;
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("Cargo.toml"), b"[package]\nname=\"g\"\n").unwrap();
        fs::write(dir.join("src/lib.rs"), b"fn a(){}").unwrap();
        fs::write(dir.join("src/foo.rs"), b"fn b(){}").unwrap();

        let mut w = SourceTreeWatch::new(dir);
        assert!(!w.poll(), "no change right after construction");

        // Touch a non-lib module — still a source change.
        write_newer(&dir.join("src/foo.rs"), b"fn b(){ }");
        assert!(w.poll(), "foo.rs change detected");
        assert!(!w.poll(), "absorbed; no repeat");

        // Cargo.toml counts too.
        write_newer(
            &dir.join("Cargo.toml"),
            b"[package]\nname=\"g\"\nedition=\"2021\"\n",
        );
        assert!(w.poll(), "Cargo.toml change detected");
    }
}
