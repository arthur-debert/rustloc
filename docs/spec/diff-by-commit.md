# Diff by Commit

`rustloc diff --by-commit` reports the line-count changes introduced by each
commit selected by a Git revision range.

## Context

`rustloc diff` currently compares two repository states and can group the
result by crate, module, or file. It cannot show how the changes accumulated
across the commits between those states. Users must run the command separately
for every commit and combine the results themselves.

The existing `rustloc commit <revision>` command defines one commit's changes
as the comparison between that commit and its first parent. Diff by Commit
extends that meaning across a Git revision range.

## Problem

An endpoint diff answers what changed between two states but hides when each
change entered the history. This makes it difficult to identify commits that
added or removed large amounts of production code, tests, documentation, or
other line types.

## Goals

- Add `--by-commit` to `rustloc diff`.
- Select commits using the diff command's existing Git revision semantics.
- Report one row for every selected commit.
- Preserve the existing diff columns, filters, output formats, and narrowing
  controls.
- Keep the commit hash and the beginning of its subject readable in human
  tables.
- Leave every existing command and aggregation unchanged when the option is
  absent.

## Non-Goals

- Combining commit rows with file, crate, or module rows.
- Representing working-tree or staged changes as synthetic commits.
- Displaying authors, dates, signatures, or the commit graph.
- Changing the behavior of `rustloc commit`.

## Proposed Shape

The command accepts `--by-commit` after `diff`:

```text
rustloc diff HEAD~5..HEAD --by-commit
rustloc diff main feature --by-commit
rustloc diff main...feature --by-commit
rustloc diff main --by-commit
```

`--by-commit` conflicts with `--by-file`, `--by-crate`, `--by-module`, and
`--staged`. A revision argument is required because a working tree contains no
commits to enumerate.

Rustloc selects commits as follows:

- `A..B` selects commits reachable from `B` but not from `A`.
- `A B` is equivalent to `A..B`.
- `A...B` selects commits from the merge base of `A` and `B` through `B`,
  following `git diff A...B` rather than the symmetric history selected by
  `git log A...B`.
- `A` selects commits reachable from `HEAD` but not from `A`.

The left endpoint is excluded. Every selected commit produces a row, including
empty commits and commits whose changes do not match the active language or
path filters.

Each commit is compared with its first parent. Merge commits use the same
comparison. A selected root commit is compared with an empty tree.

When the user does not provide `--ordering`, rows appear in the order
`git rev-list <range>` emits: every commit precedes its parents, and commits
the parent constraint does not relate are ordered by descending commit
timestamp with Git's own tie-break. Delegating to that traversal makes the
default deterministic for a given repository, including around merges and
equal timestamps. `--ordering` remains the opt-in override and uses the
existing diff rules: label ordering compares complete row
labels, while numeric ordering compares net changes. Predicates filter commit
rows by their net values, and `--top` runs after filtering and ordering.

Each complete row label has this form:

```text
868ed442 Add per-commit diff aggregation
```

The hash is the first eight lowercase hexadecimal characters of the commit ID.
The subject is the first logical line of the commit message with line-breaking
whitespace normalized to spaces. A missing subject appears as `(no subject)`.

Structured formats retain the complete label. Human tables preserve the hash
and following space, then use the remaining label-column width for the start of
the subject. Long subjects truncate at the end with an ellipsis. Truncation
uses terminal display width and does not split a UTF-8 character. Human
rendering treats commit text as plain data: rustloc escapes subjects before
they reach the semantic-markup renderer, so every `[` and `]` sequence —
including a valid semantic tag name such as `[bold]` — appears literally in
the table and cannot introduce styling.

The total row sums all selected commit rows before predicates or `--top` are
applied. It therefore measures commit churn, not only the difference between
the endpoint trees. Adding lines in one commit and removing them in another
contributes to both the added and removed totals. A merge commit's first-parent
comparison can count changes also reported by commits from its merged branch.

The report's file count is the number of distinct analyzed files touched by the
selected commits. The skipped-changes summary accumulates unsupported-file
additions and removals across those commits.

Human output names the label column `Commit` and the total unit `commits`.
JSON, YAML, and XML identify the aggregation as `ByCommit` and contain one diff
item per commit. CSV contains one row per commit followed by the existing
`TOTAL` row.

## User / Agent Stories

1. As a developer, I want one row per commit in a revision range so that I can
   see when line-count changes entered the history.
2. As a reviewer, I want commit rows in Git's default traversal order so that
   I can read recent changes first without requesting an explicit sort.
3. As a script author, I want complete labels and numeric values in structured
   output so that terminal truncation does not discard data.
4. As a user who supplied incompatible options, I want a usage error before
   repository analysis begins so that I can correct the command quickly.
5. As a maintainer, I want existing diff aggregations to retain their schemas
   and approved rendering when `--by-commit` is absent.

## Risks And Rabbit Holes

- Per-commit rows measure history churn, while an endpoint diff measures final
  state. Tests and help text must keep that distinction explicit.
- Revision ranges can contain merges, empty commits, root commits, and missing
  parent objects in shallow repositories. Rustloc must not silently omit a
  selected commit.
- Long histories can repeat expensive repository-state analysis. The
  implementation should reuse analysis for repository trees that occur in
  adjacent comparisons.
- Commit subjects are untrusted text. The human renderer resolves semantic
  `[tag]` markup, and labels are not escaped today, so the implementation must
  add the escaping step rather than assume subjects pass through inertly.

## Cross-Cutting Concerns

Missing commits or parents produce a clear Git error. Existing output schemas
remain unchanged for every other aggregation. The implementation uses the
repository's existing Git integration and adds no network access.

The command should avoid resolving the same range or analyzing the same tree
once per row. Runtime should grow with the number of selected commits and
unique repository trees rather than repeating both endpoints for every commit.

## Testing / Verification

Library tests use temporary Git repositories to verify two-dot, three-dot,
single-revision, and two-positional-revision selection. They cover diverged
histories, merge commits, roots, empty commits, missing shallow-history parents,
and commits whose selected statistics are zero.

Query tests verify the default traversal order — including a merge history and
commits with equal timestamps — explicit label and numeric
ordering, predicates followed by `--top`, churn totals, distinct file counts,
and accumulated skipped changes.

Pipeline tests verify option conflicts, the required revision, human
`Commit`/`commits` wording, all structured formats, and labels containing long
ASCII text, CJK characters, emoji, and bracketed text. A subject containing a
valid semantic tag name must render its brackets literally with no styling
applied. Existing approved fixtures must remain unchanged.

The completed implementation passes:

```text
pixi run test
pixi run lint
```

## Workstream Hints

The implementation should keep revision walking, commit metadata, tree reuse,
and per-parent comparisons inside the existing reusable diff module. The CLI
adds the diff-only option and projects the returned commit records through the
existing query and presentation paths.

## Out Of Scope

The feature does not add author/date columns, graph rendering, combined merge
diffs, working-tree rows, or commit-by-file matrices.

## Further Notes

None.
