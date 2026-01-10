//! Cargo workspace discovery and crate enumeration.
//!
//! This module provides functionality to discover crates within a Cargo workspace
//! and enumerate their source files.

use std::path::{Path, PathBuf};

use cargo_metadata::{MetadataCommand, Package};

use crate::error::RustlocError;
use crate::Result;

/// Information about a crate in a workspace.
#[derive(Debug, Clone)]
pub struct CrateInfo {
    /// Name of the crate
    pub name: String,
    /// Root directory of the crate (where Cargo.toml is)
    pub root: PathBuf,
    /// Source directories to scan (typically just "src")
    pub src_dirs: Vec<PathBuf>,
    /// Test directory if it exists
    pub tests_dir: Option<PathBuf>,
    /// Examples directory if it exists
    pub examples_dir: Option<PathBuf>,
    /// Benches directory if it exists
    pub benches_dir: Option<PathBuf>,
}

impl CrateInfo {
    /// Create CrateInfo from a cargo_metadata Package
    fn from_package(package: &Package) -> Self {
        let root = package
            .manifest_path
            .parent()
            .map(|p| p.to_path_buf().into_std_path_buf())
            .unwrap_or_default();

        let src_dir = root.join("src");
        let tests_dir = root.join("tests");
        let examples_dir = root.join("examples");
        let benches_dir = root.join("benches");

        Self {
            name: package.name.clone(),
            root: root.clone(),
            src_dirs: if src_dir.exists() {
                vec![src_dir]
            } else {
                vec![]
            },
            tests_dir: if tests_dir.exists() {
                Some(tests_dir)
            } else {
                None
            },
            examples_dir: if examples_dir.exists() {
                Some(examples_dir)
            } else {
                None
            },
            benches_dir: if benches_dir.exists() {
                Some(benches_dir)
            } else {
                None
            },
        }
    }

    /// Get all directories that should be scanned for this crate
    pub fn all_dirs(&self) -> Vec<&Path> {
        let mut dirs: Vec<&Path> = self.src_dirs.iter().map(|p| p.as_path()).collect();

        if let Some(ref tests) = self.tests_dir {
            dirs.push(tests.as_path());
        }
        if let Some(ref examples) = self.examples_dir {
            dirs.push(examples.as_path());
        }
        if let Some(ref benches) = self.benches_dir {
            dirs.push(benches.as_path());
        }

        dirs
    }

    /// Check if a file path belongs to this crate.
    ///
    /// This is used to map arbitrary file paths (e.g., from git diffs) to their
    /// owning crate, enabling centralized filtering. Rather than re-implementing
    /// crate filtering logic in multiple places, callers should:
    ///
    /// 1. Use this method to find which crate a file belongs to
    /// 2. Apply crate-level filters using `WorkspaceInfo::filter_by_names()`
    ///
    /// This design keeps filtering logic centralized in the workspace module.
    ///
    /// The path can be absolute or relative to the workspace root.
    pub fn contains_path(&self, path: &Path, workspace_root: &Path) -> bool {
        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace_root.join(path)
        };
        absolute_path.starts_with(&self.root)
    }
}

/// Workspace information containing all discovered crates.
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    /// Root directory of the workspace
    pub root: PathBuf,
    /// All crates in the workspace
    pub crates: Vec<CrateInfo>,
}

impl WorkspaceInfo {
    /// Discover workspace information from a path.
    ///
    /// The path can be:
    /// - A directory containing Cargo.toml
    /// - A path to a Cargo.toml file
    /// - Any path within a cargo project (will search up for Cargo.toml)
    pub fn discover(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Find the manifest path
        let manifest_path = if path.is_file() && path.file_name() == Some("Cargo.toml".as_ref()) {
            path.to_path_buf()
        } else if path.is_dir() {
            let cargo_toml = path.join("Cargo.toml");
            if cargo_toml.exists() {
                cargo_toml
            } else {
                return Err(RustlocError::PathNotFound(path.to_path_buf()));
            }
        } else {
            return Err(RustlocError::PathNotFound(path.to_path_buf()));
        };

        let metadata = MetadataCommand::new()
            .manifest_path(&manifest_path)
            .exec()
            .map_err(|e| RustlocError::CargoMetadata(e.to_string()))?;

        let root = metadata.workspace_root.into_std_path_buf();

        // Get workspace members
        let workspace_members: std::collections::HashSet<_> =
            metadata.workspace_members.iter().collect();

        let crates: Vec<CrateInfo> = metadata
            .packages
            .iter()
            .filter(|p| workspace_members.contains(&p.id))
            .map(CrateInfo::from_package)
            .collect();

        Ok(Self { root, crates })
    }

    /// Filter crates by name.
    ///
    /// Returns a new WorkspaceInfo containing only crates whose names match
    /// any of the provided names.
    pub fn filter_by_names(&self, names: &[&str]) -> Self {
        let crates = self
            .crates
            .iter()
            .filter(|c| names.contains(&c.name.as_str()))
            .cloned()
            .collect();

        Self {
            root: self.root.clone(),
            crates,
        }
    }

    /// Get a crate by name.
    pub fn get_crate(&self, name: &str) -> Option<&CrateInfo> {
        self.crates.iter().find(|c| c.name == name)
    }

    /// Get all crate names.
    pub fn crate_names(&self) -> Vec<&str> {
        self.crates.iter().map(|c| c.name.as_str()).collect()
    }

    /// Find which crate a file path belongs to.
    ///
    /// This enables centralized crate-level filtering for any source of file paths
    /// (filesystem discovery, git diffs, etc.). The design principle is:
    ///
    /// **All filtering (glob patterns, crate names) should be done centrally
    /// using `FilterConfig` and `WorkspaceInfo`, not re-implemented per feature.**
    ///
    /// For example, when computing git diffs:
    /// 1. Get changed file paths from git
    /// 2. Use `FilterConfig::matches()` for glob filtering
    /// 3. Use this method to map files to crates
    /// 4. Apply crate filter via `filter_by_names()` check
    ///
    /// The path can be absolute or relative to the workspace root.
    pub fn crate_for_path(&self, path: &Path) -> Option<&CrateInfo> {
        self.crates
            .iter()
            .find(|c| c.contains_path(path, &self.root))
    }
}

/// Check if a path is within a Cargo workspace.
pub fn is_cargo_project(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    if path.is_dir() {
        path.join("Cargo.toml").exists()
    } else {
        path.file_name() == Some("Cargo.toml".as_ref()) && path.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crate_info_all_dirs() {
        // Test with a mock CrateInfo
        let info = CrateInfo {
            name: "test-crate".to_string(),
            root: PathBuf::from("/project"),
            src_dirs: vec![PathBuf::from("/project/src")],
            tests_dir: Some(PathBuf::from("/project/tests")),
            examples_dir: Some(PathBuf::from("/project/examples")),
            benches_dir: None,
        };

        let dirs = info.all_dirs();
        assert_eq!(dirs.len(), 3);
        assert!(dirs.contains(&Path::new("/project/src")));
        assert!(dirs.contains(&Path::new("/project/tests")));
        assert!(dirs.contains(&Path::new("/project/examples")));
    }

    #[test]
    fn test_workspace_filter_by_names() {
        let workspace = WorkspaceInfo {
            root: PathBuf::from("/workspace"),
            crates: vec![
                CrateInfo {
                    name: "crate-a".to_string(),
                    root: PathBuf::from("/workspace/crate-a"),
                    src_dirs: vec![],
                    tests_dir: None,
                    examples_dir: None,
                    benches_dir: None,
                },
                CrateInfo {
                    name: "crate-b".to_string(),
                    root: PathBuf::from("/workspace/crate-b"),
                    src_dirs: vec![],
                    tests_dir: None,
                    examples_dir: None,
                    benches_dir: None,
                },
                CrateInfo {
                    name: "crate-c".to_string(),
                    root: PathBuf::from("/workspace/crate-c"),
                    src_dirs: vec![],
                    tests_dir: None,
                    examples_dir: None,
                    benches_dir: None,
                },
            ],
        };

        let filtered = workspace.filter_by_names(&["crate-a", "crate-c"]);
        assert_eq!(filtered.crates.len(), 2);
        assert!(filtered.get_crate("crate-a").is_some());
        assert!(filtered.get_crate("crate-b").is_none());
        assert!(filtered.get_crate("crate-c").is_some());
    }

    #[test]
    fn test_crate_names() {
        let workspace = WorkspaceInfo {
            root: PathBuf::from("/workspace"),
            crates: vec![
                CrateInfo {
                    name: "alpha".to_string(),
                    root: PathBuf::from("/workspace/alpha"),
                    src_dirs: vec![],
                    tests_dir: None,
                    examples_dir: None,
                    benches_dir: None,
                },
                CrateInfo {
                    name: "beta".to_string(),
                    root: PathBuf::from("/workspace/beta"),
                    src_dirs: vec![],
                    tests_dir: None,
                    examples_dir: None,
                    benches_dir: None,
                },
            ],
        };

        let names = workspace.crate_names();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_is_cargo_project() {
        // Test with temp directory
        let temp = tempfile::tempdir().unwrap();
        let temp_path = temp.path();

        // Not a cargo project initially
        assert!(!is_cargo_project(temp_path));

        // Create Cargo.toml
        std::fs::write(temp_path.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();

        // Now it's a cargo project
        assert!(is_cargo_project(temp_path));
        assert!(is_cargo_project(temp_path.join("Cargo.toml")));
    }

    #[test]
    fn test_crate_info_contains_path() {
        let crate_info = CrateInfo {
            name: "my-crate".to_string(),
            root: PathBuf::from("/workspace/my-crate"),
            src_dirs: vec![PathBuf::from("/workspace/my-crate/src")],
            tests_dir: Some(PathBuf::from("/workspace/my-crate/tests")),
            examples_dir: None,
            benches_dir: None,
        };

        let workspace_root = PathBuf::from("/workspace");

        // Absolute paths
        assert!(
            crate_info.contains_path(Path::new("/workspace/my-crate/src/lib.rs"), &workspace_root)
        );
        assert!(crate_info.contains_path(
            Path::new("/workspace/my-crate/tests/test.rs"),
            &workspace_root
        ));
        assert!(!crate_info.contains_path(
            Path::new("/workspace/other-crate/src/lib.rs"),
            &workspace_root
        ));

        // Relative paths
        assert!(crate_info.contains_path(Path::new("my-crate/src/lib.rs"), &workspace_root));
        assert!(!crate_info.contains_path(Path::new("other-crate/src/lib.rs"), &workspace_root));
    }

    #[test]
    fn test_workspace_crate_for_path() {
        let workspace = WorkspaceInfo {
            root: PathBuf::from("/workspace"),
            crates: vec![
                CrateInfo {
                    name: "crate-a".to_string(),
                    root: PathBuf::from("/workspace/crate-a"),
                    src_dirs: vec![PathBuf::from("/workspace/crate-a/src")],
                    tests_dir: None,
                    examples_dir: None,
                    benches_dir: None,
                },
                CrateInfo {
                    name: "crate-b".to_string(),
                    root: PathBuf::from("/workspace/crate-b"),
                    src_dirs: vec![PathBuf::from("/workspace/crate-b/src")],
                    tests_dir: None,
                    examples_dir: None,
                    benches_dir: None,
                },
            ],
        };

        // Find crate by absolute path
        let crate_a = workspace.crate_for_path(Path::new("/workspace/crate-a/src/lib.rs"));
        assert!(crate_a.is_some());
        assert_eq!(crate_a.unwrap().name, "crate-a");

        let crate_b = workspace.crate_for_path(Path::new("/workspace/crate-b/src/main.rs"));
        assert!(crate_b.is_some());
        assert_eq!(crate_b.unwrap().name, "crate-b");

        // Non-existent crate
        let none = workspace.crate_for_path(Path::new("/workspace/crate-c/src/lib.rs"));
        assert!(none.is_none());

        // Relative paths
        let crate_a_rel = workspace.crate_for_path(Path::new("crate-a/src/lib.rs"));
        assert!(crate_a_rel.is_some());
        assert_eq!(crate_a_rel.unwrap().name, "crate-a");
    }
}
