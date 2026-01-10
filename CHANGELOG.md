# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
