# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-01-10

### Changed

- Renamed types for consistent naming across codebase
- Standardized naming conventions throughout codebase

### Fixed

- Show relative paths instead of absolute paths in file output

## [0.2.0] - 2026-01-10

### Added

- `--by-module` / `-m` option for module aggregation
- `--type` filter to select code contexts (code, tests, examples)

### Changed

- Renamed CLI options for consistency
- Move line type filtering to library, simplify CLI
- `--type` now filters code contexts instead of line types

## [0.1.0] - 2026-01-10

### Added

- Git diff LOC analysis (`diff` command for comparing commits)
- Diff CLI command for git LOC comparison

### Fixed

- Fetch full git history in CI for diff tests
- Count test code in all contexts for diff output

## [0.0.4] - 2025-01-09

### Fixed

- Test automated crates.io publishing

## [0.0.3] - 2025-01-09

### Fixed

- Make crates.io publishing conditional on token availability

## [0.0.2] - 2025-01-09

### Fixed

- Release workflow permissions for creating GitHub releases

## [0.0.1] - 2025-01-09

### Added

- Initial release
- Rust-aware LOC counting that separates main, tests, examples, docs, comments, and blank lines
- Recognition of `#[test]` and `#[cfg(test)]` attributes to identify test code
- Context detection from file paths (`tests/`, `examples/` directories)
- Cargo workspace support with crate filtering
- Glob-based file filtering (include/exclude patterns)
- Multiple output formats: table, JSON, CSV
- Per-crate and per-file breakdown options
- `rustloclib` library for programmatic use
- `rustloc` CLI tool

### Acknowledgments

- Parsing logic adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc) by Maxim Gritsenko

[Unreleased]: https://github.com/arthur-debert/rustloc/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/arthur-debert/rustloc/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/arthur-debert/rustloc/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/arthur-debert/rustloc/compare/v0.0.4...v0.1.0
[0.0.4]: https://github.com/arthur-debert/rustloc/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/arthur-debert/rustloc/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/arthur-debert/rustloc/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/arthur-debert/rustloc/releases/tag/v0.0.1
