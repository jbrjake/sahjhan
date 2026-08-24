# hooks, guards, and monitors

Transitions enforce structure: the agent can't skip states or advance without
gates passing. But a transition only fires when the agent asks to move. Between
transitions it can edit files, run commands, write summaries, and claim
completion, with sahjhan having no say. The protocol says "you must be in state X
to do Y," and nothing was checking that on every tool use.

Hooks close that gap. They're declared in `hooks.toml`, an optional config
file, sealed at init like the rest of the eight, and evaluated on every tool
call.

The agent can read `hooks.toml` and see exactly what will block it. That's fine.
Knowing the rule doesn't help when a binary checks the ledger before the tool
call runs.

## generating the bridge

```bash
sahjhan hook generate --harness cc --output-dir .claude/hooks
```

That writes four scripts: `pre_tool_hook.py` (PreToolUse), `post_tool_hook.py`
(PostToolUse), `stop_hook.py` (Stop), and `_sahjhan_bootstrap.py`. The first three
are thin wrappers that parse the harness event, call `sahjhan hook eval`, and
forward the decision — all the logic lives in the binary. The bootstrap hook is
self-contained, because it protects the system it would otherwise delegate to.

## evaluating by hand

```bash
$ sahjhan hook eval --event PreToolUse --tool Edit --file src/main.rs
{
  "decision": "block",
  "messages": [
    {
      "source": "hook",
      "rule_index": 0,
      "action": "block",
      "message": "Cannot edit source files in working state without a check_done event. Record one first."
    }
  ],
  "auto_records": [],
  "monitor_warnings": []
}

$ sahjhan event check_done
recorded: check_done

$ sahjhan hook eval --event PreToolUse --tool Edit --file src/main.rs
{ "decision": "allow", "messages": [], "auto_records": [], "monitor_warnings": [] }
```

Exit code 1 on block, 0 on allow or warn. The decision is the strongest of the
matching rules: block beats warn beats allow.

Evaluation order inside `hook eval`: derive the current state from the ledger,
check managed paths, check write-gated paths, run the `hooks.toml` rules, then
evaluate monitors.

## three kinds of hook

Every hook matches on `event` (`PreToolUse`, `PostToolUse`, `Stop`) and may
narrow further by `tools`, `states`, `states_not`, and a path `filter`.

### gate hooks

Any gate type can appear in a hook, with one catch: hooks evaluate with no
state params, so a gate templated on a state param never resolves — which makes
it fire unconditionally. Only the `paths.*` and `sets.<name>` variables are
available. The hook fires when the gate **fails**:

```toml
[[hooks]]
event = "PreToolUse"
tools = ["Edit"]
states = ["fix_loop"]
action = "block"
message = "Write a failing test before editing source files."

[hooks.gate]
type = "ledger_has_event_since"
event = "failing_test"
since = "fix_commit"

[hooks.filter]
path_not_matches = "tests/*"
```

The filter means the hook only applies to source files. Edit
`tests/test_thing.py` and it steps aside. Edit `src/main.py` with no
`failing_test` event since the last `fix_commit`, and it blocks. The agent
doesn't get to decide whether its test was good enough; it doesn't get to decide
whether it wrote one.

Globs support `*`, `**`, and `*.ext`, in both `path_matches` and
`path_not_matches`.

### check hooks

Checks cover conditions the gate system doesn't: pattern-matching on the agent's
output, and event-count thresholds.

```toml
[[hooks]]
event = "Stop"
states_not = ["converged", "finalized"]
action = "block"
message = "You're claiming completion but state is {current_state}."

[hooks.check]
type = "output_contains_any"
patterns = ["audit complete", "all fixes applied", "CONVERGED"]
```

This catches premature victory laps. The agent writes "AUDIT COMPLETE" while
still in `fix_loop`; the stop hook sees the completion language, checks the
state, and blocks the output. Write all the summaries you want — until the
protocol says you're done, you're not done.

### auto-record hooks

These observe without enforcing. They append an event whenever a matching tool is
used, which gives the ledger ground truth about what actually changed:

```toml
[[hooks]]
event = "PostToolUse"
tools = ["Edit", "Write"]

[hooks.auto_record]
event_type = "source_edit"
fields = { file_path = "{tool.file_path}" }
```

Now the ledger knows which files were modified independent of what the agent
says about it, and stall detectors and edit-accumulation checks can work off
those events rather than self-reporting.

## write-gated paths

`paths.managed` blocks writes to a directory always. Some files should be
writable, just not yet: a summary that should only exist after convergence, a
punchlist that only updates in the finalized state.

```toml
# protocol.toml
[[guards.write_gated]]
path = "docs/SUMMARY.md"
writable_in = ["finalized"]
message = "SUMMARY.md can only be written after convergence."
```

Checked during `hook eval` for Edit and Write. In `finalized` the write goes
through; anywhere else it's blocked. `path` supports globs. This is
conditional where `paths.managed` is absolute. One difference from hook rules:
the `message` here is delivered verbatim — the `{current_state}` and `{count}`
placeholders only interpolate in hook and monitor messages.

## monitors

Monitors catch drift. They don't block, they warn, and the warning surfaces in
every `hook eval` response until something changes — in the `monitor_warnings`
array. Note the generated Claude Code wrappers forward only `messages`, so
today a monitor warning reaches whoever reads the JSON, not the agent behind
the bridge.

```toml
[[monitors]]
name = "fix_loop_stall"
states = ["fix_loop"]
action = "warn"
message = "{count} events since last state transition. Commit your fixes."

[monitors.trigger]
type = "event_count_since_last_transition"
threshold = 20
```

Twenty events in `fix_loop` without advancing is a lot of activity with nothing
to show for it. Monitors piggyback on `hook eval`, so they're checked on every
tool use — no timer, no polling, just a question asked on every action: are you
still making progress?

## a note on timeouts

Hooks usually run under a short harness timeout. Two things to keep in mind:

- `sahjhan status` evaluates transition gates, and a `command_succeeds` gate can
  spawn a test suite. Use `sahjhan status --no-gates` in a hook.
- A gate hook whose gate runs a command pays that cost on every matching tool
  call. Prefer ledger and query gates in hooks; save command gates for
  transitions.
