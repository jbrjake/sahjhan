---
id: sj-e3zkmf1
title: Give sahjhan the portfolio's Rust gate scaffold
category: build
seq: 40
docs:
  - portfolio#CLAUDE.md#every-rust-workspace-gets-the-gate-scaffold-at-creation--not-after-the-gate-gets-slow
verify: "all(contains Cargo.toml /\\[profile\\.dev\\]/, exists .cargo/config.toml, exists tests/sahjhan/main.rs, absent tests/gate_tests.rs)"
status: open
created: 2026-10-23T20:24Z
---
Found while working sj-1zq2xr5 (2026-10-23): sahjhan has none of the four
items `portfolio/CLAUDE.md` requires of every Rust workspace. No
`[profile.dev]` in `Cargo.toml`, so full DWARF for every dependency; no
`.cargo/config.toml`; 36 files directly under `tests/`, so 36 test binaries,
each a full link plus a Gatekeeper first-exec stall; and `target/` sits under
`~/Documents` at 81 GB. The release build alone took 1m53s.

The work: the `[profile.dev]` block and `.cargo/config.toml` `[build]`
rustflags from the portfolio rule; the 36 test files folded into
`tests/sahjhan/main.rs` with one `mod` per former file (`env!`
`CARGO_BIN_EXE_sahjhan` still resolves); `CARGO_TARGET_DIR` out of
`~/Documents`, or `mdutil -i off`. Every `target=<name>` in this store's
verifies then names `sahjhan`, so the store's open verifies change with it.
CLAUDE.md's Test Files table and its `cargo test --test` lines follow.

## log
- 2026-10-23T20:24Z created
