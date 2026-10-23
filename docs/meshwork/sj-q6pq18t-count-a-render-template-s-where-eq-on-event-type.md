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
status: open
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
