# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- **Changed**:
  - Rewritten README: tighter intro, real output examples, usage split by counting/diffs/output
  - Improved CLI `--help` text: clearer arg descriptions, inline usage hints, examples footer
  - Added examples section to `rustloc --help` and `rustloc diff --help`
  - Removed redundant value listings from `--type` help (clap already appends possible values)
  - Right-aligned numeric columns and headers for easier scanning
  - Alternating dim rows for visual contrast in long listings
  - Continuous line separators (`─`) instead of dashes

## [0.8.1] - 2026-02-09

## [0.8.0] - 2026-02-03

## [0.7.2] - 2026-01-30

## [0.7.0] - 2026-01-16

- **Changed**:
  - File paths in `--by-file` output now display relative to workspace root instead of absolute paths
  - Table values are now center-aligned to match column headers
  - Updated `outstanding` dependency to v2.2.0
- **Added**:
  - Bold styling for table headers (using outstanding's theme support)
  - `root` field in `CountResult` and `DiffResult` for workspace root path

## [0.6.0] - 2026-01-15

- **Added**:
  - Precomputed `all` field in `Locs` struct storing total line count (sum of all types)
  - `all` toggle in `LineTypes` to control showing the "All" column
  - `LineTypes::everything()` method to include all 7 line types
  - `with_all()` and `without_all()` builder methods for `LineTypes`
  - `recompute_all()` method on `Locs` for manual field updates
- **Changed**:
  - Default columns changed from all 6 types to: Code, Tests, Docs, All
  - Table column "Total" renamed to "All" and now uses precomputed field
  - `LineTypes::new()` now enables `all` by default
  - Renamed `LineTypes::all()` to `LineTypes::everything()`

## [0.5.0] - 2026-01-15

- **Changed**:
  - Unified output format for counts and diffs: both now use same table layout (rows=objects, columns=Code/Tests/Examples/Total)
  - Bare `rustloc` now shows consistent table format with "Total (N files)" row instead of separate header-only view
  - Diff output shows diff values (+added/-removed/net) in each cell, matching count layout
  - Diff CSV format now matches count CSV structure with context columns
- **Added**:
  - `CellValue` enum in library to represent both count and diff values uniformly
  - `StatsRow` struct for unified display row representation
  - `LocStatsDiff::to_stats_row()` for converting diff stats to unified format

## [0.4.0] - 2026-01-11

- **Added**:
  - Working directory diff support: `rustloc diff` now shows uncommitted changes vs HEAD
  - `--staged`/`--cached` flag to show only staged changes (like `git diff --cached`)

## [0.3.1] - 2026-01-11

- **Changed**:
  - Replaced custom release script with `cargo-release` for standardized release workflow
  - Backfilled CHANGELOG with historical releases
- **Fixed**:
  - Enabled previously ignored doc-tests with proper fixtures

## [0.3.0] - 2026-01-10

- **Changed**:
  - Renamed types for consistent naming across codebase
  - Standardized naming conventions throughout codebase
- **Fixed**:
  - Show relative paths instead of absolute paths in file output

## [0.2.0] - 2026-01-10

- **Added**:
  - `--by-module` / `-m` option for module aggregation
  - `--type` filter to select code contexts (code, tests, examples)
- **Changed**:
  - Renamed CLI options for consistency
  - Move line type filtering to library, simplify CLI
  - `--type` now filters code contexts instead of line types

## [0.1.0] - 2026-01-10

- **Added**:
  - Git diff LOC analysis (`diff` command for comparing commits)
  - Diff CLI command for git LOC comparison
- **Fixed**:
  - Fetch full git history in CI for diff tests
  - Count test code in all contexts for diff output

## [0.0.4] - 2025-01-09

- **Fixed**:
  - Test automated crates.io publishing

## [0.0.3] - 2025-01-09

- **Fixed**:
  - Make crates.io publishing conditional on token availability

## [0.0.2] - 2025-01-09

- **Fixed**:
  - Release workflow permissions for creating GitHub releases

## [0.0.1] - 2025-01-09

- **Added**:
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
- **Acknowledgments**:
  - Parsing logic adapted from [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc) by Maxim Gritsenko

[Unreleased]: https://github.com/arthur-debert/rustloc/compare/v0.8.1...HEAD
[0.8.1]: https://github.com/arthur-debert/rustloc/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/arthur-debert/rustloc/compare/v0.7.2...v0.8.0
[0.7.2]: https://github.com/arthur-debert/rustloc/compare/v0.7.0...v0.7.2
[0.7.0]: https://github.com/arthur-debert/rustloc/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/arthur-debert/rustloc/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/arthur-debert/rustloc/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/arthur-debert/rustloc/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/arthur-debert/rustloc/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/arthur-debert/rustloc/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/arthur-debert/rustloc/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/arthur-debert/rustloc/compare/v0.0.4...v0.1.0
[0.0.4]: https://github.com/arthur-debert/rustloc/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/arthur-debert/rustloc/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/arthur-debert/rustloc/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/arthur-debert/rustloc/releases/tag/v0.0.1
