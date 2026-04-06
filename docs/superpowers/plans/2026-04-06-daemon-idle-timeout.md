# Daemon Idle Timeout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add configurable idle timeout to the daemon so it doesn't die unexpectedly during idle periods, and can optionally self-terminate after a configured duration of inactivity.

**Architecture:** Add `idle_timeout: u64` to `DaemonServer`, track `last_activity: Instant` in the accept loop, check timeout in the `WouldBlock` branch. Thread idle state through to `handle_request` for status reporting. Add `--idle-timeout` CLI flag on `daemon start`.

**Tech Stack:** Rust, clap, serde_json, Unix sockets

---

### Task 1: Add idle fields to wire protocol Response

**Files:**
- Modify: `src/daemon/protocol.rs:42-62` (Response struct)
- Modify: `src/daemon/protocol.rs:110-123` (ok_status constructor)
- Modify: `src/daemon/protocol.rs:64-78` (ok_sign — add new None fields)
- Modify: `src/daemon/protocol.rs:80-93` (ok_data — add new None fields)
- Modify: `src/daemon/protocol.rs:95-108` (ok_names — add new None fields)
- Modify: `src/daemon/protocol.rs:125-138` (ok_empty — add new None fields)
- Modify: `src/daemon/protocol.rs:140-153` (err — add new None fields)
- Modify: `src/daemon/protocol.rs:155-168` (ok_verified — add new None fields)
- Test: `tests/daemon_protocol_tests.rs`

- [ ] **Step 1: Write failing tests for new status fields**

Add two tests to `tests/daemon_protocol_tests.rs`:

```rust
#[test]
fn test_serialize_ok_status_includes_idle_fields() {
    let resp = Response::ok_status(12345, 3600, 2, 120, 0);
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["pid"], 12345);
    assert_eq!(v["uptime_seconds"], 3600);
    assert_eq!(v["vault_entries"], 2);
    assert_eq!(v["idle_seconds"], 120);
    assert_eq!(v["idle_timeout"], 0);
}

#[test]
fn test_serialize_non_status_omits_idle_fields() {
    let resp = Response::ok_sign("abcdef");
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v.get("idle_seconds").is_none());
    assert!(v.get("idle_timeout").is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_serialize_ok_status_includes_idle_fields test_serialize_non_status_omits_idle_fields -- --nocapture 2>&1 | head -30`
Expected: Compilation error — `ok_status` doesn't accept 5 args yet.

- [ ] **Step 3: Add idle fields to Response struct**

In `src/daemon/protocol.rs`, add two fields to the `Response` struct after `vault_entries`:

```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_timeout: Option<u64>,
```

- [ ] **Step 4: Update all Response constructors**

In `src/daemon/protocol.rs`, update every constructor:

**`ok_status`** — change signature and body:
```rust
    pub fn ok_status(pid: u32, uptime_seconds: u64, vault_entries: usize, idle_seconds: u64, idle_timeout: u64) -> Self {
        Self {
            ok: true,
            proof: None,
            data: None,
            names: None,
            pid: Some(pid),
            uptime_seconds: Some(uptime_seconds),
            vault_entries: Some(vault_entries),
            idle_seconds: Some(idle_seconds),
            idle_timeout: Some(idle_timeout),
            verified: None,
            error: None,
            message: None,
        }
    }
```

**All other constructors** (`ok_sign`, `ok_data`, `ok_names`, `ok_empty`, `err`, `ok_verified`) — add these two lines to each `Self { ... }` block:
```rust
            idle_seconds: None,
            idle_timeout: None,
```

- [ ] **Step 5: Fix the existing test that calls ok_status with 3 args**

In `tests/daemon_protocol_tests.rs`, update `test_serialize_ok_status_response` (line 105-113):

```rust
#[test]
fn test_serialize_ok_status_response() {
    let resp = Response::ok_status(12345, 3600, 2, 0, 0);
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["pid"], 12345);
    assert_eq!(v["uptime_seconds"], 3600);
    assert_eq!(v["vault_entries"], 2);
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test daemon_protocol -- --nocapture 2>&1 | tail -20`
Expected: All daemon_protocol tests pass, including the two new ones.

- [ ] **Step 7: Commit**

```bash
git add src/daemon/protocol.rs tests/daemon_protocol_tests.rs
git commit -m "feat(daemon): add idle_seconds and idle_timeout to status response (#24)"
```

---

### Task 2: Add idle_timeout to DaemonServer and accept loop

**Files:**
- Modify: `src/daemon/mod.rs:54-64` (DaemonServer struct)
- Modify: `src/daemon/mod.rs:75` (DaemonServer::new signature)
- Modify: `src/daemon/mod.rs:132-143` (DaemonServer::new return)
- Modify: `src/daemon/mod.rs:194-226` (accept loop in start())
- Modify: `src/daemon/mod.rs:263-270` (handle_connection signature)
- Modify: `src/daemon/mod.rs:338-343` (handle_request signature)
- Modify: `src/daemon/mod.rs:388-396` (Status arm in handle_request)

- [ ] **Step 1: Add idle_timeout field to DaemonServer struct**

In `src/daemon/mod.rs`, add `idle_timeout: u64` after `start_time: Instant` in the struct:

```rust
pub struct DaemonServer {
    pub socket_path: PathBuf,
    pub pid_path: PathBuf,
    session_key: Zeroizing<Vec<u8>>,
    vault: Arc<Mutex<Vault>>,
    config_dir: PathBuf,
    data_dir: PathBuf,
    #[allow(dead_code)]
    trusted_callers: TrustedCallersManifest,
    start_time: Instant,
    idle_timeout: u64,
}
```

- [ ] **Step 2: Update DaemonServer::new to accept idle_timeout**

Change the `new` signature to:
```rust
    pub fn new(config_dir: PathBuf, data_dir: PathBuf, idle_timeout: u64) -> Result<Self, String> {
```

Add `idle_timeout` to the `Ok(DaemonServer { ... })` return at the end:
```rust
        Ok(DaemonServer {
            socket_path,
            pid_path,
            session_key,
            vault: Arc::new(Mutex::new(Vault::new())),
            config_dir,
            data_dir,
            trusted_callers,
            start_time: Instant::now(),
            idle_timeout,
        })
```

- [ ] **Step 3: Add last_activity tracking and timeout check to accept loop**

In `src/daemon/mod.rs`, in the `start()` method, add a `last_activity` local before the loop and update the accept loop:

```rust
        // Accept loop
        let mut last_activity = Instant::now();
        while RUNNING.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    // Set stream back to blocking for the connection handler
                    if let Err(e) = stream.set_nonblocking(false) {
                        eprintln!("warning: cannot set stream to blocking: {}", e);
                        continue;
                    }
                    last_activity = Instant::now();
                    let vault = Arc::clone(&self.vault);
                    let key = self.session_key.clone();
                    let start_time = self.start_time;
                    let idle_timeout = self.idle_timeout;
                    let plugin_root = self.config_dir.parent().unwrap_or(&self.config_dir);
                    handle_connection(
                        stream,
                        vault,
                        key,
                        start_time,
                        last_activity,
                        idle_timeout,
                        &self.trusted_callers,
                        plugin_root,
                    );
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No pending connection — sleep briefly to avoid busy-wait
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    // Check idle timeout
                    if self.idle_timeout > 0
                        && last_activity.elapsed().as_secs() >= self.idle_timeout
                    {
                        eprintln!(
                            "daemon: idle timeout ({}s), shutting down",
                            self.idle_timeout
                        );
                        break;
                    }
                }
                Err(e) => {
                    if RUNNING.load(Ordering::SeqCst) {
                        eprintln!("accept error: {}", e);
                    }
                }
            }
        }
```

- [ ] **Step 4: Update handle_connection signature**

Add `last_activity: Instant` and `idle_timeout: u64` parameters to `handle_connection`:

```rust
fn handle_connection(
    stream: UnixStream,
    vault: Arc<Mutex<Vault>>,
    session_key: Zeroizing<Vec<u8>>,
    start_time: Instant,
    last_activity: Instant,
    idle_timeout: u64,
    trusted_callers: &auth::TrustedCallersManifest,
    plugin_root: &Path,
) {
```

Update the `handle_request` call inside `handle_connection` (both call sites — the Status branch and the authenticated branch):

Change the Status arm:
```rust
            Ok(Request::Status) => {
                // Status is always allowed (health check).
                handle_request(Request::Status, &vault, &session_key, start_time, last_activity, idle_timeout)
            }
```

Change the authenticated arm:
```rust
            Ok(req) => {
                if authenticated {
                    handle_request(req, &vault, &session_key, start_time, last_activity, idle_timeout)
                } else {
                    Response::err("auth_failed", "caller not authenticated")
                }
            }
```

- [ ] **Step 5: Update handle_request signature and Status arm**

Change `handle_request` signature:
```rust
fn handle_request(
    req: Request,
    vault: &Arc<Mutex<Vault>>,
    session_key: &[u8],
    start_time: Instant,
    last_activity: Instant,
    idle_timeout: u64,
) -> Response {
```

Update the `Request::Status` arm:
```rust
        Request::Status => {
            let pid = std::process::id();
            let uptime = start_time.elapsed().as_secs();
            let idle_secs = last_activity.elapsed().as_secs();
            let vault_entries = match vault.lock() {
                Ok(v) => v.list().len(),
                Err(_) => 0,
            };
            Response::ok_status(pid, uptime, vault_entries, idle_secs, idle_timeout)
        }
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo build 2>&1 | tail -20`
Expected: Compilation errors in `daemon_cmd.rs` (calling `DaemonServer::new` with 2 args instead of 3) and possibly in tests. This is expected — we fix those in the next tasks.

- [ ] **Step 7: Fix DaemonServer::new call in daemon_cmd.rs**

In `src/cli/daemon_cmd.rs`, update `cmd_daemon_start` to pass `idle_timeout: 0` temporarily (we add the CLI arg in Task 3):

```rust
    let server = match DaemonServer::new(config_dir_abs, data_dir_abs, 0) {
```

- [ ] **Step 8: Verify full build succeeds**

Run: `cargo build 2>&1 | tail -10`
Expected: Build succeeds.

- [ ] **Step 9: Run all daemon tests to check nothing is broken**

Run: `cargo test daemon_protocol -- --nocapture 2>&1 | tail -20`
Expected: All pass (protocol tests were updated in Task 1).

- [ ] **Step 10: Commit**

```bash
git add src/daemon/mod.rs src/cli/daemon_cmd.rs
git commit -m "feat(daemon): add idle_timeout field and last_activity tracking in accept loop (#24)"
```

---

### Task 3: Add --idle-timeout CLI flag

**Files:**
- Modify: `src/main.rs:406-413` (DaemonAction enum)
- Modify: `src/main.rs:658-660` (DaemonAction::Start dispatch)
- Modify: `src/cli/daemon_cmd.rs:23` (cmd_daemon_start signature)

- [ ] **Step 1: Add --idle-timeout to DaemonAction::Start**

In `src/main.rs`, change `DaemonAction::Start` from a unit variant to a struct variant:

```rust
#[derive(Subcommand)]
enum DaemonAction {
    /// Start daemon in foreground
    Start {
        /// Idle timeout in seconds (0 = never timeout, default)
        #[arg(long, default_value = "0")]
        idle_timeout: u64,
    },
    /// Stop running daemon
    Stop,
    /// Query daemon status
    Status,
}
```

- [ ] **Step 2: Update DaemonAction::Start dispatch**

In `src/main.rs`, update the match arm (around line 658):

```rust
        Commands::Daemon { action } => match action {
            DaemonAction::Start { idle_timeout } => {
                let code = daemon_cmd::cmd_daemon_start(&cli.config_dir, idle_timeout);
                Box::new(LegacyResult::new("daemon_start", code))
            }
```

- [ ] **Step 3: Update cmd_daemon_start to accept idle_timeout**

In `src/cli/daemon_cmd.rs`, change the signature and pass it through:

```rust
// [cmd-daemon-start]
pub fn cmd_daemon_start(config_dir: &str, idle_timeout: u64) -> i32 {
    let config_dir_abs = resolve_config_dir(config_dir);
    let config = match load_config(&config_dir_abs) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };
    let data_dir_abs = resolve_data_dir(&config.paths.data_dir);

    let server = match DaemonServer::new(config_dir_abs, data_dir_abs, idle_timeout) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("daemon: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    if let Err(e) = server.start() {
        eprintln!("daemon: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    EXIT_SUCCESS
}
```

- [ ] **Step 4: Build and run all tests**

Run: `cargo build 2>&1 | tail -5 && cargo test 2>&1 | tail -20`
Expected: Build succeeds, all tests pass.

- [ ] **Step 5: Smoke-test the CLI help**

Run: `cargo run -- daemon start --help 2>&1`
Expected: Output includes `--idle-timeout <IDLE_TIMEOUT>` with description and default of 0.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/cli/daemon_cmd.rs
git commit -m "feat(daemon): add --idle-timeout CLI flag to daemon start (#24)"
```

---

### Task 4: E2E test for idle timeout shutdown

**Files:**
- Modify: `tests/daemon_signing_tests.rs`

- [ ] **Step 1: Update start_daemon helper to accept optional args**

In `tests/daemon_signing_tests.rs`, add a new helper alongside the existing `start_daemon`:

```rust
/// Spawn the daemon with extra CLI args.
fn start_daemon_with_args(dir: &std::path::Path, extra_args: &[&str]) -> std::process::Child {
    let mut args = vec!["--config-dir", "enforcement", "daemon", "start"];
    args.extend_from_slice(extra_args);
    std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(&args)
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start daemon")
}
```

- [ ] **Step 2: Write E2E test for idle timeout clean shutdown**

Add to `tests/daemon_signing_tests.rs`:

```rust
#[test]
#[ignore]
fn test_daemon_idle_timeout_clean_shutdown() {
    let dir = setup_dir();
    let mut daemon = start_daemon_with_args(dir.path(), &["--idle-timeout", "1"]);
    wait_for_socket(dir.path());

    let socket_path = dir.path().join("output/.sahjhan/daemon.sock");
    let pid_path = dir.path().join("output/.sahjhan/daemon.pid");

    // Confirm daemon is running.
    assert!(socket_path.exists());
    assert!(pid_path.exists());

    // Wait for idle timeout to fire (1s timeout + margin).
    std::thread::sleep(std::time::Duration::from_secs(3));

    // Daemon should have exited and cleaned up.
    let status = daemon.try_wait().expect("failed to check daemon status");
    assert!(
        status.is_some(),
        "daemon should have exited after idle timeout"
    );

    assert!(
        !socket_path.exists(),
        "socket file should be removed after idle timeout"
    );
    assert!(
        !pid_path.exists(),
        "PID file should be removed after idle timeout"
    );
}
```

- [ ] **Step 3: Write E2E test for status response with idle fields**

Add to `tests/daemon_signing_tests.rs`:

```rust
#[test]
#[ignore]
fn test_daemon_status_includes_idle_fields() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let socket_path = dir.path().join("output/.sahjhan/daemon.sock");

    // Connect and send status request.
    let mut stream = UnixStream::connect(&socket_path).expect("connect to daemon socket");
    writeln!(stream, r#"{{"op": "status"}}"#).expect("write status request");

    let reader = BufReader::new(&stream);
    let response_line = reader
        .lines()
        .next()
        .expect("should get a response")
        .expect("response should be readable");

    let val: serde_json::Value =
        serde_json::from_str(&response_line).expect("response should be valid JSON");

    assert_eq!(val["ok"], true);
    // idle_seconds should be present and small (we just connected).
    let idle_secs = val["idle_seconds"]
        .as_u64()
        .expect("idle_seconds should be a number");
    assert!(idle_secs < 5, "idle_seconds should be small, got {}", idle_secs);
    // idle_timeout should be 0 (default — no timeout).
    assert_eq!(
        val["idle_timeout"].as_u64().unwrap(),
        0,
        "idle_timeout should be 0 (default)"
    );

    stop_daemon(&mut daemon);
}
```

- [ ] **Step 4: Run the E2E tests**

Run: `cargo test --test daemon_signing_tests -- --ignored --nocapture 2>&1 | tail -30`
Expected: All tests pass, including the two new ones. The idle timeout test may take ~3s.

- [ ] **Step 5: Commit**

```bash
git add tests/daemon_signing_tests.rs
git commit -m "test(daemon): E2E tests for idle timeout shutdown and status fields (#24)"
```

---

### Task 5: Update documentation

**Files:**
- Modify: `CLAUDE.md`
- Modify: `src/daemon/mod.rs` (index header)
- Modify: `src/cli/daemon_cmd.rs` (index header)

- [ ] **Step 1: Update daemon/mod.rs index header**

The `DaemonServer::new` signature changed and the accept loop gained idle timeout behavior. Update the `## Index` comment at the top of `src/daemon/mod.rs` (lines 6-18). Replace the `DaemonServer::new` line:

```
// - DaemonServer::new         -- construct and initialize (key gen, preload check, stale cleanup, idle timeout)
```

- [ ] **Step 2: Update CLAUDE.md daemon module table**

In `CLAUDE.md`, in the `### daemon/ — Daemon Mode` table, update the `Server init` row:

```
| Server init | `daemon/mod.rs` | `DaemonServer::new` | Preload check, stale cleanup, key gen, mlock, deny debug, load trusted callers, idle timeout |
```

Add a new row after `Server start`:

```
| Idle timeout | `daemon/mod.rs` | `DaemonServer::start` | last_activity tracking in accept loop; clean shutdown on idle_timeout expiry |
```

- [ ] **Step 3: Update CLAUDE.md CLI table**

In the `### cli/ — Command Implementations` table, update the `Daemon start` row:

```
| Daemon start | `cli/daemon_cmd.rs` | `[cmd-daemon-start]` | Start daemon in foreground (accepts idle_timeout) |
```

- [ ] **Step 4: Update CLAUDE.md test file table**

In the `## Test Files` table, update the `daemon_signing_tests.rs` row:

```
| `tests/daemon_signing_tests.rs` | E2E daemon signing (deterministic proofs, sign-without-daemon), lifecycle (socket/PID creation, stop cleanup, status, preload rejection, idle timeout shutdown) |
```

And update the `daemon_protocol_tests.rs` row:

```
| `tests/daemon_protocol_tests.rs` | Wire protocol types: Request deserialization (all ops + unknowns), Response serialization (all constructors incl. idle fields) |
```

- [ ] **Step 5: Verify documentation anchors**

Run: `grep -n "idle_timeout\|idle_seconds\|idle timeout" src/daemon/mod.rs src/daemon/protocol.rs src/cli/daemon_cmd.rs src/main.rs | head -20`
Expected: References in all four files, confirming the feature is present where documented.

- [ ] **Step 6: Commit**

```bash
git add CLAUDE.md src/daemon/mod.rs src/cli/daemon_cmd.rs
git commit -m "docs: update CLAUDE.md and index headers for daemon idle timeout (#24)"
```

---

### Task 6: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: All tests pass (416+ tests).

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1 | tail -10`
Expected: No warnings.

- [ ] **Step 3: Run fmt**

Run: `cargo fmt -- --check 2>&1`
Expected: No formatting issues.

- [ ] **Step 4: Run E2E daemon tests**

Run: `cargo test --test daemon_signing_tests -- --ignored --nocapture 2>&1 | tail -30`
Expected: All pass.
