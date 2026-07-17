# Output compatibility fixtures

These pin the **public** JSON and CSV surface of `rustloc count` so that any
change to it has to be made on purpose, in a diff a reviewer can see.

All fixtures are generated from the deterministic sample tree built by
`sample_tree()` in `../cli_integration.rs` — a fixed two-file source tree in a
TempDir. They deliberately do **not** count the rustloc repo itself, whose
numbers move with every commit and would make the fixtures churn.

| File | What it is |
| --- | --- |
| `count_by_file.before.json` | `--by-file --output json`, **before** issue #119 |
| `count_by_file.after.json` | the same command **after** — asserted by the test suite |
| `count_by_file.csv` | `--by-file --output csv` — byte-identical before and after |

## The one intentional change

Issue #119 made each command return a single canonical response that is
independent of `--output`. Previously the handlers inspected Standout's
`_output_mode` argument and built *different data* per mode: structured modes
forced `LineTypes::everything()`, while table modes filtered the stats down to
the requested `--type` set.

The full public delta is three booleans:

```diff
   "line_types": {
-    "blanks": true,
+    "blanks": false,
     "code": true,
-    "comments": true,
+    "comments": false,
     "docs": true,
-    "examples": true,
+    "examples": false,
     "tests": true,
     "total": true
   },
```

**Every count is unchanged**, in JSON and in CSV. Only the `line_types`
metadata moved.

### Rationale

`line_types` used to be hardcoded to all-true in structured output, because
structured modes overrode the user's `--type` selection. It described the
serializer, not the request, and so carried no information — it was all-true on
every JSON document rustloc had ever emitted.

It now reports **what the caller actually asked to see**. That is the field's
only defensible meaning once the response is mode-independent: the counts are
always complete, and `line_types` is the view descriptor the render layer uses
to choose table columns. A consumer that wants "all types" still gets all types
— they are all right there in `total` and `stats`, as numbers, exactly as
before.

The alternative — honouring `--type` by zeroing the data in structured output —
was rejected: it would have changed *numbers* rather than metadata, which is a
far worse break, and it would make a display flag silently corrupt a
machine-readable payload.

### Behaviour this also fixes

Because filtering and ordering now evaluate against real counts rather than
display-zeroed ones, predicates and `--ordering` on a line type that `--type`
doesn't display now work in table mode. They previously matched nothing:

```console
$ rustloc . --by-crate --blanks-gte 500     # before: "Total (0 of 2 crates)"
$ rustloc . --by-crate --blanks-gte 500     # after:  matches on real blank counts
```

The same query already behaved correctly under `--output json`. That divergence
was the mode-dependence this workstream removes.

## Regenerating

The fixtures are asserted by `test_count_json_matches_compat_fixture` and
`test_count_csv_matches_compat_fixture`. If one fails, that is the signal to
decide whether the change is intended — not to blindly regenerate. When it is
intended, update the `after`/`csv` fixture and record the reasoning here.
`count_by_file.before.json` is a historical record of the #119 baseline and
should not be regenerated.
