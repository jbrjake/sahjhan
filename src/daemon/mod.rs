// src/daemon/mod.rs
//
// Daemon mode: holds secrets in process memory, serves signing and vault
// operations over a Unix domain socket.
//
// ## Index
// - CONNECTION_IO_TIMEOUT     -- per-connection read/write bound (a silent client must not wedge the daemon)
// - socket_path_for           -- daemon socket path (SAHJHAN_DAEMON_SOCKET override, else data_dir/daemon.sock)
// - DaemonServer              -- main server struct
// - DaemonServer::new         -- construct and initialize (key gen, preload check, stale cleanup, idle timeout, sandbox fuse)
// - DaemonServer::start       -- bind socket, accept loop, signal handling
// - DaemonServer::cleanup     -- remove socket and PID files
// - handle_connection         -- read JSON lines from a stream, dispatch, respond
// - handle_request            -- match Request variant to operation; enforcement_read/write/update/merge ops; _-prefixed vault namespace guard
// - PatchMode                 -- top-level (enforcement_update) vs recursive (enforcement_merge) patch application
// - decode_enforcement_object -- base64 + JSON-object decode shared by every enforcement mutation
// - version_refusal           -- compare-and-set check, run inside the vault lock (#49)
// - handle_enforcement_write  -- whole-blob replace, optionally conditional on version
// - handle_enforcement_patch  -- read-merge-store under one lock, either patch mode
// - handle_record_event       -- authenticated ledger append for a trusted peer (ledger-write analog of enforcement_write)
// - overlay_ledger_state      -- override enforcement blob "state" with ledger-derived state (holtz #57)
// - derive_ledger_state       -- resolve active ledger, verify chain, derive current state
// - compute_sign              -- HMAC-SHA256 signing (same algorithm as authed_event.rs)
// - build_canonical_payload   -- canonical HMAC payload from event_type + fields
// - mod platform              -- OS-specific APIs
// - mod vault                 -- in-memory secret store
// - mod protocol              -- wire protocol types
// - mod auth                  -- caller authentication
// - mod fuse                  -- sandbox fuse (refuse privileged ops when the boundary is absent)
// - mod enforcement           -- merge semantics and version token for the enforcement blob

pub mod auth;
pub mod enforcement;
pub mod fuse;
pub mod platform;
pub mod protocol;
pub mod vault;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use self::auth::TrustedCallersManifest;
use self::protocol::{Request, Response};
use self::vault::Vault;
use crate::cli::commands::{
    load_manifest, open_targeted_ledger, resolve_data_dir, resolve_ledger_from_targeting,
    LedgerTargeting, EXIT_SUCCESS,
};
use crate::cli::transition::{record_and_render, validate_event_fields};
use crate::config::{ProtocolConfig, VaultAccess};
use crate::hooks::eval::derive_current_state;
use crate::ledger::chain::Ledger;
use crate::state::machine::StateMachine;

type HmacSha256 = Hmac<Sha256>;

/// Resolve the daemon socket path: a non-empty `SAHJHAN_DAEMON_SOCKET`
/// overrides the default `data_dir/daemon.sock`.
///
/// The Python consumer hooks already honor this variable with the same
/// semantics (empty string falls through to the default). The override
/// exists so the socket can live *outside* the project working directory:
/// a sandboxed session that denies unix sockets and denies writes to the
/// socket's directory leaves the agent unable to connect to it or pre-bind
/// (squat) its path, while hooks running outside the sandbox still reach
/// it. The PID file intentionally stays in `data_dir` — consumers watch it
/// there to detect a dead daemon, and it guards nothing.
pub fn socket_path_for(data_dir: &Path) -> PathBuf {
    match std::env::var("SAHJHAN_DAEMON_SOCKET") {
        Ok(s) if !s.is_empty() => PathBuf::from(s),
        _ => data_dir.join("daemon.sock"),
    }
}

/// How long a connected client may leave the daemon waiting on a read or
/// write before the connection is dropped.
///
/// The daemon serves connections sequentially, so without this bound one
/// client that connects and goes silent wedges every other caller — and
/// under fail-closed consumers, availability *is* integrity. Requests are
/// single-line JSON over a local socket; a client that needs ten silent
/// seconds mid-connection is not a client, it is a wedge.
const CONNECTION_IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Static flag for signal handler. The signal handler sets this to false.
/// Both the handler and the accept loop read/write this directly — no Mutex
/// needed (AtomicBool is async-signal-safe for store with Ordering::SeqCst).
static RUNNING: AtomicBool = AtomicBool::new(true);

/// Signal handler — extern "C", async-signal-safe: only touches an atomic.
extern "C" fn signal_handler(_sig: libc::c_int) {
    RUNNING.store(false, Ordering::SeqCst);
}

pub struct DaemonServer {
    pub socket_path: PathBuf,
    pub pid_path: PathBuf,
    session_key: Zeroizing<Vec<u8>>,
    vault: Arc<Mutex<Vault>>,
    config_dir: PathBuf,
    data_dir: PathBuf,
    trusted_callers: Option<TrustedCallersManifest>,
    start_time: Instant,
    idle_timeout: u64,
    fuse: fuse::SandboxFuse,
}

impl DaemonServer {
    /// Create a new DaemonServer.
    ///
    /// 1. Refuse to start if LD_PRELOAD / DYLD_INSERT_LIBRARIES is set
    /// 2. Clean stale socket/PID files (or error if daemon already running)
    /// 3. Generate 32-byte session key
    /// 4. Best-effort mlock on key bytes
    /// 5. Deny debugger attachment
    /// 6. Load trusted-callers.toml
    ///
    /// `require_sandbox` arms the sandbox fuse (`[daemon] require_sandbox`
    /// in the consumer's sealed protocol.toml); `project_root` anchors the
    /// `.claude` settings scopes the fuse reads.
    pub fn new(
        config_dir: PathBuf,
        data_dir: PathBuf,
        idle_timeout: u64,
        require_sandbox: bool,
        project_root: PathBuf,
    ) -> Result<Self, String> {
        // 1. Check for library injection
        if let Some(var) = platform::check_preload_env() {
            return Err(format!("refusing to start: {} is set in environment", var));
        }

        let socket_path = socket_path_for(&data_dir);
        let pid_path = data_dir.join("daemon.pid");

        // 2. Clean stale socket/PID files
        if pid_path.exists() {
            let pid_str = std::fs::read_to_string(&pid_path)
                .map_err(|e| format!("cannot read PID file: {}", e))?;
            let pid: i32 = pid_str
                .trim()
                .parse()
                .map_err(|e| format!("invalid PID in file: {}", e))?;

            // Check if process is alive: kill(pid, 0) returns 0 if alive
            let alive = unsafe { libc::kill(pid, 0) } == 0;
            if alive {
                return Err(format!("daemon already running (PID {})", pid));
            }

            // Stale files — remove them
            let _ = std::fs::remove_file(&pid_path);
            let _ = std::fs::remove_file(&socket_path);
        } else if socket_path.exists() {
            // PID file gone but socket remains — stale
            let _ = std::fs::remove_file(&socket_path);
        }

        // 3. Generate 32-byte session key
        let mut key_bytes = vec![0u8; 32];
        getrandom::getrandom(&mut key_bytes)
            .map_err(|e| format!("failed to generate session key: {}", e))?;
        let session_key = Zeroizing::new(key_bytes);

        // 4. Best-effort mlock on key bytes
        if let Err(e) = platform::try_mlock(session_key.as_ptr(), session_key.len()) {
            eprintln!("warning: mlock failed ({}), key may be swapped to disk", e);
        }

        // 5. Deny debugger attachment
        platform::deny_debug_attach();

        // 6. Load trusted-callers.toml. An *absent* file means caller auth
        // was never configured — connections are allowed (the development
        // default). A *present* file is enforced as written, so an empty
        // `[callers]` table denies everyone rather than allowing everyone:
        // a deployment that declares callers and lists none has said "no
        // one", not "anyone".
        let callers_path = config_dir.join("trusted-callers.toml");
        let trusted_callers = if callers_path.exists() {
            Some(
                TrustedCallersManifest::load(&callers_path)
                    .map_err(|e| format!("cannot load trusted-callers.toml: {}", e))?,
            )
        } else {
            None
        };

        let fuse = fuse::SandboxFuse {
            armed: require_sandbox,
            project_root,
            socket_path: socket_path.clone(),
        };

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
            fuse,
        })
    }

    /// Start the accept loop.
    ///
    /// 1. Bind UnixListener
    /// 2. Set socket permissions to 0600
    /// 3. Write PID file
    /// 4. Install SIGTERM/SIGINT handlers
    /// 5. Non-blocking accept loop
    /// 6. On exit, cleanup
    pub fn start(&self) -> Result<(), String> {
        // 1. Bind
        let listener = UnixListener::bind(&self.socket_path).map_err(|e| {
            format!(
                "cannot bind socket at {}: {}",
                self.socket_path.display(),
                e
            )
        })?;

        // 2. Set socket permissions to 0600 (owner read/write only)
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&self.socket_path, perms)
                .map_err(|e| format!("cannot set socket permissions: {}", e))?;
        }

        // 3. Write PID file
        let pid = std::process::id();
        std::fs::write(&self.pid_path, pid.to_string())
            .map_err(|e| format!("cannot write PID file: {}", e))?;

        // Report the fuse state once at startup. Informational only: under
        // the normal arming order the daemon starts *before* the sandbox
        // settings are written, so a tripped fuse here is expected — it is
        // re-evaluated per request and starts passing when the boundary
        // appears.
        if self.fuse.armed {
            match self.fuse.refusal() {
                None => eprintln!("daemon: sandbox fuse armed; boundary verified"),
                Some(_) => eprintln!(
                    "daemon: sandbox fuse armed; boundary not yet in place — privileged \
                     operations are refused until it is"
                ),
            }
        }

        // 4. Install signal handlers for SIGTERM and SIGINT
        RUNNING.store(true, Ordering::SeqCst);
        unsafe {
            libc::signal(
                libc::SIGTERM,
                signal_handler as *const () as libc::sighandler_t,
            );
            libc::signal(
                libc::SIGINT,
                signal_handler as *const () as libc::sighandler_t,
            );
        }

        // 5. Set listener to non-blocking for polling
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("cannot set non-blocking: {}", e))?;

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
                    let plugin_root = &self.config_dir;
                    handle_connection(
                        stream,
                        vault,
                        key,
                        start_time,
                        last_activity,
                        idle_timeout,
                        self.trusted_callers.as_ref(),
                        plugin_root,
                        &self.fuse,
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

        // 6. Cleanup
        self.cleanup();
        Ok(())
    }

    /// Remove socket and PID files.
    pub fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.socket_path);
        let _ = std::fs::remove_file(&self.pid_path);
    }

    /// Return a reference to the session key bytes (for tests or CLI key export).
    pub fn session_key(&self) -> &[u8] {
        &self.session_key
    }

    /// Return the config dir.
    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    /// Return the data dir.
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }
}

/// Handle a single client connection.
///
/// Reads newline-delimited JSON requests, dispatches each to `handle_request`,
/// and writes back JSON responses (one per line). Both I/O directions are
/// bounded by `CONNECTION_IO_TIMEOUT` so a silent client cannot wedge the
/// sequential accept loop.
///
/// Authenticates the caller via PID-based manifest check before processing.
/// Status requests are exempt (health checks). All other requests require
/// successful authentication, and — when the sandbox fuse is armed — a
/// passing boundary check, evaluated fresh per request.
#[allow(clippy::too_many_arguments)]
fn handle_connection(
    stream: UnixStream,
    vault: Arc<Mutex<Vault>>,
    session_key: Zeroizing<Vec<u8>>,
    start_time: Instant,
    last_activity: Instant,
    idle_timeout: u64,
    trusted_callers: Option<&auth::TrustedCallersManifest>,
    plugin_root: &Path,
    sandbox_fuse: &fuse::SandboxFuse,
) {
    // Bound both I/O directions before anything else: a silent or stalled
    // client must not hold the (single-threaded) daemon open indefinitely.
    // A timed-out read surfaces as an error in the line loop below, which
    // drops the connection and returns to the accept loop.
    if let Err(e) = stream.set_read_timeout(Some(CONNECTION_IO_TIMEOUT)) {
        eprintln!("cannot set read timeout: {}", e);
        return;
    }
    if let Err(e) = stream.set_write_timeout(Some(CONNECTION_IO_TIMEOUT)) {
        eprintln!("cannot set write timeout: {}", e);
        return;
    }

    // Authenticate before setting up reader/writer. `None` means no
    // trusted-callers.toml exists — caller auth was never configured, and
    // all connections are allowed (the development default). A present
    // manifest is enforced as written: an empty `[callers]` table denies
    // every caller rather than allowing every caller.
    let (authenticated, auth_reason) = match trusted_callers {
        None => (true, None),
        Some(manifest) => match auth::authenticate_peer(&stream, manifest, plugin_root) {
            Ok(()) => (true, None),
            Err(e) => {
                let reason = e.reason_code().to_string();
                eprintln!("auth: {} (reason: {})", e, reason);
                (false, Some(reason))
            }
        },
    };

    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot clone stream: {}", e);
            return;
        }
    };
    let reader = BufReader::new(reader_stream);
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // Connection closed or read error
        };

        if line.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Status) => {
                // Status is always allowed (health check).
                handle_request(
                    Request::Status,
                    &vault,
                    &session_key,
                    start_time,
                    last_activity,
                    idle_timeout,
                    plugin_root,
                )
            }
            Ok(req) => {
                // The fuse outranks caller identity: without the sandbox
                // boundary, "who is on the socket" is unanswerable anyway.
                if let Some(refusal) = sandbox_fuse.refusal() {
                    refusal
                } else if authenticated {
                    handle_request(
                        req,
                        &vault,
                        &session_key,
                        start_time,
                        last_activity,
                        idle_timeout,
                        plugin_root,
                    )
                } else {
                    let reason = auth_reason.as_deref().unwrap_or("pid_resolution_failed");
                    Response::err_with_reason("auth_failed", "caller not authenticated", reason)
                }
            }
            Err(e) => Response::err("parse_error", &format!("invalid request: {}", e)),
        };

        let resp_json = match serde_json::to_string(&response) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("cannot serialize response: {}", e);
                break;
            }
        };

        if writeln!(writer, "{}", resp_json).is_err() {
            break; // Write failed — connection closed
        }
    }
}

/// Dispatch a parsed request to the appropriate operation.
#[allow(clippy::too_many_arguments)]
fn handle_request(
    req: Request,
    vault: &Arc<Mutex<Vault>>,
    session_key: &[u8],
    start_time: Instant,
    last_activity: Instant,
    idle_timeout: u64,
    config_dir: &Path,
) -> Response {
    match req {
        Request::Sign { event_type, fields } => {
            let proof = compute_sign(session_key, &event_type, &fields);
            Response::ok_sign(&proof)
        }
        Request::VaultStore { name, data } => {
            if name.starts_with('_') {
                return Response::err("reserved", "vault names starting with '_' are reserved");
            }
            if let Some(err) = check_vault_policy(config_dir, &name, VaultAccess::Store) {
                return err;
            }
            let bytes = match base64::engine::general_purpose::STANDARD.decode(&data) {
                Ok(b) => b,
                Err(e) => {
                    return Response::err("decode_error", &format!("invalid base64: {}", e));
                }
            };
            match vault.lock() {
                Ok(mut v) => {
                    v.store(name, bytes);
                    Response::ok_empty()
                }
                Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
            }
        }
        Request::VaultRead { name } => {
            if name.starts_with('_') {
                return Response::err("reserved", "vault names starting with '_' are reserved");
            }
            if let Some(err) = check_vault_policy(config_dir, &name, VaultAccess::Read) {
                return err;
            }
            match vault.lock() {
                Ok(v) => match v.read(&name) {
                    Some(bytes) => {
                        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                        Response::ok_data(&encoded)
                    }
                    None => Response::err("not_found", &format!("no entry named '{}'", name)),
                },
                Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
            }
        }
        Request::VaultDelete { name } => {
            if name.starts_with('_') {
                return Response::err("reserved", "vault names starting with '_' are reserved");
            }
            if let Some(err) = check_vault_policy(config_dir, &name, VaultAccess::Delete) {
                return err;
            }
            match vault.lock() {
                Ok(mut v) => {
                    v.delete(&name);
                    Response::ok_empty()
                }
                Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
            }
        }
        Request::VaultList => match vault.lock() {
            Ok(v) => {
                let names: Vec<String> = v
                    .list()
                    .into_iter()
                    .filter(|s| !s.starts_with('_'))
                    .map(|s| s.to_string())
                    .collect();
                Response::ok_names(names)
            }
            Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
        },
        Request::Status => {
            let pid = std::process::id();
            let uptime = start_time.elapsed().as_secs();
            let idle_secs = last_activity.elapsed().as_secs();
            let (vault_entries, enforcement_active) = match vault.lock() {
                Ok(v) => {
                    let entries = v.list().into_iter().filter(|s| !s.starts_with('_')).count();
                    (entries, v.read("_enforcement").is_some())
                }
                Err(_) => (0, false),
            };
            Response::ok_status(
                pid,
                uptime,
                vault_entries,
                idle_secs,
                idle_timeout,
                enforcement_active,
            )
        }
        Request::Verify {
            event_type,
            fields,
            proof,
        } => {
            let expected = compute_sign(session_key, &event_type, &fields);
            if constant_time_eq(proof.as_bytes(), expected.as_bytes()) {
                Response::ok_verified()
            } else {
                Response::err("invalid_proof", "proof does not match")
            }
        }
        Request::EnforcementRead => match vault.lock() {
            Ok(v) => match v.read("_enforcement") {
                Some(bytes) => {
                    // The version is of the bytes as *stored*, computed before
                    // the overlay: it is the token a conditional mutation is
                    // checked against, and a transition that moves the ledger
                    // must not invalidate it (#49).
                    let version = enforcement::version_of(bytes);
                    // The stored blob's "state" field is only as fresh as the
                    // consumer's last successful write; transitions advance the
                    // ledger without touching the vault (holtz #57). Serve
                    // ledger truth at read time instead of the stored value.
                    let bytes = overlay_ledger_state(bytes, config_dir);
                    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                    Response::ok_data(&encoded).with_version(&version)
                }
                None => Response::err("not_found", "no enforcement state"),
            },
            Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
        },
        Request::EnforcementWrite {
            data,
            expect_version,
        } => handle_enforcement_write(vault, &data, expect_version.as_deref()),
        Request::EnforcementUpdate {
            patch,
            expect_version,
        } => handle_enforcement_patch(
            vault,
            &patch,
            expect_version.as_deref(),
            PatchMode::TopLevel,
        ),
        Request::EnforcementMerge {
            patch,
            expect_version,
        } => handle_enforcement_patch(
            vault,
            &patch,
            expect_version.as_deref(),
            PatchMode::Recursive,
        ),
        Request::RecordEvent { event_type, fields } => {
            handle_record_event(config_dir, &event_type, fields)
        }
    }
}

/// How a patch combines with the stored enforcement blob.
enum PatchMode {
    /// `enforcement_update`: a patch key replaces the stored value under that
    /// key, whatever its shape. The original op, semantics unchanged.
    TopLevel,
    /// `enforcement_merge`: RFC 7386 — objects on both sides recurse, `null`
    /// deletes, everything else replaces. The per-actor write (#49).
    Recursive,
}

/// Decode the base64 JSON object every enforcement mutation carries, or the
/// refusal to send back. `what` names the payload in the error message so the
/// caller learns which side was malformed. The refusal is boxed only because
/// `Response` is a wide envelope and an unboxed error variant that size makes
/// every success path carry it.
fn decode_enforcement_object(
    payload: &str,
    what: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, Box<Response>> {
    let bytes = match base64::engine::general_purpose::STANDARD.decode(payload) {
        Ok(b) => b,
        Err(e) => {
            return Err(Box::new(Response::err(
                "decode_error",
                &format!("invalid base64: {}", e),
            )))
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(serde_json::Value::Object(m)) => Ok(m),
        Ok(_) => Err(Box::new(Response::err(
            "invalid_data",
            &format!("{} must be a JSON object", what),
        ))),
        Err(e) => Err(Box::new(Response::err(
            "invalid_data",
            &format!("invalid JSON: {}", e),
        ))),
    }
}

/// Compare a conditional mutation's `expect_version` against what is stored.
///
/// Every caller runs this **holding the vault lock**, which is the whole point
/// (#49): a compare the client does around its own read-modify-write re-opens
/// the race it is trying to close, whereas a compare here means the loser is
/// told `version_conflict` and can re-read and retry safely. Returns the
/// refusal to send back, or `None` to proceed. An absent `expect_version` is
/// an unconditional mutation — the pre-#49 behavior, and still the default.
fn version_refusal(expected: Option<&str>, current: Option<&[u8]>) -> Option<Response> {
    let expected = expected?;
    match current {
        Some(bytes) => {
            let actual = enforcement::version_of(bytes);
            if actual == expected {
                None
            } else {
                Some(
                    Response::err(
                        "version_conflict",
                        "enforcement state has changed since it was read",
                    )
                    .with_version(&actual),
                )
            }
        }
        // Nothing stored: the caller is holding a token for a blob that no
        // longer exists (or never did). Refusing beats creating one and
        // reporting success for a compare that could not be made.
        None => Some(Response::err(
            "version_conflict",
            "no enforcement state to match against",
        )),
    }
}

/// Replace the whole enforcement blob, optionally conditional on its version.
fn handle_enforcement_write(
    vault: &Arc<Mutex<Vault>>,
    data: &str,
    expect_version: Option<&str>,
) -> Response {
    let mut obj = match decode_enforcement_object(data, "enforcement state") {
        Ok(o) => o,
        Err(refusal) => return *refusal,
    };
    let mut v = match vault.lock() {
        Ok(v) => v,
        Err(e) => return Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
    };
    let current = v.read("_enforcement").map(|b| b.to_vec());
    if let Some(refusal) = version_refusal(expect_version, current.as_deref()) {
        return refusal;
    }
    obj.insert(
        "last_refresh".to_string(),
        serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
    );
    let serialized = serde_json::to_vec(&obj).expect("re-serialization cannot fail");
    let version = enforcement::version_of(&serialized);
    v.store("_enforcement".to_string(), serialized);
    Response::ok_empty().with_version(&version)
}

/// Merge a patch into the stored enforcement blob — top-level for
/// `enforcement_update`, recursive for `enforcement_merge` — and hand back the
/// merged state plus its new version.
///
/// The read, the merge, and the store all happen under one lock, so the
/// result is the caller's patch applied to whatever the last writer left, not
/// to a snapshot the caller took earlier.
fn handle_enforcement_patch(
    vault: &Arc<Mutex<Vault>>,
    patch: &str,
    expect_version: Option<&str>,
    mode: PatchMode,
) -> Response {
    let patch_obj = match decode_enforcement_object(patch, "patch") {
        Ok(o) => o,
        Err(refusal) => return *refusal,
    };
    let mut v = match vault.lock() {
        Ok(v) => v,
        Err(e) => return Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
    };
    let current = match v.read("_enforcement") {
        Some(bytes) => bytes.to_vec(),
        None => return Response::err("not_found", "no enforcement state to update"),
    };
    if let Some(refusal) = version_refusal(expect_version, Some(&current)) {
        return refusal;
    }
    let mut state: serde_json::Map<String, serde_json::Value> =
        match serde_json::from_slice(&current) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => {
                return Response::err(
                    "internal_error",
                    "stored enforcement state is not a valid JSON object",
                );
            }
        };
    match mode {
        PatchMode::TopLevel => state.extend(patch_obj),
        PatchMode::Recursive => enforcement::merge_patch(&mut state, &patch_obj),
    }
    state.insert(
        "last_refresh".to_string(),
        serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
    );
    let serialized = serde_json::to_vec(&state).expect("re-serialization cannot fail");
    let encoded = base64::engine::general_purpose::STANDARD.encode(&serialized);
    let version = enforcement::version_of(&serialized);
    v.store("_enforcement".to_string(), serialized);
    Response::ok_data(&encoded).with_version(&version)
}

/// Append a consumer-declared event to the active ledger on behalf of an
/// already-authenticated socket peer.
///
/// This is the ledger-write analog of `enforcement_write`. The connecting
/// peer was authenticated against `trusted-callers.toml` in
/// `handle_connection` before this request was dispatched, so the peer's
/// identity *is* the authorization — no HMAC proof is required. This lets a
/// trusted hook (e.g. `primer.py`) record a `restricted` event directly,
/// bypassing the fragile `authed-event` CLI-courier path whose submitter
/// (the bare `sahjhan` binary) cannot be resolved to a manifest script.
///
/// The daemon holds no domain knowledge of specific events: the event type
/// and its field schema are validated against the *consumer's* events.toml.
/// Any declared event is accepted (restricted or not) — the trust boundary
/// is the authenticated peer, exactly as for `enforcement_write`.
///
/// Ledger + manifest are resolved cwd-relative, identical to the CLI and to
/// `derive_ledger_state`; the daemon runs with cwd = the consumer project
/// root, so this targets the same files the CLI would. `Ledger::append`
/// takes an exclusive file lock and re-reads the on-disk tail, so a
/// concurrent CLI append cannot corrupt the chain.
fn handle_record_event(
    config_dir: &Path,
    event_type: &str,
    fields: HashMap<String, String>,
) -> Response {
    let config = match ProtocolConfig::load(config_dir) {
        Ok(c) => c,
        Err(e) => return Response::err("config_error", &format!("cannot load config: {}", e)),
    };

    // Event must be declared by the consumer. The daemon never invents event
    // types — an unknown type is a consumer/caller bug, not a daemon concern.
    let event_config = match config.events.get(event_type) {
        Some(ec) => ec,
        None => {
            return Response::err(
                "unknown_event",
                &format!("event type '{}' is not defined in events.toml", event_type),
            );
        }
    };

    if let Err((_, msg)) = validate_event_fields(event_config, &fields, event_type) {
        return Response::err("invalid_field", &msg);
    }

    let targeting = LedgerTargeting {
        ledger_name: None,
        ledger_path: None,
    };
    let (ledger, _mode) = match open_targeted_ledger(&config, &targeting, config_dir) {
        Ok(lm) => lm,
        Err((_, msg)) => return Response::err("ledger_error", &msg),
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let mut manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((_, msg)) => return Response::err("manifest_error", &msg),
    };

    let mut machine = StateMachine::new(&config, ledger);
    let code = record_and_render(
        &config,
        config_dir,
        &mut machine,
        &mut manifest,
        &data_dir,
        event_type,
        fields,
        &targeting,
    );
    if code == EXIT_SUCCESS {
        let seq = machine
            .ledger()
            .entries()
            .last()
            .map(|e| e.seq)
            .unwrap_or(0);
        Response::ok_data(&seq.to_string())
    } else {
        Response::err(
            "record_failed",
            &format!("event recording failed (exit {})", code),
        )
    }
}

/// Override the enforcement blob's `state` field with the ledger-derived
/// current state.
///
/// The vault blob is consumer-owned and opaque except for this one key:
/// `state` must always reflect the ledger (the source of truth), because
/// transitions advance the ledger without any authenticated path back into
/// the vault (holtz #57). If the ledger cannot be resolved or fails chain
/// verification, or the blob is not a JSON object, the stored bytes are
/// served unchanged (fail-soft).
fn overlay_ledger_state(bytes: &[u8], config_dir: &Path) -> Vec<u8> {
    let Some(state) = derive_ledger_state(config_dir) else {
        return bytes.to_vec();
    };
    match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(serde_json::Value::Object(mut obj)) => {
            obj.insert("state".to_string(), serde_json::Value::String(state));
            serde_json::to_vec(&obj).unwrap_or_else(|_| bytes.to_vec())
        }
        _ => bytes.to_vec(),
    }
}

/// Resolve the active ledger the same way the CLI does (active-ledger
/// marker → registry → default), verify its hash chain, and derive the
/// current state from the last `state_transition` event.
///
/// Returns `None` on any failure so callers can fall back to stored data.
fn derive_ledger_state(config_dir: &Path) -> Option<String> {
    let config = ProtocolConfig::load(config_dir).ok()?;
    let targeting = LedgerTargeting {
        ledger_name: None,
        ledger_path: None,
    };
    let (path, _mode) = resolve_ledger_from_targeting(&config, &targeting).ok()?;
    let ledger = Ledger::open(&path).ok()?;
    ledger.verify().ok()?;
    Some(derive_current_state(&config, &ledger))
}

/// Enforce a vault key's declared state-based access policy (`vault.toml`).
///
/// Returns `Some(err)` when the op is forbidden in the ledger's current state,
/// `None` when permitted. `None` also covers the backward-compatible cases:
/// a key with no policy, or a policy with no whitelist for this op, is
/// unrestricted — so keys authored before `vault.toml` existed behave exactly
/// as before.
///
/// Fail-soft on config-load failure (returns `None`, i.e. allow): a config
/// that cannot load already breaks the daemon elsewhere, and this mirrors
/// `overlay_ledger_state`'s fail-soft handling. But a *policed* key whose
/// current state cannot be derived is denied — a state-gated secret must not
/// become reachable just because the ledger is momentarily unresolvable.
fn check_vault_policy(config_dir: &Path, name: &str, access: VaultAccess) -> Option<Response> {
    let config = ProtocolConfig::load(config_dir).ok()?;
    let policy = config.vault_policies.get(name)?;
    // No whitelist declared for this op → unrestricted.
    policy.states_for(access).as_ref()?;
    match derive_ledger_state(config_dir) {
        Some(state) if policy.permits(access, &state) => None,
        Some(state) => Some(Response::err_with_reason(
            "state_forbidden",
            &format!(
                "vault '{}' is not {} in state '{}'",
                name,
                access.adjective(),
                state
            ),
            "vault_state_policy",
        )),
        None => Some(Response::err_with_reason(
            "state_forbidden",
            &format!(
                "vault '{}' is {}-gated but the current state cannot be determined",
                name,
                access.adjective()
            ),
            "vault_state_policy",
        )),
    }
}

/// Constant-time byte comparison using the `subtle` crate.
///
/// Returns `true` only if both slices have identical length and contents.
/// Runs in time proportional to the slice length regardless of where
/// (or whether) the first mismatch occurs, preventing timing side-channels.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

/// Compute HMAC-SHA256 proof for signing requests.
///
/// Uses the same canonical payload format as `cli/authed_event.rs`.
fn compute_sign(session_key: &[u8], event_type: &str, fields: &HashMap<String, String>) -> String {
    let payload = build_canonical_payload(event_type, fields);
    let mut mac = HmacSha256::new_from_slice(session_key).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Build the canonical payload for HMAC computation.
///
/// Format: `event_type\0field1_name=field1_value\0field2_name=field2_value`
/// Fields sorted lexicographically by name. This is the same algorithm used
/// in `cli/authed_event.rs`.
pub fn build_canonical_payload(event_type: &str, fields: &HashMap<String, String>) -> String {
    let mut sorted_fields: Vec<(&str, &str)> = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    sorted_fields.sort_by_key(|(k, _)| *k);

    let mut payload = event_type.to_string();
    for (k, v) in &sorted_fields {
        payload.push('\0');
        payload.push_str(&format!("{}={}", k, v));
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test covers default, override, and empty-string cases sequentially:
    /// `SAHJHAN_DAEMON_SOCKET` is process-global state, so separate parallel
    /// test functions would race on it.
    #[test]
    fn socket_path_env_override() {
        let data_dir = Path::new("/some/data/dir");

        std::env::remove_var("SAHJHAN_DAEMON_SOCKET");
        assert_eq!(
            socket_path_for(data_dir),
            PathBuf::from("/some/data/dir/daemon.sock")
        );

        std::env::set_var("SAHJHAN_DAEMON_SOCKET", "/tmp/elsewhere/d.sock");
        assert_eq!(
            socket_path_for(data_dir),
            PathBuf::from("/tmp/elsewhere/d.sock")
        );

        // Empty string falls through to the default, matching the Python side.
        std::env::set_var("SAHJHAN_DAEMON_SOCKET", "");
        assert_eq!(
            socket_path_for(data_dir),
            PathBuf::from("/some/data/dir/daemon.sock")
        );

        std::env::remove_var("SAHJHAN_DAEMON_SOCKET");
    }
}
