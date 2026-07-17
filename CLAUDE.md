@AGENTS.md

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

#### CLI internal layering

The CLI has its own seam so no single layer spans "clap syntax → library call →
formatted output". Each module owns one step, in order:

| Module | Owns | Never touches |
| --- | --- | --- |
| `app` | The **construction factory**. Deterministic, pure functions building the Standout app (templates, theme, dispatch) and the Clap command (derive + injected filter grid). Used by `main` *and* by tests. | Environment, argv, I/O |
| `command` | The **parsing boundary**. The only place that interprets `ArgMatches` as command logic; converts CLI syntax into typed requests (`CountRequest` / `DiffRequest`), failing fast on invalid values. | Library calls, output mode |
| `application` | Typed **orchestration**. Takes a request, picks the `rustloclib` entry point, returns the canonical response. | clap types, output mode |
| `handlers` | The **dispatch bridge**. Request → orchestration → `Output::Render`. Three lines each. | Everything else |
| `presentation` | The **render boundary**. The one place allowed to read the output mode and pick table / CSV / direct serialization. | Command logic |
| `table` | Formatting a response into a `LOCTable`. | Command logic |

`main` is what remains once construction moves to `app`: read `std::env::args`,
write the result, map `RunResult` to an exit code. Those three are genuinely
process-level, which is exactly why they are the only things a test has to
spawn a process to observe.

Two modules outside `command` legitimately name `ArgMatches`, and neither
breaks the boundary — don't "fix" them:

- `filter_args` owns both ends of the synthetic `--<field>-<op>` predicate grid
  (it registers the 42 hidden args and reads them back). `command` calls its
  `extract`, so the grid stays a detail of the module that invents it instead
  of 42 cases at the parsing boundary.
- `presentation` reads the single injected `_output_mode` arg. That is a render
  decision made at the render boundary, not command logic.

`handlers` also take `&ArgMatches`, purely to pass it to `command`. So the rule
to enforce is not "only `command` may name the type" but: **CLI syntax becomes
typed values at the parsing boundary, and nothing downstream of `application`
re-derives command logic from matches.**

Two more rules follow from this, and reviewers should enforce both:

- **Conversions that can fail, fail at the parsing boundary** as clap usage
  errors — never with a silent fallback to a default further downstream. An
  invalid `--ordering` must exit non-zero, not quietly sort by label.
- **Orchestration is directly testable.** `application::count`/`diff` take a
  request struct, so tests construct one instead of building an `ArgMatches`.

Command-specific orchestration belongs in `application`, not `rustloclib`:
"`--by-crate` requires a workspace" is a rule about *this CLI's flags*. Only
genuinely reusable domain behavior moves into the library.

## Key Design Principle: Library Does All Computation

When adding a feature, ask: "Where does the logic live?"

**Correct**: The library computes and filters data, CLI displays it.

```rust
// Library: takes Contexts, returns filtered data
let options = CountOptions::new().contexts(Contexts::code_only());
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

### `crates/rustloclib/src/query/options.rs`

Input configuration types that control what the library returns:

- `Contexts` - which code contexts to include (code, tests, examples)
- `Aggregation` - result granularity (Total, ByCrate, ByModule, ByFile)

### `crates/rustloclib/src/data/stats.rs`

Output data types returned by the library:

- `Locs` - counts for a single context (blank, logic, docs, comments)
- `LocStats` - aggregated stats separating code/tests/examples
- `FileStats`, `ModuleStats`, `CrateStats` - breakdown types
- All types implement `filter(&self, contexts: Contexts) -> Self`

### `crates/rustloclib/src/data/counter.rs`

Counting API:

- `CountOptions` - configuration for counting (crates, filters, aggregation, contexts)
- `CountResult` - result containing total and optional breakdowns
- `count_workspace()`, `count_directory()`, `count_file()` - entry points

### `crates/rustloclib/src/data/diff.rs`

Git diff API:

- `DiffOptions` - configuration for diffing
- `DiffResult` - result with added/removed stats
- `diff_revspec()` - entry point for comparing commits (accepts a single git
  revspec string: `<rev>`, `<a>..<b>`, or `<a>...<b>`)
- `diff_workdir()` - entry point for diffing the working tree against HEAD

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

- **Library tests**: Unit tests for computation logic in `crates/rustloclib/src/*/tests`

The CLI is tested as a pyramid. Pick the **smallest layer that can observe the
behavior**, and don't assert the same fact at more than one — unless a past
regression justifies it (the table line-structure tests are the one such case).

| Layer | Covers | Where |
| --- | --- | --- |
| Unit | Parsing and orchestration as plain functions — no `ArgMatches` needed | `src/command.rs`, `src/application.rs` |
| Pipeline | argv → clap → handler → post-dispatch → render, in-process | `src/pipeline_tests.rs` |
| Process | Exit codes, stderr routing, executable integration, real Git | `tests/cli_integration.rs` |

The pipeline layer builds the app via `app::app()` / `app::cli_command()` — the
same construction `main` uses — and drives it through Standout's
`run_to_string`. Tests pass fixture paths as absolute, mutate no process-global
state, and so run in parallel without a serial guard.

`standout-test::TestHarness` is the intended tool for that middle layer and is
currently **unusable**: it is published only up to 7.5.1, which does not compile
against standout 7.6.x (`RunResult` gained a `#[non_exhaustive]` `Error`
variant), and upstream's 7.6.x is `publish = false`. Taking it from git pulls a
second standout into the graph, so the harness cannot accept our `App`. See the
module docs in `src/pipeline_tests.rs`. Ambient seams the harness would provide
— TTY, terminal width, colour capability, cwd, stdin — are therefore uncovered.
