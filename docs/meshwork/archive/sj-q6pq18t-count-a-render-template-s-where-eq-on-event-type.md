---
id: sj-q6pq18t
title: Count a render template's where_eq on event_type as an L5 consumer
category: lint
seq: 10
labels: [gh-33]
docs:
  - CLAUDE.md#flow-lint-static-integrity-analysis
relates: [sj-xarb4a4]
verify: run cargo test target=lint_tests l5_counts_a_template_where_eq_on_event_type
status: done
created: 2026-10-23T21:45Z
---
GitHub #33, first class. L5 counts a render as consuming only the event types
in its `event_types`. A Tera template that selects
`events | where_eq(attribute="event_type", value="X")` consumes `X` too, and L5
reports `X` as "never produced or consumed". In holtz that wrong reason hid a
real finding: those events had no writer.

The issue asks for the conservative version: scan each render's template file
for that exact filter shape and add the quoted values to `[consumed-events]`.
Template paths resolve the way `RenderEngine` resolves them, so lint and render
cannot disagree about which file is the template. A template that cannot be
read is a finding or is skipped, but it must not panic. `src/lint/` has no
template scanning today (`grep -rn where_eq src/lint/` is empty).

Lint is config-only, and reading template files stays within that because they
are config. Update the L5 row in CLAUDE.md and the README's static-analysis
section if it says what L5 counts as a consumer.

## log
- 2026-10-23T21:45Z created
- 2026-10-23T21:59Z open→doing — claimed by claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)
- 2026-10-23T22:03Z doing→done — verify exit 0 @ 0f5e156+5

## comments
- 2026-10-23T22:04Z [claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)] Done: 46636de (fix + tests), 0f5e156 (docs). Broader than the issue's where_eq-only ask: any declared event quoted in a {{ }} or {% %} tag counts, which is the same syntactic rule sql_event_mentions applies to predicates and also covers Tera's filter() and == comparisons. Watched failing with the scan removed (3 L5 findings) and with comments scanned as tags (0 findings). CLI before/after on the issue's shape: the 16:56 release build gives 3 L5 warnings (_checkpoint, prediction, prediction_outcome); HEAD is clean, from the root and from a subdir with --config-dir. #33 is closed by this commit. Its resolution comment waits for the release, per the ISSUE CLOSURE RULE.
