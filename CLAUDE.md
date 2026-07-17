@AGENTS.md

# Architecture Guide for Coding Assistants

rustloc is a two-crate application with an explicit boundary between reusable
analysis and shell presentation. Preserve this flow:

```text
typed request -> application -> canonical queryset -> presentation
```

Clap and Standout surround that flow at the CLI boundary; they are not part of
the reusable library.

## Ownership boundary

### `rustloclib`: reusable analysis

The library owns source discovery, language backends, counting, Git diffing,
filtering and ordering primitives, aggregation, and the canonical typed
responses `CountQuerySet` and `DiffQuerySet`. It returns numbers and metadata.

The library does **not** own tables, headers, display widths, footer wording,
semantic style tags, colours, output modes, stdout/stderr, or file output. It
ends at canonical typed querysets; callers decide how to present them.

### `rustloc`: command application and presentation

The CLI owns command syntax, command-specific validation and orchestration,
Standout dispatch, presentation adapters, templates and CSS, serialization,
final writes, streams, and process exit behavior.

| Surface | Owns | Must not own |
| --- | --- | --- |
| `app` | Builds the same Clap command and Standout `App` for `main` and tests; registers dispatch, templates, and the merged theme | Environment, argv, command logic, writes |
| `command` | Converts `ArgMatches` into `CountRequest` / `DiffRequest`; validates syntax and values | Library calls, output-mode decisions |
| `application` | Orchestrates library entry points and canonical query operations from typed requests | Clap types, rendering, output mode |
| `handlers` | Thin Standout bridge: parse request, call application, return `Output::Render(queryset)` | Presentation branches or writes |
| `presentation` | Post-dispatch render boundary; selects direct serialization, the CSV row projection, or a human table view | Command logic or analysis |
| `table` | Adapts canonical querysets into typed `CountView` / `DiffView` data for templates | Display strings, widths, style tags |
| `templates/` | Human wording, table layout, widths, alignment, diff notation, semantic style tags | Computation or machine-readable schemas |
| `styles/` | Maps semantic tags to terminal appearance | Business meaning or raw output text |
| `main` | Reads process argv, writes the final result, owns stdout/stderr, maps `RunResult` to `ExitCode` | Command or presentation policy |

`filter_args` legitimately registers and extracts the synthetic
`--<field>-<op>` grid. `presentation` legitimately reads Standout's injected
`_output_mode` argument. Handlers accept `ArgMatches` only to hand them to
`command`. These are boundary mechanics, not permission to re-derive command
logic downstream.

## Why Standout is the application seam

Handlers remain output-mode-independent and return one canonical response.
Standout then applies the selected presentation without changing command or
application logic:

- JSON, YAML, and XML serialize the canonical queryset directly.
- CSV uses one isolated flat-row adapter because tabular serialization needs a
  top-level sequence with stable columns.
- Human modes adapt to `CountView` / `DiffView`, render MiniJinja templates,
  and resolve semantic tags through the CSS theme.
- Standout owns output-mode flags and final output-file handling.

This keeps one command implementation useful to humans and automation while
making each layer independently testable. Structured modes must never pass
through a table adapter or template, and handlers must never inspect output
mode.

## Presentation rules

Rust supplies typed values and data keys; MiniJinja decides what a human reads.
Put these in `crates/rustloc/templates/`, not Rust:

- header words, titles, legends, footers, and unit nouns;
- widths, padding, alignment, and truncation choices;
- `+added/-removed/net` notation and semantic `[tag]` markup.

Put concrete terminal styling in `crates/rustloc/styles/default.css`. Never emit
raw ANSI from Rust or templates. `table` may select columns from `line_types`
and repair stale `total_items`; neither is display wording.

Approved render fixtures live in
`crates/rustloc/tests/fixtures/render/`. When a deliberate presentation change
is required, regenerate them with:

```bash
UPDATE_RENDER_FIXTURES=1 cargo test -p rustloc
```

Review the resulting fixture diff; regeneration is not approval by itself.

## Testing pyramid

Use the smallest boundary that can observe the contract. A real file or Git
fixture does not by itself require a subprocess.

| Level | Covers | Location |
| --- | --- | --- |
| Direct | Typed request parsing and validation; orchestration; counting/diffing; filtering, ordering, aggregation; canonical responses | `src/command.rs`, `src/application.rs`, and `rustloclib` unit tests |
| In-process pipeline | Clap routing, handlers, post-dispatch adapters, templates, themes, and structured serializers through the real Standout app | `src/pipeline_tests.rs` |
| Process | Binary startup/linking, OS exit codes, stdout/stderr ownership, final output-file writes, and ambient child seams such as cwd or forced colour | `tests/cli_integration.rs` |

Every process test must state its process-only contract. Keep behavior coverage
out of that suite when a direct or pipeline test can observe it.

### Why the pipeline uses `run_to_string`

rustloc depends on Standout **7.6.2**. A compatible published
`standout-test` is not available: crates.io has `standout-test` only through
7.5.1, whose exhaustive `RunResult` matches do not compile after 7.6 added the
non-exhaustive `Error` variant. Upstream's 7.6-era harness is unpublished, and
using its Git source introduces a second Standout instance whose `App` type is
incompatible with rustloc's registry dependency.

Therefore `src/pipeline_tests.rs` drives `app::app()` and
`app::cli_command()` directly with `App::run_to_string`. This is the same
in-process argv-to-render pipeline that the harness wraps. The tests use
absolute fixture paths and do not mutate process-global detectors, so they can
run in parallel. Do not claim `TestHarness` is currently available, add an
unpublished dependency, or add `#[serial]` without a new global seam.

Ambient state that `run_to_string` cannot safely model stays in the small
process suite. The forced-colour case, for example, sets `CLICOLOR_FORCE=1` on
a child rather than changing the test runner's global detector.

## Review checklist

- Does CLI syntax become a typed request exactly once?
- Does `application` return a canonical `CountQuerySet` / `DiffQuerySet` with
  no output-mode branch?
- Do handlers return `Output::Render` rather than print or render?
- Are machine modes independent of templates and table views?
- Are all human-facing words and semantic tags in templates, with appearance
  in CSS?
- Is each test at the smallest boundary that can observe its contract?
- If a process test remains, does its comment name the OS/process fact?

## Standout theme gotchas

- The CSS parser silently drops unsupported properties. Use `dim: true`, not
  the documented-but-unimplemented `opacity`, and assert the resolved theme
  attributes.
- An unknown semantic tag does not fail rendering. Test tag/theme agreement
  against the theme; `term-debug` preserves both known and unknown tags.
- `table_row_odd` comes from `Theme::default()` and is adaptive. The app merges
  `styles/default.css` over that default; do not replace it with a flat rule.

## Verification

```bash
pixi run test
pixi run lint
```

Both are commit/push checks and must pass before opening a draft PR.
