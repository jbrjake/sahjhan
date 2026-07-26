// tests/lint_tests.rs
//
// Static integrity analysis (`sahjhan lint`, issue #32).
//
// Configs are written to a tempdir and loaded through ProtocolConfig::load so
// the new TOML surfaces are exercised end to end, not just the check logic.

use assert_cmd::Command;
use sahjhan::config::ProtocolConfig;
use sahjhan::lint::{self, LintOptions, Severity};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

const PROTOCOL_HEAD: &str = r#"
[protocol]
name = "lint-test"
version = "1.0.0"
description = "lint fixture"

[paths]
managed = []
data_dir = ".data"
render_dir = "."
"#;

/// A protocol config on disk, built piece by piece.
struct Fixture {
    dir: TempDir,
    protocol: String,
    states: String,
    transitions: String,
    events: String,
    hooks: String,
}

impl Fixture {
    fn new() -> Self {
        Fixture {
            dir: TempDir::new().unwrap(),
            protocol: PROTOCOL_HEAD.to_string(),
            states: String::new(),
            transitions: String::new(),
            events: String::new(),
            hooks: String::new(),
        }
    }

    fn protocol(mut self, extra: &str) -> Self {
        self.protocol.push_str(extra);
        self
    }

    fn states(mut self, toml: &str) -> Self {
        self.states.push_str(toml);
        self
    }

    fn transitions(mut self, toml: &str) -> Self {
        self.transitions.push_str(toml);
        self
    }

    fn events(mut self, toml: &str) -> Self {
        self.events.push_str(toml);
        self
    }

    fn hooks(mut self, toml: &str) -> Self {
        self.hooks.push_str(toml);
        self
    }

    fn write(&self) -> &std::path::Path {
        let p = self.dir.path();
        std::fs::write(p.join("protocol.toml"), &self.protocol).unwrap();
        std::fs::write(p.join("states.toml"), &self.states).unwrap();
        std::fs::write(p.join("transitions.toml"), &self.transitions).unwrap();
        if !self.events.is_empty() {
            std::fs::write(p.join("events.toml"), &self.events).unwrap();
        }
        if !self.hooks.is_empty() {
            std::fs::write(p.join("hooks.toml"), &self.hooks).unwrap();
        }
        p
    }

    fn load(&self) -> ProtocolConfig {
        ProtocolConfig::load(self.write()).unwrap()
    }

    fn lint(&self) -> Vec<lint::LintFinding> {
        lint::run(&self.load(), &LintOptions::default())
    }
}

/// A two-state protocol: idle -(go)-> done, with `gate` guarding the edge.
fn simple(gate: &str, events: &str) -> Fixture {
    Fixture::new()
        .states(
            r#"
[states.idle]
label = "Idle"
initial = true

[states.done]
label = "Done"
terminal = true
"#,
        )
        .transitions(&format!(
            r#"
[[transitions]]
from = "idle"
to = "done"
command = "go"
gates = [{}]
"#,
            gate
        ))
        .events(events)
}

fn findings_for<'a>(findings: &'a [lint::LintFinding], check: &str) -> Vec<&'a lint::LintFinding> {
    findings.iter().filter(|f| f.check == check).collect()
}

// ---------------------------------------------------------------------------
// L1 — unsatisfiable gates
// ---------------------------------------------------------------------------

#[test]
fn test_l1_restricted_event_without_producer_is_error() {
    let f = simple(
        r#"{ type = "ledger_has_event", event = "context_reset" }"#,
        r#"
[events.context_reset]
description = "context was reset"
restricted = true
fields = []
"#,
    );
    let findings = f.lint();
    let l1 = findings_for(&findings, "L1");
    assert_eq!(l1.len(), 1, "expected one L1 finding: {:?}", findings);
    assert_eq!(l1[0].severity, Severity::Error);
    assert!(
        l1[0].message.contains("context_reset"),
        "message should name the event: {}",
        l1[0].message
    );
}

#[test]
fn test_l1_restricted_event_with_declared_producer_is_clean() {
    let f = simple(
        r#"{ type = "ledger_has_event", event = "context_reset" }"#,
        r#"
[events.context_reset]
description = "context was reset"
restricted = true
fields = []

[[events.context_reset.producers]]
id = "hook:session-start"
"#,
    );
    let findings = f.lint();
    assert!(
        findings_for(&findings, "L1").is_empty(),
        "declared producer should satisfy L1: {:?}",
        findings
    );
}

#[test]
fn test_l1_transition_emit_counts_as_producer() {
    let f = Fixture::new()
        .states(
            r#"
[states.idle]
label = "Idle"
initial = true

[states.mid]
label = "Mid"

[states.done]
label = "Done"
terminal = true
"#,
        )
        .transitions(
            r#"
[[transitions]]
from = "idle"
to = "mid"
command = "start"
gates = []

[[transitions.emits]]
event = "work_logged"

[[transitions]]
from = "mid"
to = "done"
command = "finish"
gates = [{ type = "ledger_has_event", event = "work_logged" }]
"#,
        )
        .events(
            r#"
[events.work_logged]
description = "work happened"
restricted = true
fields = []
"#,
        )
        .protocol("\n[lint]\nrequire_producers = true\n");
    let findings = f.lint();
    assert!(
        findings_for(&findings, "L1").is_empty(),
        "a transition emit is a producer the engine can see: {:?}",
        findings
    );
}

#[test]
fn test_l1_hook_auto_record_counts_as_producer() {
    let f = simple(
        r#"{ type = "ledger_has_event", event = "file_edited" }"#,
        r#"
[events.file_edited]
description = "a file was edited"
restricted = true
fields = []
"#,
    )
    .hooks(
        r#"
[[hooks]]
event = "PostToolUse"
tools = ["Edit"]

[hooks.auto_record]
event_type = "file_edited"
"#,
    );
    let findings = f.lint();
    assert!(
        findings_for(&findings, "L1").is_empty(),
        "hook auto_record is a producer: {:?}",
        findings
    );
}

#[test]
fn test_l1_unrestricted_event_without_producer_is_silent_by_default() {
    let f = simple(
        r#"{ type = "ledger_has_event", event = "note_taken" }"#,
        r#"
[events.note_taken]
description = "agent recorded a note"
fields = []
"#,
    );
    let findings = f.lint();
    assert!(
        findings_for(&findings, "L1").is_empty(),
        "`sahjhan event` can record an unrestricted event, so this is not a defect: {:?}",
        findings
    );
}

#[test]
fn test_l1_require_producers_flags_unrestricted_event() {
    let f = simple(
        r#"{ type = "ledger_has_event", event = "note_taken" }"#,
        r#"
[events.note_taken]
description = "agent recorded a note"
fields = []
"#,
    )
    .protocol("\n[lint]\nrequire_producers = true\n");
    let findings = f.lint();
    let l1 = findings_for(&findings, "L1");
    assert_eq!(l1.len(), 1, "expected one L1 finding: {:?}", findings);
    assert_eq!(l1[0].severity, Severity::Error);
}

#[test]
fn test_l1_undeclared_event_is_warning() {
    let f = simple(
        r#"{ type = "ledger_has_event", event = "typoed_event" }"#,
        r#"
[events.real_event]
description = "the one that exists"
fields = []
"#,
    );
    let findings = f.lint();
    let l1 = findings_for(&findings, "L1");
    assert_eq!(l1.len(), 1, "expected one L1 finding: {:?}", findings);
    assert_eq!(l1[0].severity, Severity::Warning);
    assert!(l1[0].message.contains("not declared in events.toml"));
}

#[test]
fn test_l1_negated_reference_needs_no_producer() {
    let f = simple(
        r#"{ type = "not", gates = [{ type = "ledger_has_event", event = "context_reset" }] }"#,
        r#"
[events.context_reset]
description = "context was reset"
restricted = true
fields = []
"#,
    );
    let findings = f.lint();
    assert!(
        findings_for(&findings, "L1").is_empty(),
        "a gate that requires an event's ABSENCE needs no producer: {:?}",
        findings
    );
}

#[test]
fn test_l1_ledger_lacks_event_needs_no_producer() {
    let f = simple(
        r#"{ type = "ledger_lacks_event", event = "context_reset" }"#,
        r#"
[events.context_reset]
description = "context was reset"
restricted = true
fields = []
"#,
    );
    assert!(findings_for(&f.lint(), "L1").is_empty());
}

#[test]
fn test_l1_disjunctive_branch_is_warning_not_error() {
    let f = simple(
        r#"{ type = "any_of", gates = [
            { type = "ledger_has_event", event = "context_reset" },
            { type = "file_exists", path = "/tmp" },
        ] }"#,
        r#"
[events.context_reset]
description = "context was reset"
restricted = true
fields = []
"#,
    );
    let findings = f.lint();
    let l1 = findings_for(&findings, "L1");
    assert_eq!(l1.len(), 1, "expected one L1 finding: {:?}", findings);
    assert_eq!(
        l1[0].severity,
        Severity::Warning,
        "an any_of branch that can never pass leaves the gate satisfiable"
    );
}

#[test]
fn test_l1_budget_gate_requires_nothing() {
    // min_count = 0 with a max_count ceiling passes on an empty ledger.
    let f = simple(
        r#"{ type = "ledger_has_event", event = "retry", min_count = 0, max_count = 3 }"#,
        r#"
[events.retry]
description = "a retry"
restricted = true
fields = []
"#,
    );
    assert!(findings_for(&f.lint(), "L1").is_empty());
}

#[test]
fn test_l1_hook_gate_reports_always_fires() {
    let f = simple(r#""#, r#""#).hooks(
        r#"
[[hooks]]
event = "PreToolUse"
tools = ["Edit"]
action = "block"
message = "no"

[hooks.gate]
type = "ledger_has_event"
event = "context_reset"
"#,
    );
    // Declare the event as restricted with no producer.
    let f = f.events(
        r#"
[events.context_reset]
description = "context was reset"
restricted = true
fields = []
"#,
    );
    let findings = f.lint();
    let l1 = findings_for(&findings, "L1");
    assert_eq!(l1.len(), 1, "expected one L1 finding: {:?}", findings);
    assert!(
        l1[0].location.starts_with("hooks.toml"),
        "finding should point at the hook: {}",
        l1[0].location
    );
    assert!(
        l1[0].message.contains("every matching tool use"),
        "message should explain the hook always fires: {}",
        l1[0].message
    );
}

// ---------------------------------------------------------------------------
// L4 — dead-end states
// ---------------------------------------------------------------------------

#[test]
fn test_l4_non_terminal_state_without_exit_is_error() {
    let f = Fixture::new()
        .states(
            r#"
[states.idle]
label = "Idle"
initial = true

[states.stuck]
label = "Stuck"
"#,
        )
        .transitions(
            r#"
[[transitions]]
from = "idle"
to = "stuck"
command = "go"
gates = []
"#,
        );
    let findings = f.lint();
    let l4 = findings_for(&findings, "L4");
    assert_eq!(l4.len(), 1, "expected one L4 finding: {:?}", findings);
    assert_eq!(l4[0].severity, Severity::Error);
    assert!(l4[0].message.contains("'stuck'"));
}

#[test]
fn test_l4_terminal_state_without_exit_is_fine() {
    let f = simple("", "");
    assert!(
        findings_for(&f.lint(), "L4").is_empty(),
        "a terminal state is supposed to have no exit"
    );
}

#[test]
fn test_l4_all_exits_unsatisfiable_is_error() {
    let f = Fixture::new()
        .states(
            r#"
[states.idle]
label = "Idle"
initial = true

[states.trap]
label = "Trap"

[states.done]
label = "Done"
terminal = true
"#,
        )
        .transitions(
            r#"
[[transitions]]
from = "idle"
to = "trap"
command = "go"
gates = []

[[transitions]]
from = "trap"
to = "done"
command = "escape"
gates = [{ type = "ledger_has_event", event = "impossible" }]
"#,
        )
        .events(
            r#"
[events.impossible]
description = "nothing can record this"
restricted = true
fields = []
"#,
        );
    let findings = f.lint();
    let l4 = findings_for(&findings, "L4");
    assert_eq!(l4.len(), 1, "expected one L4 finding: {:?}", findings);
    assert!(
        l4[0].message.contains("every exit") && l4[0].message.contains("'trap'"),
        "message should name the trapped state: {}",
        l4[0].message
    );
}

// ---------------------------------------------------------------------------
// L5 — dead vocabulary
// ---------------------------------------------------------------------------

#[test]
fn test_l5_unused_event_is_warning() {
    let f = simple(
        "",
        r#"
[events.orphan]
description = "nothing reads or writes this"
fields = []
"#,
    );
    let findings = f.lint();
    let l5 = findings_for(&findings, "L5");
    assert_eq!(l5.len(), 1, "expected one L5 finding: {:?}", findings);
    assert_eq!(l5[0].severity, Severity::Warning);
    assert!(l5[0].message.contains("orphan"));
}

#[test]
fn test_l5_event_consumed_by_gate_is_alive() {
    let f = simple(
        r#"{ type = "ledger_lacks_event", event = "orphan" }"#,
        r#"
[events.orphan]
description = "read by a gate"
fields = []
"#,
    );
    assert!(findings_for(&f.lint(), "L5").is_empty());
}

#[test]
fn test_l5_event_named_in_named_query_is_alive() {
    let f = simple(
        "",
        r#"
[events.counted]
description = "counted by a named query"
fields = []
"#,
    )
    .protocol(
        r#"
[queries.counted_enough]
sql = "SELECT count(*) >= 1 as result FROM events WHERE event_type = 'counted'"
"#,
    );
    assert!(
        findings_for(&f.lint(), "L5").is_empty(),
        "a predicate that names the event counts as consumption"
    );
}

#[test]
fn test_l5_engine_events_are_exempt() {
    let f = simple(
        "",
        r#"
[events.state_transition]
description = "engine-written"
fields = []

[events.set_member_complete]
description = "engine-written"
fields = []
"#,
    );
    assert!(
        findings_for(&f.lint(), "L5").is_empty(),
        "engine events are always live"
    );
}

// ---------------------------------------------------------------------------
// Selection and CLI
// ---------------------------------------------------------------------------

#[test]
fn test_only_selection_filters_findings() {
    let f = simple(
        "",
        r#"
[events.orphan]
description = "nothing reads this"
fields = []
"#,
    );
    let config = f.load();
    let findings = lint::run(
        &config,
        &LintOptions {
            only: vec!["L4".to_string()],
        },
    );
    assert!(
        findings.iter().all(|f| f.check == "L4"),
        "only L4 should be reported: {:?}",
        findings
    );
}

#[test]
fn test_disabled_checks_are_skipped() {
    let f = simple(
        "",
        r#"
[events.orphan]
description = "nothing reads this"
fields = []
"#,
    )
    .protocol("\n[lint]\ndisabled_checks = [\"L5\"]\n");
    let findings = f.lint();
    assert!(
        findings_for(&findings, "L5").is_empty(),
        "L5 was disabled in [lint]: {:?}",
        findings
    );
}

#[test]
fn test_cli_lint_clean_protocol_exits_zero() {
    let f = simple("", "");
    let dir = f.write();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", dir.to_str().unwrap(), "lint"])
        .assert()
        .success()
        .stdout(predicates::str::contains("clean."));
}

#[test]
fn test_cli_lint_error_exits_config_error() {
    let f = Fixture::new()
        .states(
            r#"
[states.idle]
label = "Idle"
initial = true

[states.stuck]
label = "Stuck"
"#,
        )
        .transitions(
            r#"
[[transitions]]
from = "idle"
to = "stuck"
command = "go"
gates = []
"#,
        );
    let dir = f.write();
    let assert = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", dir.to_str().unwrap(), "lint"])
        .assert()
        .code(3);
    // Findings print even though the command failed.
    assert.stderr(predicates::str::contains("L4 error"));
}

#[test]
fn test_cli_lint_strict_promotes_warnings() {
    let f = simple(
        "",
        r#"
[events.orphan]
description = "nothing reads this"
fields = []
"#,
    );
    let dir = f.write();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", dir.to_str().unwrap(), "lint"])
        .assert()
        .success();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", dir.to_str().unwrap(), "lint", "--strict"])
        .assert()
        .code(3);
}

#[test]
fn test_cli_lint_json_envelope() {
    let f = simple(
        "",
        r#"
[events.orphan]
description = "nothing reads this"
fields = []
"#,
    );
    let dir = f.write();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", dir.to_str().unwrap(), "--json", "lint"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["command"], "lint");
    assert_eq!(json["data"]["warning_count"], 1);
    assert_eq!(json["data"]["error_count"], 0);
    assert_eq!(json["data"]["findings"][0]["check"], "L5");
    assert_eq!(json["data"]["findings"][0]["severity"], "warning");
}

#[test]
fn test_cli_lint_unknown_check_id_is_usage_error() {
    let f = simple("", "");
    let dir = f.write();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            dir.to_str().unwrap(),
            "lint",
            "--only",
            "L99",
        ])
        .assert()
        .code(4)
        .stderr(predicates::str::contains("unknown check id"));
}

// ---------------------------------------------------------------------------
// L3 — boundary route-arounds
// ---------------------------------------------------------------------------

/// A protocol shaped like the one L3 exists for: work → merge_done, then back
/// into fix_loop, with a pause state offering a second `resume` command.
fn boundary_fixture(second_resume_target: Option<&str>) -> Fixture {
    let mut transitions = r#"
[[transitions]]
from = "fix_loop"
to = "merge_done"
command = "merge"
gates = []

[[transitions]]
from = "merge_done"
to = "awaiting_clear"
command = "clear"
gates = []

[[transitions]]
from = "awaiting_clear"
to = "fix_loop"
command = "resume"
boundary = "context-reset"
gates = []

[[transitions]]
from = "merge_done"
to = "paused"
command = "pause"
gates = []
"#
    .to_string();

    if let Some(target) = second_resume_target {
        transitions.push_str(&format!(
            r#"
[[transitions]]
from = "paused"
to = "{}"
command = "resume"
gates = []
"#,
            target
        ));
    }

    Fixture::new()
        .protocol(
            r#"
[[boundaries]]
name = "context-reset"
must_traverse = { from = "merge_done", to = "fix_loop" }
"#,
        )
        .states(
            r#"
[states.fix_loop]
label = "Fix loop"
initial = true

[states.merge_done]
label = "Merge done"

[states.awaiting_clear]
label = "Awaiting clear"

[states.paused]
label = "Paused"
"#,
        )
        .transitions(&transitions)
}

#[test]
fn test_l3_boundary_with_only_tagged_route_is_clean() {
    let f = boundary_fixture(None);
    let findings = f.lint();
    assert!(
        findings_for(&findings, "L3").is_empty(),
        "the only route from merge_done to fix_loop crosses the tagged edge: {:?}",
        findings
    );
}

#[test]
fn test_l3_untagged_second_route_is_a_bypass() {
    // The pause state's `resume` lands straight back in fix_loop, skipping the
    // context reset — the exact shape a hand-written test cannot generalize.
    let f = boundary_fixture(Some("fix_loop"));
    let findings = f.lint();
    let l3 = findings_for(&findings, "L3");
    assert_eq!(l3.len(), 1, "expected one L3 finding: {:?}", findings);
    assert_eq!(l3[0].severity, Severity::Error);
    assert!(
        l3[0].message.contains("routed around"),
        "message should say it can be routed around: {}",
        l3[0].message
    );
    assert!(
        l3[0]
            .message
            .contains("merge_done -(pause)-> paused -(resume)-> fix_loop"),
        "message should print the bypass path: {}",
        l3[0].message
    );
}

#[test]
fn test_l3_second_route_through_the_boundary_is_clean() {
    // Routing the pause state back through awaiting_clear keeps the boundary.
    let f = boundary_fixture(Some("awaiting_clear"));
    assert!(
        findings_for(&f.lint(), "L3").is_empty(),
        "a path that rejoins before the tagged edge still crosses it"
    );
}

#[test]
fn test_l3_untagged_boundary_is_error() {
    let f = Fixture::new()
        .protocol(
            r#"
[[boundaries]]
name = "context-reset"
must_traverse = { from = "a", to = "b" }
"#,
        )
        .states(
            r#"
[states.a]
label = "A"
initial = true

[states.b]
label = "B"
terminal = true
"#,
        )
        .transitions(
            r#"
[[transitions]]
from = "a"
to = "b"
command = "go"
gates = []
"#,
        );
    let findings = f.lint();
    let l3 = findings_for(&findings, "L3");
    assert!(
        l3.iter()
            .any(|f| f.message.contains("no transition carries")),
        "an untagged boundary enforces nothing: {:?}",
        findings
    );
}

#[test]
fn test_l3_unknown_state_in_boundary_is_error() {
    let f = Fixture::new()
        .protocol(
            r#"
[[boundaries]]
name = "b"
must_traverse = { from = "a", to = "nowhere" }
"#,
        )
        .states(
            r#"
[states.a]
label = "A"
initial = true
terminal = true
"#,
        )
        .transitions("transitions = []\n");
    let findings = f.lint();
    assert!(
        findings_for(&findings, "L3")
            .iter()
            .any(|f| f.message.contains("unknown state 'nowhere'")),
        "expected unknown-state finding: {:?}",
        findings
    );
}

#[test]
fn test_boundary_tag_referencing_undeclared_boundary_fails_validation() {
    let f = Fixture::new()
        .states(
            r#"
[states.a]
label = "A"
initial = true

[states.b]
label = "B"
terminal = true
"#,
        )
        .transitions(
            r#"
[[transitions]]
from = "a"
to = "b"
command = "go"
boundary = "never-declared"
gates = []
"#,
        );
    let config = f.load();
    let (errors, _) = config.validate_deep(f.write());
    assert!(
        errors
            .iter()
            .any(|e| e.contains("never-declared") && e.contains("not declared")),
        "expected validation error for the dangling tag: {:?}",
        errors
    );
}
