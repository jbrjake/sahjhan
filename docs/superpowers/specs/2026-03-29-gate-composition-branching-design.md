# Gate Composition, Branching Transitions, and Mermaid Export — Design Spec

Three features that address fundamental expressiveness limits in the Sahjhan protocol grammar.

## Problem

### Gate composition
Every gate list on a transition is implicitly AND. There's no OR, NOT, or k-of-n. You can't express "any 2 of these 3 security scans must pass" or "either integration tests OR manual approval." Workaround is parallel transition paths, which explodes states.toml.

### Branching / conditional transitions
The state machine forces linear chains. No `on_failure` routing. If tests fail you get a blocked gate, not a redirect to a fix-and-retry state. Without branching, every real-world protocol either cheats with manual resets or is artificially linear.

### Protocol visualization
No way to see the protocol as a diagram. Reading five TOML files to understand the state graph is tedious.

## Design

### 1. Gate Boolean Composition

New composite gate types that contain nested gate arrays. The top-level gate list on a transition remains implicitly AND. Composition happens inside individual gates.

#### TOML syntax

```toml
gates = [
    # OR — either tests pass or manual approval exists
    { type = "any_of", intent = "tests or manual override required", gates = [
        { type = "command_succeeds", cmd = "pytest" },
        { type = "ledger_has_event", event = "manual_approval" },
    ]},

    # Explicit AND (useful nested inside any_of)
    { type = "all_of", gates = [
        { type = "file_exists", path = "tests/test.py" },
        { type = "command_succeeds", cmd = "pytest" },
    ]},

    # NOT — inverts child gate result
    { type = "not", intent = "no regressions recorded", gates = [
        { type = "ledger_has_event", event = "regression" },
    ]},

    # k-of-n — at least k of n child gates must pass
    { type = "k_of_n", k = 2, intent = "2 of 3 scans must pass", gates = [
        { type = "command_succeeds", cmd = "bandit -r src/" },
        { type = "command_succeeds", cmd = "safety check" },
        { type = "command_succeeds", cmd = "semgrep --config auto" },
    ]},
]
```

#### Struct change

Add optional `gates` field to `GateConfig` in `src/config/transitions.rs`:

```rust
pub struct GateConfig {
    #[serde(rename = "type")]
    pub gate_type: String,
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default)]
    pub gates: Vec<GateConfig>,   // recursive, for composite gates
    #[serde(flatten)]
    pub params: HashMap<String, toml::Value>,
}
```

Serde processes named fields before flatten, so `gates` is consumed by the vec field and won't appear in `params`. Non-composite gates have an empty vec.

#### Evaluation

New match arms in `gates/types.rs eval()`:

- **`any_of`**: Evaluate all children. Pass if any child passes. Description: "N of M alternatives passed." Reason lists which children failed.
- **`all_of`**: Evaluate all children. Pass if all children pass. Description: "N of M conditions passed." Reason lists which children failed.
- **`not`**: Evaluate single child (validate exactly 1 in vec). Pass if child fails. Description: "not(child_type): inverted." Reason explains the inversion.
- **`k_of_n`**: Extract `k` from `params` as integer. Evaluate all children. Pass if k or more pass. Description: "N of M passed (k required)."

All children are always evaluated (no short-circuit) so the GateResult can report the full picture of which sub-gates passed/failed.

#### Validation

`validate_deep` additions:

- `any_of`, `all_of`: require `gates` non-empty, recursively validate children
- `not`: require exactly 1 child gate in `gates`
- `k_of_n`: require `k` param (integer, 1 <= k <= len(gates)), require `gates` non-empty
- All composite types: recursively validate nested gates for known types and required params

### 2. Branching / Conditional Transitions

Allow multiple transitions with the same `from` state and `command` name. The first transition (in TOML declaration order) whose gates all pass is taken.

#### TOML syntax

```toml
# Happy path: tests pass → advance to verifying
[[transitions]]
from = "implementing"
to = "verifying"
command = "submit"
gates = [
    { type = "command_succeeds", cmd = "python -m pytest tests/", timeout = 120 },
    { type = "no_violations" },
]

# Fallback: gates above failed → go fix
[[transitions]]
from = "implementing"
to = "fix-and-retry"
command = "submit"
gates = []
```

A gateless transition at the end acts as a catch-all fallback. TOML `[[transitions]]` preserves insertion order, which determines priority.

#### State machine change

`StateMachine::transition()` in `src/state/machine.rs` changes from `find()` to `filter()`:

1. Collect all transitions matching `command` + `from` state (preserving TOML order).
2. If none found: `StateError::NoTransition` (unchanged).
3. For each candidate in order:
   a. Build `state_params` for that candidate's target state.
   b. Merge CLI args (positional + key=value).
   c. Evaluate all gates via `evaluate_gates()` (returns `Vec<GateResult>`).
   d. If all pass: reload ledger, append `state_transition` event, update state, return `Ok`.
   e. If any fail: stash the failures, try next candidate.
4. If no candidate passed: return error with accumulated failure info.

#### Error type

New `StateError` variant for multi-candidate failures:

```rust
#[error("all transition candidates for '{command}' from '{state}' were blocked")]
AllCandidatesBlocked {
    command: String,
    state: String,
    /// (target_state, failed_gate_type, reason) per candidate
    candidates: Vec<(String, String, String)>,
}
```

The CLI handles this new variant alongside the existing `GateBlocked`, printing each candidate's failure.

#### Side effects

Gate evaluation can run shell commands. When multiple candidates exist, gates for earlier candidates may execute and fail before later candidates are tried. This is intentional — you must evaluate to determine which path to take. Protocol authors should be aware that gate commands for all attempted candidates may execute.

#### Gate check (dry-run)

`cmd_gate_check` in `src/cli/transition.rs` shows all matching transitions and their gate status:

```
gate-check: submit
  candidate 1: implementing → verifying
    ✗ command_succeeds: 'pytest' exit 1
    ✓ no_violations
  candidate 2: implementing → fix-and-retry
    (no gates)
  result: would take candidate 2 → fix-and-retry
```

#### Validation

- Multiple transitions with same `from`+`command` is now explicitly allowed.
- Warning if multiple candidates exist and none is gateless (no fallback — agent could get stuck if all gates fail).

### 3. Mermaid Export

New CLI command for protocol visualization.

#### CLI

```bash
# Raw mermaid stateDiagram-v2 text to stdout
sahjhan mermaid

# ASCII art state diagram to stdout
sahjhan mermaid --rendered
```

Accepts `--config-dir` like every other command. Reads only config, no ledger needed.

#### Raw output

Mermaid `stateDiagram-v2` format:

```
stateDiagram-v2
    [*] --> idle
    idle --> writing_tests : start
    writing_tests --> implementing : tests-done
    implementing --> verifying : submit [gates]
    implementing --> fix_and_retry : submit [fallback]
    fix_and_retry --> implementing : retry
    verifying --> [*]
```

Mermaid state IDs can't contain hyphens, so state names are sanitized (hyphens to underscores). Original names used in display labels via `state fix_and_retry : fix-and-retry`.

Gate annotations on edges are kept short — command name plus abbreviated gate summary. Multiple candidates from same state+command shown as separate edges.

#### Rendered output (ASCII art)

Tree-walk from initial state using BFS:

```
[idle] (initial)
 └─ start ──▶ [writing-tests]
               └─ tests-done ──▶ [implementing]
                  │ file_exists, any_of(2), set_covered
                  ├─ submit ──▶ [verifying] (terminal)
                  │  │ command_succeeds, k_of_n(2/3), no_violations
                  └─ submit ──▶ [fix-and-retry] (fallback)
                                 └─ retry ──▶ [implementing] (↑ cycle)
```

Rendering rules:
- BFS from initial state
- Each state as `[name]`, annotated with `(initial)` / `(terminal)`
- Outgoing transitions indented with `└─` / `├─` tree connectors
- Gate summaries on sub-line: leaf gates by name, composites abbreviated (`any_of(2)`, `2-of-3`, `not(event)`)
- Back-edges (cycles) marked `(↑ cycle)` instead of recursing
- Multiple candidates from same command shown as sibling branches, later ones marked `(fallback)`

#### Implementation location

- `src/mermaid.rs` — generation logic, takes `&ProtocolConfig`, returns `String` for both modes
- `src/cli/mermaid.rs` — CLI command module
- Registered in `main.rs` as new clap subcommand

### 4. README Example Update

The existing TDD example protocol in the README gets extended to demonstrate both new features with realistic use cases.

#### New state

```toml
[states.fix-and-retry]
label = "Fix and retry"
```

#### Updated transitions

```toml
# idle → writing-tests (unchanged)
[[transitions]]
from = "idle"
to = "writing-tests"
command = "start"
gates = []

# writing-tests → implementing (gains any_of for test override)
[[transitions]]
from = "writing-tests"
to = "implementing"
command = "tests-done"
gates = [
    { type = "file_exists", path = "tests/test_feature.py",
      intent = "test file must exist on disk before implementation begins" },
    { type = "any_of", intent = "tests must run or be explicitly overridden", gates = [
        { type = "command_succeeds", cmd = "python -m pytest tests/", timeout = 60 },
        { type = "ledger_has_event", event = "manual_test_override" },
    ]},
    { type = "set_covered", set = "suites",
      event = "set_member_complete", field = "member",
      intent = "every test suite must be written before implementing" },
]

# implementing → verifying (happy path with k-of-n static analysis)
[[transitions]]
from = "implementing"
to = "verifying"
command = "submit"
gates = [
    { type = "command_succeeds", cmd = "python -m pytest tests/", timeout = 120,
      intent = "all tests must pass before verification" },
    { type = "k_of_n", k = 2, intent = "at least 2 of 3 code quality checks must pass", gates = [
        { type = "command_succeeds", cmd = "python -m mypy src/" },
        { type = "command_succeeds", cmd = "python -m pylint src/" },
        { type = "command_succeeds", cmd = "python -m bandit -r src/" },
    ]},
    { type = "no_violations", intent = "clean record — no tampering" },
]

# implementing → fix-and-retry (fallback when submit gates fail)
[[transitions]]
from = "implementing"
to = "fix-and-retry"
command = "submit"
gates = []

# fix-and-retry → implementing (recovery loop)
[[transitions]]
from = "fix-and-retry"
to = "implementing"
command = "retry"
gates = [
    { type = "not", intent = "previous test failures must not persist", gates = [
        { type = "command_output", cmd = "python -m pytest tests/ --tb=no -q 2>&1 | tail -1",
          expect = "no tests ran" },
    ]},
]
```

#### What this demonstrates

- **`any_of`**: Tests can run OR a manual override event exists. Real-world: CI is flaky, human can unblock.
- **`k_of_n`**: 2 of 3 static analysis tools must pass. Real-world: not every linter agrees, but consensus should hold.
- **Branching**: `submit` from `implementing` tries happy path first, falls to `fix-and-retry` if gates fail.
- **`not`**: Recovery transition checks that "no tests ran" does NOT appear (tests exist and executed).
- **Error recovery loop**: `fix-and-retry → implementing` is a cycle, not a dead end.

#### New README sections

- "Gate composition" section after the gate types table, showing `any_of`, `all_of`, `not`, `k_of_n` with examples from the TDD protocol.
- "Conditional transitions" section showing branching with the submit/retry pattern.
- `sahjhan mermaid` added to CLI reference.
- Existing walkthrough extended to show a failed `submit` routing to `fix-and-retry`, then `retry` back.

## Files Changed

### New files
- `src/mermaid.rs` — Mermaid/ASCII generation logic
- `src/cli/mermaid.rs` — CLI command

### Modified files
- `src/config/transitions.rs` — `GateConfig` gains `gates: Vec<GateConfig>` field
- `src/gates/types.rs` — new `any_of`, `all_of`, `not`, `k_of_n` match arms in `eval()`
- `src/gates/evaluator.rs` — `default_intent` gains entries for composite gate types
- `src/state/machine.rs` — `transition()` becomes multi-candidate; new `AllCandidatesBlocked` error
- `src/config/mod.rs` — `validate_deep` gains composite gate validation + branching warnings
- `src/cli/transition.rs` — `cmd_transition` handles `AllCandidatesBlocked`; `cmd_gate_check` shows all candidates
- `src/main.rs` — new `mermaid` subcommand
- `src/lib.rs` — export `mermaid` module
- `README.md` — extended example, new sections, CLI reference update
- `CLAUDE.md` — updated lookup tables and flow maps

### Test files
- `tests/gate_tests.rs` — tests for `any_of`, `all_of`, `not`, `k_of_n` evaluation
- `tests/state_machine_tests.rs` — tests for multi-candidate transitions, fallback routing, cycle handling
- `tests/config_tests.rs` — validation tests for composite gate constraints
- `tests/integration_tests.rs` — end-to-end CLI tests for branching + gate composition
- `tests/mermaid_tests.rs` (new) — tests for Mermaid and ASCII output generation
