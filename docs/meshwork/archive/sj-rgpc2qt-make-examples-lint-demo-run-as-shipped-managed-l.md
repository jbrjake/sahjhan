---
id: sj-rgpc2qt
title: "Make examples/lint-demo run as shipped, managed listing output under [paths], fix_budget reading the events table's type column, and a test that inits and merges it"
category: examples/lint-demo
labels: [project-sites, project-site]
seq: 10
answers: jbrubin-net#jb-q2b1eaq
docs:
  - examples/lint-demo/protocol.toml
  - docs/lint.md
  - jbrubin-net#docs/superpowers/specs/2026-09-24-project-sites-design.md#12-evidence
verify: "all(exists examples/lint-demo/protocol.toml, contains examples/lint-demo/protocol.toml /managed = \\[\"output\"\\]/, lacks examples/lint-demo/protocol.toml event_type, run cargo test target=lint_tests test_lint_demo_example_inits_and_merges)"
status: done
created: 2026-09-29T02:01Z
---
The example passes all eight lint checks and cannot run. `sahjhan init` on it exits 3 with
`E12: data_dir 'output/.sahjhan' is not under any managed path []` and leaves a one-entry
`ledger.jsonl` and a `ledgers.toml` behind with no manifest. With `[paths]` fixed, `transition
merge` is refused with `Schema error: No field named event_type`: `[queries.fix_budget]` says
`WHERE event_type = 'fix_commit'` and the events table's column is `type`. Lint checks the graph
and does not run the SQL, which is why nothing caught it.

1. `examples/lint-demo/protocol.toml`: `managed = ["output"]` (line 11), and `type` for
   `event_type` in the query (line 21). Nothing else in the example changes.
2. A test in `tests/lint_tests.rs` named `test_lint_demo_example_inits_and_merges`, beside the
   two `test_lint_demo_example_*` tests already there and in their idiom (`assert_cmd::Command::
   cargo_bin("sahjhan")`, `tempfile::TempDir`): copy the example directory to a temp dir, run
   `init` there and assert exit 0, then `event fix_commit --field sha=3f9c2e1` and
   `transition merge` and assert exit 0 and the `fix_loop → merge_done` line. Run it on the
   unfixed example first and watch it fail.
3. The CLAUDE.md documentation rule: no source file changes, so no `// ## Index` update, but the
   test file's own index header gains the new test if that file carries one.
4. The release that carries this is what jbrubin-net's tape will pin, so the close comment names
   the commit and, when cut, the release tag. Cutting the release is the owner's.

Done when: the four verify arms pass (the `cargo test` arm builds one test binary and takes a few
minutes), the commit is `fix(examples): lint-demo runs as shipped -- managed output, and type not
event_type`, and the close comment names the commit.

## log
- 2026-09-29T02:01Z created
- 2026-09-30T15:11Z open→doing — claimed by claude (e16f684c-ee66-4925-aac7-b58aedbf0707)
- 2026-09-30T15:14Z doing→done — verify exit 0 @ 18f26cc+5

## comments
- 2026-09-30T15:14Z [claude (e16f684c-ee66-4925-aac7-b58aedbf0707)] Fixed in 18f26cc (fix(examples): lint-demo runs as shipped -- managed output, and type not event_type). Not yet in a release: the latest tag is v0.28.x and cutting the release is the owner's. Once it is cut, a follow-up comment here names the tag, which is what jbrubin-net's tape pins. The new test test_lint_demo_example_inits_and_merges was watched failing twice on the unfixed example: first on init (E12, exit 3), then, with only managed fixed, on merge (Schema error: No field named event_type, exit 1). It passes with both fixes, as do all 73 lint_tests. docs/lint.md already spells fix_budget with type, so no prose changed. Found while reproducing: a failed init leaves a sealed genesis behind that wedges the retry. Filed as its own task, see relates.
