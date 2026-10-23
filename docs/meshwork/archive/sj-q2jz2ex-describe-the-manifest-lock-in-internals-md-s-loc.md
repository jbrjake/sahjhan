---
id: sj-q2jz2ex
title: "Describe the manifest lock in internals.md's locking section, once the owner takes the proposed text"
status: done
category: docs
discovered-from: sj-1zq2xr5
relates: [sj-1zq2xr5]
verify: contains docs/internals.md /[Mm]anifest has its own lock|manifest lock/
docs:
  - docs/internals.md#locking
seq: 25
created: 2026-10-23T20:28Z
---

`docs/internals.md` `## locking` reads, in full:

> Exclusive file locks for writes, shared for reads. The five-second timeout
> applies to the exclusive side; shared reads block without one.

That describes the ledger's lock. Since sj-1zq2xr5 (49277b9) there is a second
one, outside it, which a reader of this page needs to know about. Nothing in the
paragraph is false, but it is now half the answer. `docs/` is the owner's prose,
so this is a proposal for the owner to take, cut, or rewrite. Not to be written in
without the owner's go-ahead. Proposed addition after the existing paragraph:

> The manifest has its own lock, outside the ledger's: an `flock` on the data
> dir itself. A writer holds it from loading `manifest.json` to its last save,
> across the append, the seal, and every render the event triggers.
> `manifest verify` holds it shared across the load and every hash, so it never
> reads between a render and its seal. Both sides wait up to 30 seconds. Gate
> and emit commands run before it is taken, because one may itself run
> `sahjhan event`.

The verify checks only that the page mentions the lock. Whether the text is
right is the owner's review.

## log
- 2026-10-23T20:28Z created
- 2026-10-23T20:53Z open→doing — claimed by claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)
- 2026-10-23T20:55Z doing→done — verify exit 0 @ d5cc97d+5

## comments
- 2026-10-23T20:55Z [claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)] Owner, this session: 'you can edit the internals, i only said to hold the readme.' Written in d5cc97d, after his two sentences, which are unchanged. Checked against the release build: a writer facing a held lock exits 2 after 30s naming the data dir, and a verify waits for the holder rather than timing out early.
