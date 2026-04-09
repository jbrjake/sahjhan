// src/daemon/mod.rs
//
// Daemon mode: holds secrets in process memory, serves signing and vault
// operations over a Unix domain socket.
//
// ## Index
// - DaemonServer              -- main server struct
// - DaemonServer::new         -- construct and initialize (key gen, preload check, stale cleanup, idle timeout)
// - DaemonServer::start       -- bind socket, accept loop, signal handling
// - DaemonServer::cleanup     -- remove socket and PID files
// - handle_connection         -- read JSON lines from a stream, dispatch, respond
// - handle_request            -- match Request variant to operation; enforcement_read/write/update ops; _-prefixed vault namespace guard
// - compute_sign              -- HMAC-SHA256 signing (same algorithm as authed_event.rs)
// - build_canonical_payload   -- canonical HMAC payload from event_type + fields
// - mod platform              -- OS-specific APIs
// - mod vault                 -- in-memory secret store
// - mod protocol              -- wire protocol types
// - mod auth                  -- caller authentication

pub mod auth;
pub mod platform;
pub mod protocol;
pub mod vault;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

use self::auth::TrustedCallersManifest;
use self::protocol::{Request, Response};
use self::vault::Vault;

type HmacSha256 = Hmac<Sha256>;

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
    #[allow(dead_code)]
    trusted_callers: TrustedCallersManifest,
    start_time: Instant,
    idle_timeout: u64,
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
    pub fn new(config_dir: PathBuf, data_dir: PathBuf, idle_timeout: u64) -> Result<Self, String> {
        // 1. Check for library injection
        if let Some(var) = platform::check_preload_env() {
            return Err(format!("refusing to start: {} is set in environment", var));
        }

        let socket_path = data_dir.join("daemon.sock");
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

        // 6. Load trusted-callers.toml
        let callers_path = config_dir.join("trusted-callers.toml");
        let trusted_callers = if callers_path.exists() {
            TrustedCallersManifest::load(&callers_path)
                .map_err(|e| format!("cannot load trusted-callers.toml: {}", e))?
        } else {
            // No manifest — empty callers (all connections allowed)
            TrustedCallersManifest {
                callers: HashMap::new(),
            }
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
/// and writes back JSON responses (one per line).
///
/// Authenticates the caller via PID-based manifest check before processing.
/// Status requests are exempt (health checks). All other requests require
/// successful authentication.
#[allow(clippy::too_many_arguments)]
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
    // Authenticate before setting up reader/writer.
    // If no callers are configured, skip auth (allow all). This lets the
    // daemon operate without caller restrictions when trusted-callers.toml
    // has an empty [callers] table, which is the default for development
    // and testing.
    let (authenticated, auth_reason) = if trusted_callers.callers.is_empty() {
        (true, None)
    } else {
        match auth::authenticate_peer(&stream, trusted_callers, plugin_root) {
            Ok(()) => (true, None),
            Err(e) => {
                let reason = e.reason_code().to_string();
                eprintln!("auth: {} (reason: {})", e, reason);
                (false, Some(reason))
            }
        }
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
                )
            }
            Ok(req) => {
                if authenticated {
                    handle_request(
                        req,
                        &vault,
                        &session_key,
                        start_time,
                        last_activity,
                        idle_timeout,
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
fn handle_request(
    req: Request,
    vault: &Arc<Mutex<Vault>>,
    session_key: &[u8],
    start_time: Instant,
    last_activity: Instant,
    idle_timeout: u64,
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
            if proof == expected {
                Response::ok_verified()
            } else {
                Response::err("invalid_proof", "proof does not match")
            }
        }
        Request::EnforcementRead => match vault.lock() {
            Ok(v) => match v.read("_enforcement") {
                Some(bytes) => {
                    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                    Response::ok_data(&encoded)
                }
                None => Response::err("not_found", "no enforcement state"),
            },
            Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
        },
        Request::EnforcementWrite { data } => {
            let bytes = match base64::engine::general_purpose::STANDARD.decode(&data) {
                Ok(b) => b,
                Err(e) => {
                    return Response::err("decode_error", &format!("invalid base64: {}", e));
                }
            };
            let mut obj: serde_json::Map<String, serde_json::Value> =
                match serde_json::from_slice(&bytes) {
                    Ok(serde_json::Value::Object(m)) => m,
                    Ok(_) => {
                        return Response::err(
                            "invalid_data",
                            "enforcement state must be a JSON object",
                        );
                    }
                    Err(e) => {
                        return Response::err("invalid_data", &format!("invalid JSON: {}", e));
                    }
                };
            obj.insert(
                "last_refresh".to_string(),
                serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
            );
            let serialized = serde_json::to_vec(&obj).expect("re-serialization cannot fail");
            match vault.lock() {
                Ok(mut v) => {
                    v.store("_enforcement".to_string(), serialized);
                    Response::ok_empty()
                }
                Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
            }
        }
        Request::EnforcementUpdate { patch } => {
            let patch_bytes = match base64::engine::general_purpose::STANDARD.decode(&patch) {
                Ok(b) => b,
                Err(e) => {
                    return Response::err("decode_error", &format!("invalid base64: {}", e));
                }
            };
            let patch_obj: serde_json::Map<String, serde_json::Value> =
                match serde_json::from_slice(&patch_bytes) {
                    Ok(serde_json::Value::Object(m)) => m,
                    Ok(_) => {
                        return Response::err("invalid_data", "patch must be a JSON object");
                    }
                    Err(e) => {
                        return Response::err("invalid_data", &format!("invalid JSON: {}", e));
                    }
                };
            match vault.lock() {
                Ok(mut v) => {
                    let current = match v.read("_enforcement") {
                        Some(bytes) => bytes.to_vec(),
                        None => {
                            return Response::err("not_found", "no enforcement state to update");
                        }
                    };
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
                    state.extend(patch_obj);
                    state.insert(
                        "last_refresh".to_string(),
                        serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
                    );
                    let serialized =
                        serde_json::to_vec(&state).expect("re-serialization cannot fail");
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&serialized);
                    v.store("_enforcement".to_string(), serialized);
                    Response::ok_data(&encoded)
                }
                Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
            }
        }
    }
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
