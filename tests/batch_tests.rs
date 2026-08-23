// tests/batch_tests.rs
//
// `sahjhan batch <name>` — one transition applied to every item a named query
// returns, per selected step.
//
// The fixture is the shape the feature was built for: findings with a
// severity, a deferral transition per severity, and a budget gate on one of
// them. A batch has to reach that budget and stop there without failing — a
// cap that turns the whole command into an error is a cap nobody can spend.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

const PROTOCOL: &str = r#"
[protocol]
name = "batch-demo"
version = "1.0.0"
description = "Batch demo protocol"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"

[aliases]
"defer batch" = "batch defer"

[queries.open_low_findings]
sql = "SELECT DISTINCT f.id AS id FROM events f WHERE f.type='finding' AND f.severity='LOW' AND f.id NOT IN (SELECT d.id FROM events d WHERE d.type='finding_deferred')"
intent = "open LOW findings"

[queries.open_medium_findings]
sql = "SELECT DISTINCT f.id AS id FROM events f WHERE f.type='finding' AND f.severity='MEDIUM' AND f.id NOT IN (SELECT d.id FROM events d WHERE d.type='finding_deferred')"
intent = "open MEDIUM findings"

[queries.item_open]
sql = "SELECT count(*) = 0 FROM events WHERE type='finding_deferred' AND id='{{item_id}}'"
intent = "finding must not already be deferred"

[queries.medium_budget]
sql = "SELECT (SELECT count(DISTINCT id) FROM events WHERE type='finding_deferred' AND reason='medium_budget') < 1"
intent = "at most one MEDIUM deferral"

[batches.defer]
description = "Defer findings in bulk, by severity"
param = "severity"
steps = [
    { value = "low", items = "open_low_findings", transition = "defer_low" },
    { value = "medium", items = "open_medium_findings", transition = "defer_medium" },
]
"#;

const STATES: &str = r#"
[states.fix_loop]
label = "Fix loop"
initial = true
"#;

const TRANSITIONS: &str = r#"
[[transitions]]
from = "fix_loop"
to = "fix_loop"
command = "defer_low"
args = ["item_id"]
gates = [
    { type = "query", query = "item_open", expect = "true" },
]
emits = [
    { event = "finding_deferred", fields = { id = "{{item_id}}", reason = "low_priority" } },
]

[[transitions]]
from = "fix_loop"
to = "fix_loop"
command = "defer_medium"
args = ["item_id"]
gates = [
    { type = "query", query = "item_open", expect = "true" },
    { type = "query", query = "medium_budget", expect = "true" },
]
emits = [
    { event = "finding_deferred", fields = { id = "{{item_id}}", reason = "medium_budget" } },
]
"#;

const EVENTS: &str = r#"
[events.finding]
description = "A finding"
fields = [
    { name = "id", type = "string" },
    { name = "severity", type = "string" },
]

[events.finding_deferred]
description = "A finding was deferred"
fields = [
    { name = "id", type = "string" },
    { name = "reason", type = "string" },
]
"#;

const RENDERS: &str = r#"
[[renders]]
target = "DEFERRED.md"
template = "templates/deferred.md.tera"
trigger = "on_event"
event_types = ["finding_deferred"]
"#;

const DEFERRED_TEMPLATE: &str = r#"# Deferred
{% for event in events %}{% if event.event_type == "finding_deferred" %}
- {{ event.fields.id }} ({{ event.fields.reason }}){% endif %}{% endfor %}
"#;

/// A project whose ledger carries `findings` at the given severities.
fn setup(findings: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(config_dir.join("templates")).unwrap();
    std::fs::write(config_dir.join("protocol.toml"), PROTOCOL).unwrap();
    std::fs::write(config_dir.join("states.toml"), STATES).unwrap();
    std::fs::write(config_dir.join("transitions.toml"), TRANSITIONS).unwrap();
    std::fs::write(config_dir.join("events.toml"), EVENTS).unwrap();
    std::fs::write(config_dir.join("renders.toml"), RENDERS).unwrap();
    std::fs::write(
        config_dir.join("templates/deferred.md.tera"),
        DEFERRED_TEMPLATE,
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    for (id, severity) in findings {
        Command::cargo_bin("sahjhan")
            .unwrap()
            .args([
                "--config-dir",
                "enforcement",
                "event",
                "finding",
                "--field",
                &format!("id={}", id),
                "--field",
                &format!("severity={}", severity),
            ])
            .current_dir(dir.path())
            .assert()
            .success();
    }
    dir
}

fn deferred_ids(dir: &tempfile::TempDir) -> Vec<String> {
    let out = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "query",
            "SELECT id FROM events WHERE type='finding_deferred'",
            "--format",
            "jsonl",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            l.split("\"id\":\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .map(str::to_string)
        })
        .collect()
}

#[test]
fn a_batch_applies_its_transition_to_every_item_the_query_returns() {
    let dir = setup(&[("BH-001", "LOW"), ("BH-002", "LOW"), ("BH-003", "HIGH")]);

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "batch",
            "defer",
            "--severity",
            "low",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("2 applied"));

    let mut ids = deferred_ids(&dir);
    ids.sort();
    assert_eq!(ids, vec!["BH-001", "BH-002"], "the HIGH must be untouched");
}

#[test]
fn one_command_spends_both_steps_and_stops_at_the_budget() {
    // The escape the feature exists for: every open LOW *and* the MEDIUM
    // budget, in one command. The budget gate refuses the second MEDIUM, and
    // that refusal is the batch working, not failing.
    let dir = setup(&[
        ("BH-001", "LOW"),
        ("BH-002", "MEDIUM"),
        ("BH-003", "MEDIUM"),
    ]);

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "batch",
            "defer",
            "--severity",
            "low,medium",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("2 applied"))
        .stdout(predicate::str::contains("1 refused"))
        .stdout(predicate::str::contains("medium_budget"));

    assert_eq!(deferred_ids(&dir).len(), 2);
}

#[test]
fn the_command_that_defers_is_the_command_that_writes_the_view() {
    let dir = setup(&[("BH-001", "LOW")]);
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "batch",
            "defer",
            "--severity",
            "low",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let rendered = std::fs::read_to_string(dir.path().join("output/DEFERRED.md")).unwrap();
    assert!(
        rendered.contains("BH-001"),
        "the batch did not trigger the on_event render: {}",
        rendered
    );
}

#[test]
fn the_multi_word_alias_reaches_the_batch() {
    // `defer batch` is two words; until 0.22.0 alias keys matched only the
    // first, so this spelling died at clap with "unrecognized subcommand".
    let dir = setup(&[("BH-001", "LOW")]);
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "defer",
            "batch",
            "--severity",
            "low",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("1 applied"));
}

#[test]
fn a_selector_naming_no_step_is_an_error() {
    // Silence here would look exactly like "nothing to defer".
    let dir = setup(&[("BH-001", "LOW")]);
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "batch",
            "defer",
            "--severity",
            "critical",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("no step named 'critical'"));

    assert!(deferred_ids(&dir).is_empty());
}

#[test]
fn an_unknown_flag_is_an_error() {
    let dir = setup(&[("BH-001", "LOW")]);
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "batch",
            "defer",
            "--level",
            "low",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--severity"));
}

#[test]
fn no_selector_runs_every_step() {
    let dir = setup(&[("BH-001", "LOW"), ("BH-002", "MEDIUM")]);
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "batch", "defer"])
        .current_dir(dir.path())
        .assert()
        .success();
    assert_eq!(deferred_ids(&dir).len(), 2);
}

#[test]
fn validate_rejects_a_step_naming_an_undeclared_query() {
    let dir = setup(&[]);
    let broken = PROTOCOL.replace("items = \"open_low_findings\"", "items = \"no_such_query\"");
    std::fs::write(dir.path().join("enforcement/protocol.toml"), broken).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "validate"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("no_such_query"));
}

#[test]
fn validate_rejects_a_step_applying_an_undefined_transition() {
    let dir = setup(&[]);
    let broken = PROTOCOL.replace(
        "transition = \"defer_low\"",
        "transition = \"defer_nothing\"",
    );
    std::fs::write(dir.path().join("enforcement/protocol.toml"), broken).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "validate"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("defer_nothing"));
}
