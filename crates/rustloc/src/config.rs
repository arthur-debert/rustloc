//! Rustloc application configuration.
//!
//! Clapfig owns discovery, sparse file parsing, defaults, and strict schema
//! validation. The CLI only asks for the resolved typed settings and then
//! decides whether a command-line override applies to the current render.

use clapfig::{Clapfig, SearchPath};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::PathBuf;

/// Settings read from `rustloc.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, clapfig::Schema)]
pub struct RustlocConfig {
    /// Show a percentage row below count table totals.
    #[clapfig(default = false)]
    pub shows_ratios: bool,
    /// Group integer digits in count, diff, and commit tables.
    ///
    /// `None` when rustloc.toml does not set it: grouping is then on unless
    /// `--number-fmt=false` turns it off. An explicit `number_fmt = false`
    /// turns grouping off unless `--number-fmt[=true]` turns it back on.
    pub number_fmt: Option<bool>,
}

impl RustlocConfig {
    /// Load the application configuration using rustloc's normal discovery.
    ///
    /// `SearchPath::Platform` is Clapfig's app-name default. `SearchPath::Cwd`
    /// adds project-local `rustloc.toml` discovery without parsing paths in
    /// rustloc itself; Clapfig still owns file lookup and validation.
    pub fn load() -> Result<Self, clapfig::ClapfigError> {
        Self::load_from_paths(vec![SearchPath::Platform, SearchPath::Cwd])
    }

    fn load_from_paths(paths: Vec<SearchPath>) -> Result<Self, clapfig::ClapfigError> {
        Clapfig::schema_builder::<Self>()
            .app_name("rustloc")
            .search_paths(paths)
            .load()
    }

    #[cfg(test)]
    pub fn load_from_dirs(
        dirs: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, clapfig::ClapfigError> {
        Self::load_from_paths(dirs.into_iter().map(SearchPath::Path).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_hiding_ratios() {
        let dir = tempfile::tempdir().unwrap();
        let config = RustlocConfig::load_from_dirs([dir.path().to_path_buf()]).unwrap();

        assert!(!config.shows_ratios);
        assert_eq!(config.number_fmt, None);
    }

    #[test]
    fn reads_shows_ratios_from_rustloc_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rustloc.toml"), "shows_ratios = true\n").unwrap();
        let config = RustlocConfig::load_from_dirs([dir.path().to_path_buf()]).unwrap();

        assert!(config.shows_ratios);
    }

    #[test]
    fn reads_number_fmt_from_rustloc_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rustloc.toml"), "number_fmt = true\n").unwrap();
        let config = RustlocConfig::load_from_dirs([dir.path().to_path_buf()]).unwrap();

        assert_eq!(config.number_fmt, Some(true));
    }

    #[test]
    fn reads_explicit_false_number_fmt_from_rustloc_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rustloc.toml"), "number_fmt = false\n").unwrap();
        let config = RustlocConfig::load_from_dirs([dir.path().to_path_buf()]).unwrap();

        assert_eq!(config.number_fmt, Some(false));
    }
}
