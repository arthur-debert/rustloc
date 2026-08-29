//! Cargo project classification: which Rust files only exist in test builds.
//!
//! The Rust syntax backend classifies the lines *inside* one file. It cannot
//! see why the file exists. A file like `archive_tests.rs` reads as ordinary
//! production code until you read its parent's declaration:
//!
//! ```rust,ignore
//! #[cfg(all(test, unix))]
//! #[path = "archive_tests.rs"]
//! mod tests;
//! ```
//!
//! Rust only treats `archive_tests.rs` as a module when `cfg(test)` and
//! `cfg(unix)` are both active, so every line in it belongs to the test build.
//!
//! This module answers that question from the Cargo module graph instead of
//! from filenames. It loads the project twice with rust-analyzer's project
//! model — once with `cfg(test)` off, once with it on — and records which
//! source files each configuration reaches. A file reachable only with
//! `cfg(test)`, or reachable only as (or from) a Cargo test target, is
//! test-only.
//!
//! Cargo and rust-analyzer types stay behind this boundary:
//! [`ProjectClassification`] hands callers a single `is_test_only` predicate
//! over paths, and nothing else in rustloclib links a file to its crate graph.
//!
//! Loading never executes build scripts or proc macros, and never fails a
//! command: any error — no Cargo, no rustc, an unparseable manifest — yields
//! an empty classification, which leaves the file-local result untouched.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ra_ap_hir::Crate;
use ra_ap_ide_db::base_db::CrateOrigin;
use ra_ap_load_cargo::{load_workspace, LoadCargoConfig, ProcMacroServerChoice};
use ra_ap_paths::AbsPathBuf;
use ra_ap_project_model::{
    CargoConfig, ProjectManifest, ProjectWorkspace, ProjectWorkspaceKind, TargetKind,
};
use ra_ap_vfs::{FileId, Vfs};

/// Which files of one Cargo project belong to test builds only.
///
/// Paths are stored relative to the project root, so a classification built
/// from an exported git tree answers questions about the repository-relative
/// paths a diff reports.
#[derive(Debug, Clone, Default)]
pub struct ProjectClassification {
    root: PathBuf,
    test_only: HashSet<PathBuf>,
}

impl ProjectClassification {
    /// A classification that marks nothing as test-only.
    ///
    /// This is what callers that have no Cargo project — single files,
    /// plain directories — use, and what a failed load falls back to.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Load the Cargo project rooted at `root`, or an empty classification.
    ///
    /// `root` must contain the `Cargo.toml` to load; no parent directory is
    /// searched. Every failure mode — a missing manifest, no `cargo` or
    /// `rustc` on `PATH`, a manifest cargo rejects — returns
    /// [`ProjectClassification::empty`] rather than an error, so a command
    /// that worked before project classification existed still works.
    pub fn load(root: impl AsRef<Path>) -> Self {
        Self::try_load(root.as_ref()).unwrap_or_else(Self::empty)
    }

    /// Whether `path` belongs to a module only the test build can reach.
    ///
    /// `path` may be absolute (under the project root) or relative to it.
    pub fn is_test_only(&self, path: &Path) -> bool {
        if self.test_only.is_empty() {
            return false;
        }
        match self.relative(path) {
            Some(relative) => self.test_only.contains(&relative),
            None => false,
        }
    }

    /// Whether this classification knows about any test-only file.
    pub fn is_empty(&self) -> bool {
        self.test_only.is_empty()
    }

    /// Rewrite `path` into the project-root-relative form the map is keyed by.
    ///
    /// The filesystem is only consulted as a last resort, so a classification
    /// built from an exported git tree keeps answering after the export
    /// directory is gone.
    fn relative(&self, path: &Path) -> Option<PathBuf> {
        if path.is_relative() {
            return Some(path.to_path_buf());
        }
        if let Ok(relative) = path.strip_prefix(&self.root) {
            return Some(relative.to_path_buf());
        }
        let canonical = std::fs::canonicalize(path).ok()?;
        canonical
            .strip_prefix(&self.root)
            .ok()
            .map(Path::to_path_buf)
    }

    fn try_load(root: &Path) -> Option<Self> {
        let root = std::fs::canonicalize(root).ok()?;
        let normal = Reachability::load(&root, false)?;
        let with_test_cfg = Reachability::load(&root, true)?;

        let test_only = with_test_cfg
            .reachable
            .difference(&normal.production)
            .cloned()
            .collect();

        Some(Self { root, test_only })
    }
}

/// The source files one `cfg` configuration of a project reaches.
struct Reachability {
    /// Files reachable from local crates that are not Cargo test targets.
    production: HashSet<PathBuf>,
    /// Files reachable from any local crate, test targets included.
    reachable: HashSet<PathBuf>,
}

impl Reachability {
    /// Load the project at `root` (already canonical) with `cfg(test)` set to
    /// `set_test`, and collect the source file of every reachable module.
    fn load(root: &Path, set_test: bool) -> Option<Self> {
        let manifest_file = root.join("Cargo.toml");
        if !manifest_file.is_file() {
            return None;
        }
        let manifest_path = AbsPathBuf::try_from(manifest_file.to_str()?).ok()?;
        let manifest = ProjectManifest::from_manifest_file(manifest_path).ok()?;

        // `no_deps` keeps the load to workspace members, and no sysroot keeps
        // it off the standard library: module reachability needs neither, and
        // both cost seconds.
        let cargo_config = CargoConfig {
            no_deps: true,
            set_test,
            ..CargoConfig::default()
        };
        let workspace = ProjectWorkspace::load(manifest, &cargo_config, &|_| ()).ok()?;
        let test_target_roots = test_target_roots(&workspace);

        // `load_out_dirs_from_check: false` skips `cargo check`, so build
        // scripts never run; `ProcMacroServerChoice::None` starts no
        // proc-macro server, so proc macros never expand.
        let load_config = LoadCargoConfig {
            load_out_dirs_from_check: false,
            with_proc_macro_server: ProcMacroServerChoice::None,
            prefill_caches: false,
            num_worker_threads: 1,
            proc_macro_processes: 0,
        };
        let (db, vfs, _proc_macro_client) =
            load_workspace(workspace, &cargo_config.extra_env, &load_config).ok()?;

        let mut production = HashSet::new();
        let mut reachable = HashSet::new();

        for krate in Crate::all(&db) {
            if !matches!(krate.origin(&db), CrateOrigin::Local { .. }) {
                continue;
            }
            let is_test_target = file_path(&vfs, krate.root_file(&db))
                .is_some_and(|path| test_target_roots.contains(&path));

            for module in krate.modules(&db) {
                let Some(file_id) = module.as_source_file_id(&db) else {
                    continue;
                };
                let Some(path) = file_path(&vfs, file_id.file_id(&db)) else {
                    continue;
                };
                let Ok(relative) = path.strip_prefix(root) else {
                    continue;
                };
                reachable.insert(relative.to_path_buf());
                if !is_test_target {
                    production.insert(relative.to_path_buf());
                }
            }
        }

        Some(Self {
            production,
            reachable,
        })
    }
}

/// Canonical source paths of every Cargo `test` target in the workspace.
///
/// Cargo exposes integration-test targets in both `cfg` configurations, so
/// reachability alone never separates them from production crates. Their
/// roots are collected here and excluded from the production set instead.
fn test_target_roots(workspace: &ProjectWorkspace) -> HashSet<PathBuf> {
    let mut roots = HashSet::new();
    let ProjectWorkspaceKind::Cargo { cargo, .. } = &workspace.kind else {
        return roots;
    };
    for package in cargo.packages() {
        if !cargo[package].is_member {
            continue;
        }
        for &target in &cargo[package].targets {
            if cargo[target].kind == TargetKind::Test {
                if let Ok(path) = std::fs::canonicalize(cargo[target].root.as_ref() as &Path) {
                    roots.insert(path);
                }
            }
        }
    }
    roots
}

/// The canonical filesystem path a VFS file id stands for.
fn file_path(vfs: &Vfs, file_id: FileId) -> Option<PathBuf> {
    let path = vfs.file_path(file_id).as_path()?;
    std::fs::canonicalize(path.as_ref() as &Path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Write `contents` to `root/relative`, creating parent directories.
    fn write(root: &Path, relative: &str, contents: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// A single-package project shaped like Proiectio's `archive.rs`.
    fn proiectio_shaped(root: &Path) {
        write(
            root,
            "Cargo.toml",
            "[package]\nname = \"shaped\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        write(root, "src/lib.rs", "pub mod archive;\n");
        write(
            root,
            "src/archive.rs",
            "pub fn archive() {}\n\n#[cfg(all(test, unix))]\n#[path = \"archive_tests.rs\"]\nmod tests;\n",
        );
        write(
            root,
            "src/archive_tests.rs",
            "use super::*;\n\n#[test]\nfn archives() {\n    archive();\n}\n",
        );
    }

    #[test]
    fn cfg_test_only_modules_are_test_only() {
        let temp = tempdir().unwrap();
        proiectio_shaped(temp.path());

        let project = ProjectClassification::load(temp.path());

        assert!(project.is_test_only(Path::new("src/archive_tests.rs")));
        assert!(!project.is_test_only(Path::new("src/archive.rs")));
        assert!(!project.is_test_only(Path::new("src/lib.rs")));
    }

    #[test]
    fn cargo_test_targets_and_their_modules_are_test_only() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write(
            root,
            "Cargo.toml",
            "[package]\nname = \"shaped\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        write(root, "src/lib.rs", "pub fn run() {}\n");
        write(
            root,
            "tests/end_to_end.rs",
            "mod helper;\n\n#[test]\nfn works() {\n    helper::help();\n}\n",
        );
        write(root, "tests/helper/mod.rs", "pub fn help() {}\n");

        let project = ProjectClassification::load(root);

        assert!(project.is_test_only(Path::new("tests/end_to_end.rs")));
        assert!(project.is_test_only(Path::new("tests/helper/mod.rs")));
        assert!(!project.is_test_only(Path::new("src/lib.rs")));
    }

    #[test]
    fn modules_shared_with_production_are_not_test_only() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        write(
            root,
            "Cargo.toml",
            "[package]\nname = \"shaped\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        write(
            root,
            "src/lib.rs",
            "pub mod shared;\n\n#[cfg(test)]\nmod unit_tests;\n",
        );
        write(root, "src/shared.rs", "pub fn shared() {}\n");
        write(root, "src/unit_tests.rs", "#[test]\nfn works() {}\n");

        let project = ProjectClassification::load(root);

        assert!(!project.is_test_only(Path::new("src/shared.rs")));
        assert!(project.is_test_only(Path::new("src/unit_tests.rs")));
    }

    #[test]
    fn a_manifest_cargo_rejects_classifies_nothing() {
        let temp = tempdir().unwrap();
        let root = temp.path();
        // An edition no toolchain knows: cargo refuses the manifest, so the
        // project never loads.
        write(
            root,
            "Cargo.toml",
            "[package]\nname = \"broken\"\nversion = \"0.1.0\"\nedition = \"2099\"\n",
        );
        write(
            root,
            "src/lib.rs",
            "pub fn run() {}\n\n#[cfg(test)]\nmod cases;\n",
        );
        write(root, "src/cases.rs", "#[test]\nfn case() {}\n");

        let project = ProjectClassification::load(root);

        assert!(project.is_empty());
        assert!(!project.is_test_only(Path::new("src/cases.rs")));
    }

    #[test]
    fn a_directory_without_a_manifest_classifies_nothing() {
        let temp = tempdir().unwrap();
        write(temp.path(), "src/lib.rs", "pub fn run() {}\n");

        let project = ProjectClassification::load(temp.path());

        assert!(project.is_empty());
        assert!(!project.is_test_only(Path::new("src/lib.rs")));
    }
}
