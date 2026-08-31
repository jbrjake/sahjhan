# hooks, guards, and monitors

Transitions enforce structure: the agent can't skip states or advance without gates passing. But a transition only fires when the agent asks to move. Between transitions it can edit files, run commands, write summaries, and claim completion, with sahjhan having no say.

Hooks give it a say. They're declared in `hooks.toml`, an optional config file, sealed at init like the rest of the eight, and evaluated on every tool call.

The agent can read `hooks.toml` and see exactly what will block it. That's fine. Knowing the rule doesn't help when a binary checks the ledger before the tool call runs.

## generating the bridge

```bash
sahjhan hook generate --harness cc --output-dir .claude/hooks
```

That writes four scripts: `pre_tool_hook.py` (PreToolUse), `post_tool_hook.py` (PostToolUse), `stop_hook.py` (Stop), and `_sahjhan_bootstrap.py`. The first three are thin wrappers that parse the harness event, call `sahjhan hook eval`, and forward the decision. All the logic lives in the binary. The bootstrap hook is self-contained, because it protects the system it would otherwise delegate to.

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

Exit code 1 on block, 0 on allow or warn. The decision is the strongest of the matching rules: block beats warn beats allow.

Evaluation order inside `hook eval`: derive the current state from the ledger, check managed paths, check write-gated paths, run the `hooks.toml` rules, then evaluate monitors.

## three kinds of hook

Every hook matches on `event` (`PreToolUse`, `PostToolUse`, `Stop`) and may narrow further by `tools`, `states`, `states_not`, and a path `filter`.

### gate hooks

Hooks fire when a gate **fails**. Any gate type can appear in a hook, with one catch: hooks don't get most state params. Only `agent_id`, `paths.*`, and `sets.<name>` are available. `agent_id` comes from `hook eval --agent-id`, which Claude Code sets on a hook fired inside a subagent and omits on the main thread.

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
since = "last_event_of_type:fix_commit"

[hooks.filter]
path_not_matches = "tests/*"
```

The filter means the hook only applies to source files. Edit `tests/test_thing.py` and it steps aside. Edit `src/main.py` with no `failing_test` event since the last `fix_commit`, and it blocks.

Globs support `*`, `**`, and `*.ext`, in both `path_matches` and `path_not_matches`.

### one gate, several agents

That hook is written for one agent at a time. Run three fix agents at once and
it stops meaning what it says: `filter` scopes the gate to the actor asking, but
the anchor is still global, so the first agent to land a fix ends everyone's
window. The others are blocked mid-fix holding evidence they recorded
correctly, and the escape the message prints is unreachable because their fix is
already written.

The solution is a `since_filter` that scopes the window from the resolution of the
finding the evidence is about, so an agent's authorization is consumed by its own
work and nobody else's:

```toml
[hooks.gate]
type = "ledger_has_event_since"
event = "tdd_evidence"
since = "last_event_of_type:finding_resolved"
filter = { agent_id = "{{agent_id}}" }
since_filter = { id = "{{event.finding_id}}" }
```

Two agents, one ledger. agent-a recorded evidence for `f1`, agent-b for `f2`,
and `f1` has since been resolved:

```console
$ sahjhan hook eval --event PreToolUse --tool Edit --file src/fix.rs --agent-id agent-a
{
  "decision": "block",
  "messages": [
    {
      "source": "hook",
      "rule_index": 0,
      "action": "block",
      "message": "record a failing test for this finding first"
    }
  ],
  "auto_records": [],
  "monitor_warnings": []
}

$ sahjhan hook eval --event PreToolUse --tool Edit --file src/fix.rs --agent-id agent-b
{
  "decision": "allow",
  "messages": [],
  "auto_records": [],
  "monitor_warnings": []
}
```

See [`since_filter` in gates.md](gates.md#notes-on-the-ones-with-sharp-edges)
for the two forms and why both field names have to be declared.

### check hooks

Checks cover conditions the gate system doesn't: pattern-matching on the agent's output, and event-count thresholds.

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

This catches premature victory laps. The agent writes "AUDIT COMPLETE" while still in `fix_loop`. The stop hook sees the completion language, checks the state, and blocks the output. 

### auto-record hooks

These observe without enforcing. They append an event whenever a matching tool is used, which gives the ledger ground truth about what actually changed:

```toml
[[hooks]]
event = "PostToolUse"
tools = ["Edit", "Write"]

[hooks.auto_record]
event_type = "source_edit"
fields = { file_path = "{tool.file_path}" }
```

Now the ledger knows which files were modified independent of what the agent says about it, and stall detectors and edit-accumulation checks can work off those events rather than self-reporting.

## write-gated paths

`paths.managed` blocks writes to a directory always. Some files should be writable, just not yet, like a summary that should only exist after convergence, or a punchlist that only updates in the finalized state.

```toml
# protocol.toml
[[guards.write_gated]]
path = "docs/SUMMARY.md"
writable_in = ["finalized"]
message = "SUMMARY.md can only be written after convergence."
```

Checked during `hook eval` for Edit and Write. In `finalized` the write goes through. Anywhere else it's blocked. `path` supports globs. This is conditional where `paths.managed` is absolute. One difference from hook rules: the `message` here is delivered verbatim, while the `{current_state}` and `{count}` placeholders only interpolate in hook and monitor messages.

## monitors

Monitors catch drift. They don't block, they warn in every `hook eval` response under `monitor_warnings` until something changes. The generated Claude Code wrappers only forward `messages`, so today a monitor warning reaches whoever reads the JSON, _not_ the agent.

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

Twenty events in `fix_loop` without advancing is a lot of activity with nothing to show for it. Monitors piggyback on `hook eval`, so they're checked on every tool use so you can ask questions like "Are you still making progress?"

## a note on timeouts

Hooks usually run under a short harness timeout. Two things to keep in mind:

- `sahjhan status` evaluates transition gates, and a `command_succeeds` gate can spawn a test suite. Use `sahjhan status --no-gates` in a hook.
- A gate hook whose gate runs a command pays that cost on every matching tool call. Prefer ledger and query gates in hooks. Save command gates for transitions.
