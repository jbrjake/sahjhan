---
id: sj-55gzaec
title: "Make a refused sahjhan init write nothing, so the retry after fixing the config succeeds"
status: done
category: cli/init
discovered-from: sj-rgpc2qt
relates: [sj-rgpc2qt]
verify: run cargo test target=integration_tests test_init_refused_by_e12_writes_nothing_and_retries
docs:
  - src/cli/init.rs
seq: 20
created: 2026-09-30T15:15Z
---

A refused `sahjhan init` writes a sealed genesis and a registry, then wedges the retry. Found while
reproducing jbrubin-net#jb-q2b1eaq, which reports the leftover files as a symptom.

`cli/init.rs` `[cmd-init]` creates `data_dir`, writes `ledger.jsonl` (a genesis sealing the config
as it is now) and `ledgers.toml`, and only then calls `Manifest::init`. That call is pure and is
where E12 is raised. So on the shipped `examples/lint-demo`:

1. `init` exits 3 with `E12: data_dir 'output/.sahjhan' is not under any managed path []` and
   leaves `ledger.jsonl` + `ledgers.toml`, no manifest.
2. Fix `managed`, run `init` again: exit 4, `already initialized (...ledger.jsonl). run reset first.`
3. `status`: exit 2, `config integrity violation: protocol.toml`, because the leftover genesis
   sealed the broken config. The hint offers `reseal` or `init`. `reset` needs a daemon-signed
   proof.

The only way out is deleting `data_dir` by hand, and no message says so.

Fix: `init` checks everything that cannot write before it writes anything. Build the manifest
(E12) before `create_dir_all`, so a refused init leaves nothing, not even an empty `data_dir`. If
a write fails after the genesis exists (registry, manifest track, manifest save), remove the files
this call created before returning, so a failed init can always be retried.

Test in `tests/integration_tests.rs`, `test_init_refused_by_e12_writes_nothing_and_retries`: a
config with `managed = []` refused with E12 and no `data_dir` created, then the same directory with
`managed` fixed initialises and `status` exits 0. Watch it fail on the unfixed `cmd_init` first.

CLAUDE.md: `[cmd-init]` stays the anchor and no public item changes, so the lookup tables are
unaffected. The Test Files row for `integration_tests.rs` gains the new test. That is a doc edit,
left uncommitted for the owner.

## log
- 2026-09-30T15:15Z created
- 2026-09-30T15:15Z open→doing — claimed by claude (e16f684c-ee66-4925-aac7-b58aedbf0707)
- 2026-09-30T15:20Z doing→done — verify exit 0 @ 1ad66d1+8

## comments
- 2026-09-30T15:20Z [claude (e16f684c-ee66-4925-aac7-b58aedbf0707)] Fixed in 1ad66d1 (fix(init): a refused init writes nothing, so the retry succeeds). Not yet in a release. Before: init on the shipped lint-demo exited 3 and left ledger.jsonl and ledgers.toml; the retry with managed fixed exited 4 (already initialized); status exited 2 (config integrity violation). After: init exits 3 and creates nothing, not even output/; the retry exits 0, and merge and status both exit 0. Two tests: test_init_refused_by_e12_writes_nothing_and_retries, which failed on the unfixed cmd_init, and test_init_failing_after_genesis_takes_back_what_it_wrote, which forces the last write to fail with a directory where manifest.json goes and was watched failing with the ledger removal and the registry removal each disabled. Full suite: 785 passed, 0 failed. Not covered: a data dir already wedged by an older binary still needs deleting by hand, because init cannot tell a failed init's genesis from a real run's without a policy the owner hasn't set. The misleading 'or sahjhan init' hint is sj-kbzktgq.
