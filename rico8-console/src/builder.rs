//! Building carts: run `cargo build --target wasm32-unknown-unknown` on a
//! project, in the background, and report the result to the console.
//!
//! The same command works from a normal terminal — RICO-8 projects are
//! plain Cargo crates, so the external-editor workflow is just "run cargo
//! yourself" (or `rico8 build <dir>`).

use rico8_runtime::{cart, project::Project};
use std::{
    path::Path,
    process::Command,
    sync::mpsc::{channel, Receiver},
    time::{Duration, Instant},
};

pub struct BuildResult {
    pub success: bool,
    /// Console-ready error/diagnostic lines (already trimmed down).
    pub errors: Vec<String>,
    /// Non-fatal diagnostics, e.g. the cart exceeding the 128 K size limit.
    pub warnings: Vec<String>,
    pub duration: Duration,
}

/// A build running in a background thread.
pub struct BuildJob {
    rx: Receiver<BuildResult>,
}

impl BuildJob {
    /// Poll for completion without blocking.
    pub fn poll(&self) -> Option<BuildResult> {
        self.rx.try_recv().ok()
    }
}

/// Kick off a release wasm build of the project directory.
pub fn spawn_build(project_dir: &Path) -> BuildJob {
    let dir = project_dir.to_path_buf();
    let (tx, rx) = channel();
    let started = Instant::now();
    std::thread::spawn(move || {
        let result = run_build(&dir, started);
        let _ = tx.send(result);
    });
    BuildJob { rx }
}

/// Run the build synchronously (used by headless `rico8 build`).
pub fn run_build(dir: &Path, started: Instant) -> BuildResult {
    let output = Command::new("cargo")
        .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
        .current_dir(dir)
        .env("CARGO_TERM_COLOR", "never")
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let (errors, warnings) = post_build_diagnostics(dir);
            let success = errors.is_empty();
            BuildResult {
                success,
                errors,
                warnings,
                duration: started.elapsed(),
            }
        }
        Ok(out) => BuildResult {
            success: false,
            errors: extract_errors(&String::from_utf8_lossy(&out.stderr)),
            warnings: Vec::new(),
            duration: started.elapsed(),
        },
        Err(e) => BuildResult {
            success: false,
            errors: vec![format!("could not run cargo: {e}")],
            warnings: Vec::new(),
            duration: started.elapsed(),
        },
    }
}

/// Pull the interesting lines out of cargo's stderr: error headers, their
/// source locations, and the final summary. The console is 31 columns, so
/// less is more.
fn extract_errors(stderr: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in stderr.lines() {
        let t = line.trim();
        if t.starts_with("error") || t.starts_with("warning: unused") {
            out.push(t.to_string());
        } else if t.starts_with("-->") {
            // Source location: keep just file:line:col.
            out.push(format!("  {}", t.trim_start_matches("--> ").trim()));
        }
    }
    if out.is_empty() {
        out.push("build failed (see terminal for details)".into());
    }
    // Cap the flood; the console shows the rest of the story on request.
    if out.len() > 24 {
        let extra = out.len() - 24;
        out.truncate(24);
        out.push(format!("... and {extra} more lines"));
    }
    out
}

/// Gather all post-build diagnostics: memory-budget errors/warnings and the
/// file-size warning. Returns `(errors, warnings)`. On any read or parse
/// failure, the check is silently skipped — same defensive style as the rest
/// of the build pipeline.
fn post_build_diagnostics(dir: &Path) -> (Vec<String>, Vec<String>) {
    let Ok(project) = Project::load(dir) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(wasm) = std::fs::read(project.wasm_path()) else {
        return (Vec::new(), Vec::new());
    };

    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Memory-budget check: the cart must start within the 128 K linear-memory cap.
    if let Some(initial_bytes) = cart::initial_memory_bytes(&wasm) {
        let (mem_errors, mem_warnings) = memory_diagnostics(initial_bytes);
        errors.extend(mem_errors);
        warnings.extend(mem_warnings);
    }

    // File-size check: warn now rather than at pack time. The hard gate is at
    // export, so this stays a warning.
    warnings.extend(wasm_size_warnings(dir));

    (errors, warnings)
}

/// Memory-budget diagnostics for a freshly built cart, from its initial linear
/// memory in bytes. Returns `(errors, warnings)`.
/// - over the 128K cap (>= 3 pages): error — the cart won't load.
/// - exactly the cap (2 pages, > 64K): warning — no heap headroom.
/// - 1 page (<= 64K): nothing.
fn memory_diagnostics(initial_bytes: usize) -> (Vec<String>, Vec<String>) {
    let kib = initial_bytes / 1024;
    if initial_bytes > cart::MEMORY_CAP {
        (
            vec![format!(
                "error: cart needs {kib}K of RAM at startup; over the 128K cap — it won't \
                 load. Reduce static data or the stack reserve (stack-size in \
                 .cargo/config.toml)."
            )],
            Vec::new(),
        )
    } else if initial_bytes > cart::WASM_PAGE_SIZE {
        (
            Vec::new(),
            vec![format!(
                "warning: cart starts at {kib}K of RAM (both 64K pages) — no heap headroom left."
            )],
        )
    } else {
        (Vec::new(), Vec::new())
    }
}

/// Warn when a freshly built cart exceeds the 128 K export size limit. The
/// build still succeeds — the hard gate is at export — but the author should
/// know now rather than at pack time.
fn wasm_size_warnings(dir: &Path) -> Vec<String> {
    let Ok(project) = Project::load(dir) else {
        return Vec::new();
    };
    let Ok(meta) = std::fs::metadata(project.wasm_path()) else {
        return Vec::new();
    };
    let size = meta.len() as usize;
    if size > cart::MAX_WASM_SIZE {
        vec![format!(
            "warning: cart wasm is {size} bytes; over the 128K limit ({})",
            cart::MAX_WASM_SIZE
        )]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    /// A temporary directory that removes itself on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(suffix: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "rico8-builder-test-{suffix}-{}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Create a minimal project dir loadable by `Project::load` and return it
    /// together with the wasm output path so the caller can populate it.
    fn temp_project(suffix: &str) -> (TempDir, PathBuf) {
        let tmp = TempDir::new(suffix);
        let dir = tmp.path();
        // Minimal Cargo.toml that `Project::load` (= parse_crate_name) can read.
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"testcart\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        // Project::wasm_path() = target/wasm32-unknown-unknown/release/<name>.wasm
        let wasm_dir = dir.join("target/wasm32-unknown-unknown/release");
        fs::create_dir_all(&wasm_dir).unwrap();
        let wasm_path = wasm_dir.join("testcart.wasm");
        (tmp, wasm_path)
    }

    #[test]
    fn wasm_size_warnings_absent_when_wasm_missing() {
        let (tmp, _wasm_path) = temp_project("missing");
        // No wasm file — should return an empty vec, not panic.
        assert!(wasm_size_warnings(tmp.path()).is_empty());
    }

    #[test]
    fn wasm_size_warnings_absent_for_small_wasm() {
        let (tmp, wasm_path) = temp_project("small");
        fs::write(&wasm_path, vec![0u8; 100]).unwrap();
        assert!(wasm_size_warnings(tmp.path()).is_empty());
    }

    #[test]
    fn wasm_size_warnings_fires_for_oversized_wasm() {
        let (tmp, wasm_path) = temp_project("oversized");
        fs::write(
            &wasm_path,
            vec![0u8; rico8_runtime::cart::MAX_WASM_SIZE + 1],
        )
        .unwrap();
        let warnings = wasm_size_warnings(tmp.path());
        assert!(
            !warnings.is_empty(),
            "expected a warning for oversized wasm"
        );
        assert!(
            warnings[0].contains("128K"),
            "warning should mention 128K: {}",
            warnings[0]
        );
    }

    #[test]
    fn memory_diagnostics_ok_for_one_page() {
        let (errors, warnings) = memory_diagnostics(65_536);
        assert_eq!(errors.len(), 0, "expected no errors for 1-page cart");
        assert_eq!(warnings.len(), 0, "expected no warnings for 1-page cart");
    }

    #[test]
    fn memory_diagnostics_warns_at_two_pages() {
        let (errors, warnings) = memory_diagnostics(131_072);
        assert_eq!(errors.len(), 0, "expected no errors at the cap");
        assert_eq!(warnings.len(), 1, "expected one warning at 2 pages");
        assert!(
            warnings[0].contains("128K") || warnings[0].contains("128"),
            "warning should mention the size: {}",
            warnings[0]
        );
    }

    #[test]
    fn memory_diagnostics_errors_over_two_pages() {
        let (errors, warnings) = memory_diagnostics(196_608);
        assert_eq!(errors.len(), 1, "expected one error over the cap");
        assert_eq!(warnings.len(), 0, "expected no warnings when over the cap");
        assert!(
            errors[0].contains("192K") || errors[0].contains("192"),
            "error should mention the size: {}",
            errors[0]
        );
    }

    #[test]
    fn extracts_error_lines() {
        let stderr = "\
   Compiling game v0.1.0
error[E0425]: cannot find value `bogus` in this scope
  --> src/lib.rs:10:9
   |
10 |         bogus += 1;
   |         ^^^^^ not found in this scope
error: aborting due to 1 previous error";
        let lines = extract_errors(stderr);
        assert!(lines[0].contains("E0425"));
        assert!(lines[1].contains("src/lib.rs:10:9"));
    }
}
