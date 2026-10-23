// concurrent_append_tests.rs — Stress tests for concurrent ledger writes (Issue #21)
//
// Validates that concurrent appends to the same ledger file produce contiguous
// sequence numbers and a valid hash chain, and that concurrent CLI writers
// never leave the manifest disagreeing with the files it seals
// (holtz#ho-0mvwg8q).

use sahjhan::ledger::chain::Ledger;
use sahjhan::provenance::Recorder;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use tempfile::tempdir;

fn fields(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Simulate the CLI code path: open ledger from disk, append one event.
/// This is what `sahjhan event` does — each invocation opens the file fresh.
fn open_and_append(path: &std::path::Path, event_type: &str, id: usize) -> Result<(), String> {
    let mut ledger = Ledger::open(path).map_err(|e| format!("open failed ({}): {}", id, e))?;
    ledger
        .append(
            event_type,
            fields(&[("worker", &id.to_string())]),
            &Recorder::AgentCli,
        )
        .map_err(|e| format!("append failed ({}): {}", id, e))?;
    Ok(())
}

/// Two threads each open the ledger and append one event concurrently.
/// This is the minimal reproduction case from the issue.
#[test]
fn test_concurrent_two_thread_append() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("concurrent.jsonl");

    // Create ledger with genesis
    Ledger::init(&path, "test-proto", "1.0.0").unwrap();

    let path1 = path.clone();
    let path2 = path.clone();

    let barrier = Arc::new(Barrier::new(2));
    let b1 = barrier.clone();
    let b2 = barrier.clone();

    let t1 = thread::spawn(move || {
        b1.wait(); // sync start
        open_and_append(&path1, "test_event", 1)
    });

    let t2 = thread::spawn(move || {
        b2.wait(); // sync start
        open_and_append(&path2, "test_event", 2)
    });

    let r1 = t1.join().unwrap();
    let r2 = t2.join().unwrap();

    // Both appends should succeed
    if let Err(e) = &r1 {
        eprintln!("Thread 1 error: {}", e);
    }
    if let Err(e) = &r2 {
        eprintln!("Thread 2 error: {}", e);
    }

    // The real check: can we open and verify the ledger?
    let result = Ledger::open(&path);
    match result {
        Ok(ledger) => {
            assert_eq!(
                ledger.len(),
                3,
                "Expected genesis + 2 events, got {}",
                ledger.len()
            );
            // Verify contiguous sequence numbers
            for (i, entry) in ledger.entries().iter().enumerate() {
                assert_eq!(
                    entry.seq, i as u64,
                    "Sequence gap: expected {}, got {}",
                    i, entry.seq
                );
            }
        }
        Err(e) => {
            panic!(
                "Ledger corrupted after concurrent writes: {}. \
                 This confirms issue #21 — concurrent appends produce \
                 sequence gaps or chain breaks.",
                e
            );
        }
    }
}

/// Stress test: N threads each open-and-append to the same ledger.
/// Runs multiple iterations to increase the chance of hitting the race window.
#[test]
fn test_concurrent_stress_many_threads() {
    let iterations = 20;
    let threads_per_iter = 5;
    let mut failures = 0;

    for iter in 0..iterations {
        let dir = tempdir().unwrap();
        let path = dir.path().join(format!("stress_{}.jsonl", iter));

        Ledger::init(&path, "test-proto", "1.0.0").unwrap();

        let barrier = Arc::new(Barrier::new(threads_per_iter));
        let handles: Vec<_> = (0..threads_per_iter)
            .map(|i| {
                let p = path.clone();
                let b = barrier.clone();
                thread::spawn(move || {
                    b.wait();
                    open_and_append(&p, "stress_event", i)
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let append_errors: Vec<_> = results.iter().filter(|r| r.is_err()).collect();

        match Ledger::open(&path) {
            Ok(ledger) => {
                let expected = 1 + threads_per_iter - append_errors.len();
                if ledger.len() != expected {
                    eprintln!(
                        "Iter {}: expected {} entries, got {} (append errors: {})",
                        iter,
                        expected,
                        ledger.len(),
                        append_errors.len()
                    );
                    failures += 1;
                }
                // Verify sequence contiguity
                for (i, entry) in ledger.entries().iter().enumerate() {
                    if entry.seq != i as u64 {
                        eprintln!(
                            "Iter {}: sequence gap at position {}: expected {}, got {}",
                            iter, i, i, entry.seq
                        );
                        failures += 1;
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("Iter {}: ledger open failed: {}", iter, e);
                failures += 1;
            }
        }
    }

    if failures > 0 {
        panic!(
            "{} of {} iterations produced corrupted ledgers. \
             This confirms issue #21 — the lock critical section does not \
             cover seq assignment.",
            failures, iterations
        );
    }
}

/// Rapid sequential open-append cycles from a single thread (baseline sanity check).
/// This should always pass since there's no concurrency.
#[test]
fn test_sequential_rapid_appends_no_corruption() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sequential.jsonl");

    Ledger::init(&path, "test-proto", "1.0.0").unwrap();

    for i in 0..50 {
        open_and_append(&path, "seq_event", i).unwrap();
    }

    let ledger = Ledger::open(&path).unwrap();
    assert_eq!(ledger.len(), 51); // genesis + 50 events
    for (i, entry) in ledger.entries().iter().enumerate() {
        assert_eq!(entry.seq, i as u64);
    }
}

// ---------------------------------------------------------------------------
// The manifest seal under concurrent CLI writers (holtz#ho-0mvwg8q)
// ---------------------------------------------------------------------------

fn sahjhan(project: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .current_dir(project)
        .arg("--config-dir")
        .arg("enforcement")
        .args(args)
        .output()
        .expect("spawn sahjhan")
}

fn describe(out: &Output) -> String {
    format!(
        "exit {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A project shaped like holtz's: `finding` re-renders three managed files,
/// and `ping` — holtz's per-tool-call courier events — renders nothing. The
/// `begin` transition's gate records a `ping` itself, and a PostToolUse hook
/// auto-records one: the two other ways an append reaches the ledger. The
/// `race` transition's gate takes `begin` itself, so the machine leaves the
/// state `race` is being checked in while `race` is being checked.
fn render_project() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("enforcement");
    std::fs::create_dir_all(cfg.join("templates")).unwrap();
    let write = |name: &str, body: &str| std::fs::write(cfg.join(name), body).unwrap();
    write(
        "protocol.toml",
        r#"[protocol]
name = "race"
version = "1.0.0"
description = "concurrent writers"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"
"#,
    );
    write(
        "states.toml",
        "[states.idle]\nlabel = \"Idle\"\ninitial = true\n\n\
         [states.working]\nlabel = \"Working\"\n\n\
         [states.done]\nlabel = \"Done\"\n",
    );
    write(
        "transitions.toml",
        &format!(
            "[[transitions]]\nfrom = \"idle\"\nto = \"working\"\ncommand = \"begin\"\n\
             gates = [{{ type = \"command_succeeds\", cmd = \"'{bin}' --config-dir enforcement event ping\" }}]\n\n\
             [[transitions]]\nfrom = \"idle\"\nto = \"done\"\ncommand = \"race\"\n\
             gates = [{{ type = \"command_succeeds\", cmd = \"'{bin}' --config-dir enforcement transition begin\" }}]\n",
            bin = env!("CARGO_BIN_EXE_sahjhan")
        ),
    );
    write(
        "hooks.toml",
        "[[hooks]]\nevent = \"PostToolUse\"\ntools = [\"Bash\"]\n\n\
         [hooks.auto_record]\nevent_type = \"ping\"\n",
    );
    write(
        "events.toml",
        r#"[events.finding]
description = "A finding"
fields = [{ name = "id", type = "string" }]

[events.ping]
description = "A tool call"
fields = []
"#,
    );
    let mut renders = String::new();
    for target in ["PUNCHLIST.md", "PUNCHLIST-ARCHIVE.md", "STATUS.md"] {
        renders.push_str(&format!(
            "[[renders]]\ntarget = \"{target}\"\ntemplate = \"templates/{target}.tera\"\n\
             trigger = \"on_event\"\nevent_types = [\"finding\"]\n\n"
        ));
        write(
            &format!("templates/{target}.tera"),
            &format!(
                "# {target}\n{{% for e in events %}}{{% if e.event_type == \"finding\" %}}\
                 - {{{{ e.fields.id }}}} at {{{{ e.seq }}}}\n{{% endif %}}{{% endfor %}}\
                 {{{{ events | length }}}} events\n"
            ),
        );
    }
    write("renders.toml", &renders);

    let out = sahjhan(dir.path(), &["init"]);
    assert!(out.status.success(), "init: {}", describe(&out));
    dir
}

/// N concurrent `sahjhan event` writers, some of whose events render and some
/// of whose do not, with `manifest verify` looping beside them. Every verify
/// must come back clean — there is nothing writing to a managed file but
/// sahjhan — and when the writers stop, the manifest must seal exactly the
/// files on disk, and those files must show every finding.
///
/// The two failures it drives, each a permanent violation to holtz
/// (holtz#ho-0mvwg8q): a verify that reads between a render and its seal, and
/// an event that renders nothing saving a manifest it loaded before another
/// writer's seal.
#[test]
fn test_concurrent_events_renders_and_verify_agree() {
    let project = render_project();
    let root = project.path().to_path_buf();

    const FINDING_WRITERS: usize = 4;
    const PING_WRITERS: usize = 4;
    const EVENTS_EACH: usize = 12;

    let stop = Arc::new(AtomicBool::new(false));
    let verify_failures = Arc::new(Mutex::new(Vec::<String>::new()));
    let verifies = Arc::new(Mutex::new(0usize));

    let verifier = {
        let (root, stop) = (root.clone(), stop.clone());
        let (failures, verifies) = (verify_failures.clone(), verifies.clone());
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let out = sahjhan(&root, &["manifest", "verify"]);
                *verifies.lock().unwrap() += 1;
                if !out.status.success() {
                    failures.lock().unwrap().push(describe(&out));
                }
            }
        })
    };

    let barrier = Arc::new(Barrier::new(FINDING_WRITERS + PING_WRITERS));
    let writers: Vec<_> = (0..FINDING_WRITERS + PING_WRITERS)
        .map(|w| {
            let (root, barrier) = (root.clone(), barrier.clone());
            thread::spawn(move || {
                barrier.wait();
                let mut failed = Vec::new();
                for i in 0..EVENTS_EACH {
                    let id = format!("id=w{}-{}", w, i);
                    let args: Vec<&str> = if w < FINDING_WRITERS {
                        vec!["event", "finding", "--field", &id]
                    } else {
                        vec!["event", "ping"]
                    };
                    let out = sahjhan(&root, &args);
                    if !out.status.success() {
                        failed.push(describe(&out));
                    }
                }
                failed
            })
        })
        .collect();

    let writer_failures: Vec<String> = writers
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();
    stop.store(true, Ordering::SeqCst);
    verifier.join().unwrap();

    assert!(
        writer_failures.is_empty(),
        "{} writer(s) failed; first:\n{}",
        writer_failures.len(),
        writer_failures[0]
    );
    let failures = verify_failures.lock().unwrap();
    assert!(
        failures.is_empty(),
        "{} of {} verifies run beside the writers failed; first:\n{}",
        failures.len(),
        verifies.lock().unwrap(),
        failures[0]
    );

    let out = sahjhan(&root, &["manifest", "verify"]);
    assert!(out.status.success(), "final verify: {}", describe(&out));

    // A clean verify of a manifest that tracks nothing proves nothing: every
    // rendered file and the ledger must be among what it sealed.
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("output/.sahjhan/manifest.json")).unwrap(),
    )
    .unwrap();
    for key in [
        "output/PUNCHLIST.md",
        "output/PUNCHLIST-ARCHIVE.md",
        "output/STATUS.md",
        "output/.sahjhan/ledger.jsonl",
    ] {
        assert!(
            manifest["entries"].get(key).is_some(),
            "manifest does not seal {}: {}",
            key,
            manifest["entries"]
        );
    }

    // Renders land in append order, so the last one shows every finding. A
    // render built from a ledger older than one already sealed would drop some.
    let punchlist = std::fs::read_to_string(root.join("output/PUNCHLIST.md")).unwrap();
    for w in 0..FINDING_WRITERS {
        for i in 0..EVENTS_EACH {
            let id = format!("- w{}-{} at", w, i);
            assert!(
                punchlist.contains(&id),
                "PUNCHLIST.md lacks {}:\n{}",
                id,
                punchlist
            );
        }
    }
}

/// A gate command that records an event is a second writer inside the first.
/// The transition must take the manifest lock only after its gates have run;
/// held across them, the gate's `sahjhan event` waits on its own parent until
/// the lock times out, and the gate fails.
#[test]
fn test_transition_gate_that_records_an_event_does_not_wait_on_its_parent() {
    let project = render_project();
    let root = project.path();

    let started = std::time::Instant::now();
    let out = sahjhan(root, &["transition", "begin"]);
    let took = started.elapsed();
    assert!(out.status.success(), "transition: {}", describe(&out));
    assert!(
        took < std::time::Duration::from_secs(10),
        "transition took {:?}: its gate waited on the lock",
        took
    );

    let ledger = Ledger::open(&root.join("output/.sahjhan/ledger.jsonl")).unwrap();
    let types: Vec<&str> = ledger
        .entries()
        .iter()
        .map(|e| e.event_type.as_str())
        .collect();
    assert_eq!(
        types,
        ["genesis", "ping", "state_transition", "gate_attestation"],
        "{:?}",
        types
    );

    let out = sahjhan(root, &["manifest", "verify"]);
    assert!(out.status.success(), "verify: {}", describe(&out));
}

/// A transition's gates are checked in one state and its record appended
/// later. When the machine moves in between — here `race`'s own gate takes
/// `begin` — the commit must refuse rather than record `from = idle` for a
/// machine that is in `working`, and move it to a target chosen from a state
/// it has left.
#[test]
fn test_transition_refuses_when_its_gate_moved_the_state() {
    let project = render_project();
    let root = project.path();

    let out = sahjhan(root, &["transition", "race"]);
    assert_eq!(out.status.code(), Some(1), "race: {}", describe(&out));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("'idle'") && stderr.contains("'working'"),
        "the refusal names both states: {}",
        stderr
    );

    let ledger = Ledger::open(&root.join("output/.sahjhan/ledger.jsonl")).unwrap();
    let moves: Vec<(String, String)> = ledger
        .events_of_type("state_transition")
        .iter()
        .map(|e| (e.fields["from"].clone(), e.fields["to"].clone()))
        .collect();
    assert_eq!(
        moves,
        [("idle".to_string(), "working".to_string())],
        "only the gate's own transition is recorded"
    );

    let out = sahjhan(root, &["manifest", "verify"]);
    assert!(out.status.success(), "verify: {}", describe(&out));
}

/// Every append path seals the ledger it grows, `hook eval`'s auto-record and
/// `ledger checkpoint` included. An append that does not leaves the manifest's
/// ledger entry stale, and every `manifest verify` reads the ledger as
/// tampered with until some later `event` re-seals it.
#[test]
fn test_auto_record_and_checkpoint_seal_the_ledger() {
    let project = render_project();
    let root = project.path();

    let out = sahjhan(
        root,
        &["hook", "eval", "--event", "PostToolUse", "--tool", "Bash"],
    );
    assert!(out.status.success(), "hook eval: {}", describe(&out));
    let ledger = Ledger::open(&root.join("output/.sahjhan/ledger.jsonl")).unwrap();
    assert_eq!(ledger.entries().last().unwrap().event_type, "ping");
    let out = sahjhan(root, &["manifest", "verify"]);
    assert!(
        out.status.success(),
        "verify after auto-record: {}",
        describe(&out)
    );

    let out = sahjhan(root, &["ledger", "checkpoint", "--name", "default"]);
    assert!(out.status.success(), "checkpoint: {}", describe(&out));
    let out = sahjhan(root, &["manifest", "verify"]);
    assert!(
        out.status.success(),
        "verify after checkpoint: {}",
        describe(&out)
    );
}

/// A run ledger registered beside the data dir, the way holtz registers
/// `runs/<n>/ledger.jsonl`, and appended to from a subdirectory by name and by
/// a cwd-relative `--ledger-path` that climbs with `..`. Each append seals that
/// ledger under its one project-root key, so `manifest verify` catches it
/// truncated by its last entry, which the hash chain cannot.
#[test]
fn test_an_append_to_a_named_ledger_seals_that_ledger() {
    let project = render_project();
    let root = project.path();
    let run = root.join("output/runs/1/ledger.jsonl");

    let out = sahjhan(
        root,
        &[
            "ledger",
            "create",
            "--name",
            "run-1",
            "--path",
            "output/runs/1/ledger.jsonl",
        ],
    );
    assert!(out.status.success(), "create: {}", describe(&out));

    let sub = root.join("output");
    for target in [
        ["--ledger", "run-1"],
        ["--ledger-path", "../output/runs/1/ledger.jsonl"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_sahjhan"))
            .current_dir(&sub)
            .args(["--config-dir", "../enforcement", "event", "ping"])
            .args(target)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "event {:?} from a subdir: {}",
            target,
            describe(&out)
        );
    }

    let manifest = std::fs::read_to_string(root.join("output/.sahjhan/manifest.json")).unwrap();
    assert!(
        manifest.contains("\"output/runs/1/ledger.jsonl\""),
        "the run ledger is sealed under its project-root key: {}",
        manifest
    );
    assert!(
        !manifest.contains("\"runs/1/ledger.jsonl\"") && !manifest.contains(".."),
        "and under no cwd-relative spelling: {}",
        manifest
    );
    let out = sahjhan(root, &["manifest", "verify"]);
    assert!(out.status.success(), "verify: {}", describe(&out));

    // Drop the last entry. What is left still chains.
    let content = std::fs::read_to_string(&run).unwrap();
    let mut lines: Vec<&str> = content.lines().collect();
    lines.pop();
    std::fs::write(&run, format!("{}\n", lines.join("\n"))).unwrap();
    let out = sahjhan(root, &["log", "verify", "--ledger", "run-1"]);
    assert!(
        out.status.success(),
        "a truncated ledger still chains: {}",
        describe(&out)
    );

    let out = sahjhan(root, &["manifest", "verify"]);
    assert!(
        !out.status.success(),
        "the manifest catches the truncation: {}",
        describe(&out)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("output/runs/1/ledger.jsonl")
            || String::from_utf8_lossy(&out.stderr).contains("output/runs/1/ledger.jsonl"),
        "and names the ledger: {}",
        describe(&out)
    );
}

/// A ledger outside every managed path is not the manifest's to seal. An
/// append to it succeeds and leaves it untracked, rather than failing E13.
#[test]
fn test_an_append_to_an_unmanaged_ledger_leaves_it_unsealed() {
    let project = render_project();
    let root = project.path();

    let out = sahjhan(
        root,
        &[
            "ledger",
            "create",
            "--name",
            "scratch",
            "--path",
            "scratch/ledger.jsonl",
        ],
    );
    assert!(out.status.success(), "create: {}", describe(&out));
    let out = sahjhan(root, &["event", "ping", "--ledger", "scratch"]);
    assert!(out.status.success(), "event: {}", describe(&out));

    let manifest = std::fs::read_to_string(root.join("output/.sahjhan/manifest.json")).unwrap();
    assert!(!manifest.contains("scratch"), "{}", manifest);
    let out = sahjhan(root, &["manifest", "verify"]);
    assert!(out.status.success(), "verify: {}", describe(&out));
}
