//! Record what project classification costs a count.
//!
//! Counting a workspace now loads its Cargo module graph twice — once with
//! `cfg(test)` off, once with it on — so the price of a count is worth
//! measuring rather than guessing. This example counts two workspaces and
//! reports wall-clock runtime and peak resident memory for each:
//!
//! - a **small fixture** built in a temporary directory: one package, four
//!   files, one of them reachable only under `#[cfg(all(test, unix))]`;
//! - a **representative workspace**: the directory given on the command line,
//!   defaulting to the current one (run it from a checkout of rustloc).
//!
//! Run it in release mode, which is the configuration the numbers describe:
//!
//! ```text
//! cargo run --release -p rustloclib --example project_classification_perf
//! ```
//!
//! Peak memory comes from the platform's `/usr/bin/time`, so each measurement
//! runs in a child process. When that tool is missing the runtime is still
//! reported and memory reads `unavailable`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use rustloclib::{count_workspace, Aggregation, CountOptions};

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        // Child process: do one count and report how long it took.
        Some("--measure") => {
            let path = args.next().expect("--measure needs a path");
            let started = Instant::now();
            let result =
                count_workspace(&path, CountOptions::new().aggregation(Aggregation::ByFile))
                    .expect("count failed");
            println!(
                "elapsed_ms={} files={} code={} tests={}",
                started.elapsed().as_millis(),
                result.file_count,
                result.total.code,
                result.total.tests
            );
        }
        // Parent process: measure both fixtures.
        other => {
            let workspace = other
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            let fixture = tempfile::tempdir().expect("failed to create the small fixture");
            write_small_fixture(fixture.path());

            report("small fixture", fixture.path());
            report("representative workspace", &workspace);
        }
    }
}

/// One package whose `archive_tests.rs` is reachable only under `cfg(test)`.
fn write_small_fixture(root: &Path) {
    let files = [
        (
            "Cargo.toml",
            "[package]\nname = \"perf-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        ("src/lib.rs", "pub mod archive;\n"),
        (
            "src/archive.rs",
            "pub fn archive() -> bool {\n    true\n}\n\n\
             #[cfg(all(test, unix))]\n\
             #[path = \"archive_tests.rs\"]\n\
             mod tests;\n",
        ),
        (
            "src/archive_tests.rs",
            "use super::archive;\n\n#[test]\nfn archives() {\n    assert!(archive());\n}\n",
        ),
    ];
    for (relative, contents) in files {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
}

/// Count `path` in a child process and print its runtime and peak memory.
fn report(label: &str, path: &Path) {
    let executable = std::env::current_exe().expect("no path to this executable");
    let measure = format!("{} --measure {}", executable.display(), path.display());

    let Some((flag, output)) = run_under_time(&measure) else {
        println!("{label}: runtime unavailable (could not run /usr/bin/time)");
        return;
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let counts = stdout.trim();
    let memory = peak_memory_bytes(&stderr, flag)
        .map(|bytes| format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)))
        .unwrap_or_else(|| "unavailable".to_string());

    println!("{label}: {counts} max_rss={memory}");
}

/// Run `command` under the platform's `/usr/bin/time`, verbose flag first.
fn run_under_time(command: &str) -> Option<(&'static str, std::process::Output)> {
    for flag in ["-l", "-v"] {
        let output = Command::new("/usr/bin/time")
            .arg(flag)
            .args(["sh", "-c", command])
            .output()
            .ok()?;
        if output.status.success() {
            return Some((flag, output));
        }
    }
    None
}

/// Read peak resident set size out of `/usr/bin/time`'s report.
///
/// BSD `time -l` prints bytes on a line ending in `maximum resident set size`;
/// GNU `time -v` prints kilobytes after `Maximum resident set size (kbytes):`.
fn peak_memory_bytes(report: &str, flag: &str) -> Option<u64> {
    for line in report.lines() {
        let line = line.trim();
        if flag == "-l" && line.ends_with("maximum resident set size") {
            return line.split_whitespace().next()?.parse().ok();
        }
        if let Some(kilobytes) = line.strip_prefix("Maximum resident set size (kbytes):") {
            return kilobytes.trim().parse::<u64>().ok().map(|kb| kb * 1024);
        }
    }
    None
}
