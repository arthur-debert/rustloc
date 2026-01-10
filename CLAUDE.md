# Architecture Guide for Coding Assistants

This document describes the design principles and architecture of rustloc to help coding assistants understand the codebase.

## Two-Crate Design

The project is split into two crates with a strict separation of concerns:

### `rustloclib` (Library)
The library is responsible for:
- **All computation and logic** - parsing, filtering, aggregation, diffing
- **Pure data types** - returns structured data that can be serialized/deserialized
- **Input configuration** - takes complete input (paths, filters, aggregation level, line types)
- **No I/O formatting** - returns data, never formats for display

### `rustloc` (CLI)
The CLI is responsible for:
- **Argument parsing** - convert CLI args to library types
- **Output formatting** - table, JSON, CSV formatting for display
- **No business logic** - never compute or filter data, just present what the library returns

## Key Design Principle: Library Does All Computation

When adding a feature, ask: "Where does the logic live?"

**Correct**: The library computes and filters data, CLI displays it.
```rust
// Library: takes Contexts, returns filtered data
let options = CountOptions::new().contexts(Contexts::main_only());
let result = count_workspace(path, options)?;  // Returns pre-filtered data

// CLI: just displays what it receives
print_table(&result);  // No filtering logic here
```

**Incorrect**: CLI doing computation.
```rust
// WRONG: CLI is filtering/computing
let result = count_workspace(path, options)?;
let filtered = result.filter(my_cli_filter);  // Should be done in library
```

## Core Library Types

### `rustloclib/src/options.rs`
Input configuration types that control what the library returns:

- `Contexts` - which code contexts to include (main, tests, examples)
- `Aggregation` - result granularity (Total, ByCrate, ByModule, ByFile)

### `rustloclib/src/stats.rs`
Output data types returned by the library:

- `Locs` - counts for a single context (blank, code, docs, comments)
- `LocStats` - aggregated stats separating main/tests/examples
- `FileStats`, `ModuleStats`, `CrateStats` - breakdown types
- All types implement `filter(&self, contexts: Contexts) -> Self`

### `rustloclib/src/counter.rs`
Counting API:

- `CountOptions` - configuration for counting (crates, filters, aggregation, contexts)
- `CountResult` - result containing total and optional breakdowns
- `count_workspace()`, `count_directory()`, `count_file()` - entry points

### `rustloclib/src/diff.rs`
Git diff API:

- `DiffOptions` - configuration for diffing
- `DiffResult` - result with added/removed stats
- `diff_commits()` - entry point for comparing commits

## Adding New Features

1. **Add configuration to library** - new fields in `CountOptions`/`DiffOptions`
2. **Implement logic in library** - filtering, aggregation, etc.
3. **Expose via `lib.rs`** - add to public exports
4. **Update CLI to use** - convert CLI args to library types

Example workflow for adding a "minimum lines" filter:

```rust
// 1. Add to library options
pub struct CountOptions {
    pub min_lines: Option<u64>,  // New field
    // ...
}

// 2. Implement in count_workspace()
if let Some(min) = options.min_lines {
    result.files.retain(|f| f.stats.total() >= min);
}

// 3. Export in lib.rs (if new types)

// 4. CLI parses --min-lines arg and passes to CountOptions
let options = CountOptions::new().min_lines(args.min_lines);
```

## Serialization

Library types derive `serde::Serialize`/`Deserialize`. JSON output is just:
```rust
println!("{}", serde_json::to_string_pretty(&result)?);
```

No special JSON transformation in CLI - the library types are the JSON schema.

## Testing

- **Library tests**: Unit tests for computation logic in `rustloclib/src/*/tests`
- **CLI tests**: Integration tests in `rustloc/tests/cli_integration.rs`
