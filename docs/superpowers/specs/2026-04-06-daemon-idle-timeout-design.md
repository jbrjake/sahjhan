# Configurable Daemon Idle Timeout

**Issue:** #24
**Date:** 2026-04-06

## Problem

During Holtz audits, the `awaiting_clear` state has no hook activity — no tool use, no `PreToolUse`/`PostToolUse` events. The user may be away for minutes or hours. If the daemon dies during this window, the session key is lost and the ledger becomes permanently unwritable. The audit is irrecoverably dead.

The daemon currently has no configurable idle timeout and no mechanism to prevent unexpected termination during idle periods.

## Solution

Add `--idle-timeout <SECONDS>` to `sahjhan daemon start`. Default `0` (never timeout). Expose idle metrics in the status response. Ensure timeout-triggered shutdown follows the same clean shutdown path as SIGTERM.

## Changes

### 1. DaemonServer struct (daemon/mod.rs)

Add `idle_timeout: u64` field to `DaemonServer`. The `new()` constructor takes it as a parameter.

In `start()`, add a `last_activity: Instant` local variable initialized to `Instant::now()`. Update it to `Instant::now()` on each accepted connection (before handling).

In the `WouldBlock` branch of the accept loop, after the existing 50ms sleep:

```rust
if self.idle_timeout > 0
    && last_activity.elapsed().as_secs() >= self.idle_timeout
{
    eprintln!("daemon: idle timeout ({}s), shutting down", self.idle_timeout);
    break;
}
```

Breaking falls through to the existing `self.cleanup()` call — same shutdown path as SIGTERM/SIGINT.

### 2. Wire protocol (daemon/protocol.rs)

Add two fields to `Response`:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub idle_seconds: Option<u64>,
#[serde(skip_serializing_if = "Option::is_none")]
pub idle_timeout: Option<u64>,
```

Update `ok_status()` signature to accept `idle_seconds: u64` and `idle_timeout: u64`, and populate both fields. All other constructors set them to `None`.

### 3. Status handler (daemon/mod.rs)

Pass `last_activity: Instant` and `idle_timeout: u64` into `handle_connection` and `handle_request`.

In the `Request::Status` arm of `handle_request`, compute `idle_seconds = last_activity.elapsed().as_secs()` and pass both values to `Response::ok_status()`.

A status request IS socket activity (the connection was accepted, resetting `last_activity`), so `idle_seconds` will be ~0 during the response. This is intentional: health-check polling naturally keeps the daemon alive.

### 4. CLI (main.rs + cli/daemon_cmd.rs)

Add `--idle-timeout` to `DaemonAction::Start`:

```rust
Start {
    /// Idle timeout in seconds (0 = never, default)
    #[arg(long, default_value = "0")]
    idle_timeout: u64,
},
```

Update `cmd_daemon_start()` to accept and forward the value to `DaemonServer::new()`.

## Behavior

| idle_timeout | Behavior |
|---|---|
| `0` (default) | Daemon runs until SIGTERM/SIGINT or `daemon stop`. Fully backward compatible. |
| `> 0` | Daemon shuts down cleanly after N seconds of no socket activity. |

Clean shutdown on timeout: vault zeroed on drop, socket file removed, PID file removed. Identical to `daemon stop` outcome.

## Status response example

```json
{
  "ok": true,
  "pid": 12345,
  "uptime_seconds": 3600,
  "idle_seconds": 0,
  "idle_timeout": 0,
  "vault_entries": 2
}
```

## Files changed

| File | Change |
|---|---|
| `src/daemon/mod.rs` | `idle_timeout` field, `last_activity` tracking in accept loop, timeout check, pass idle state to handlers |
| `src/daemon/protocol.rs` | `idle_seconds` + `idle_timeout` on Response, updated `ok_status()` |
| `src/main.rs` | `--idle-timeout` on `DaemonAction::Start` |
| `src/cli/daemon_cmd.rs` | Thread `idle_timeout` from CLI to `DaemonServer::new()` |
| `tests/daemon_protocol_tests.rs` | Verify new fields serialize correctly in status response |
| `tests/daemon_signing_tests.rs` | Update any tests that construct or assert on status responses |

## Test plan

- Protocol test: `ok_status` with new fields serializes `idle_seconds` and `idle_timeout`
- Protocol test: non-status responses omit `idle_seconds` and `idle_timeout` (skip_serializing_if)
- E2E test: daemon with `idle_timeout=1` shuts down cleanly after 1-2s idle (socket and PID files removed)
- E2E test: daemon with `idle_timeout=0` does not self-terminate (existing behavior preserved)
- E2E test: status response includes both new fields with correct values

## Non-goals

- No TOML config for idle timeout (CLI flag only, per issue discussion).
- No daemon restart/recovery. Once the key is gone, it's gone.
- No supervision/watchdog. The daemon simply avoids dying when it doesn't need to.
