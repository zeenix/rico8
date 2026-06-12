//! Building carts: run `cargo build --target wasm32-unknown-unknown` on a
//! project, in the background, and report the result to the console.
//!
//! The same command works from a normal terminal — RICO-8 projects are
//! plain Cargo crates, so the external-editor workflow is just "run cargo
//! yourself" (or `rico8 build <dir>`).

use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

pub struct BuildResult {
    pub success: bool,
    /// Console-ready error/diagnostic lines (already trimmed down).
    pub errors: Vec<String>,
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
        Ok(out) if out.status.success() => BuildResult {
            success: true,
            errors: Vec::new(),
            duration: started.elapsed(),
        },
        Ok(out) => BuildResult {
            success: false,
            errors: extract_errors(&String::from_utf8_lossy(&out.stderr)),
            duration: started.elapsed(),
        },
        Err(e) => BuildResult {
            success: false,
            errors: vec![format!("could not run cargo: {e}")],
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

#[cfg(test)]
mod tests {
    use super::*;

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
