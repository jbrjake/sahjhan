// src/daemon/fuse.rs
//
// The sandbox fuse: refuse privileged daemon operations unless the consumer
// project's Claude Code sandbox settings confirm the boundary is in place.
//
// The daemon provably cannot authenticate a same-user socket peer (a process
// can fork a connected fd into a genuine hook, or load code into one), so
// caller identity is not the boundary. The boundary is the Claude Code Bash
// sandbox: the agent's commands run inside it with unix sockets denied, and
// hooks run outside it. The fuse's job is to convert "hopefully sandboxed"
// into "provably configured-sandboxed, or nothing works": every privileged
// op re-checks the settings, so a daemon started before the sandbox was
// armed (the normal order — the arming hook starts the daemon first, then
// writes the settings) begins serving the moment the boundary appears, and
// stops the moment it goes away.
//
// What the fuse reads is *configuration*, not live kernel state — it cannot
// observe whether a given shell command actually ran confined. The gap is
// closed by requiring `failIfUnavailable: true`: with it, a sandbox that
// cannot initialize halts the session rather than silently running open, so
// "settings demand it" implies "it is on, or nothing is running."
//
// Scopes consulted, in precedence order (first scope defining a key wins,
// matching Claude Code's merged read): the project's settings.local.json,
// the project's settings.json, the user's ~/.claude/settings.json. Managed
// (org-deployed) settings and `--settings` launch files are NOT consulted:
// both are operator-controlled surfaces, and the fuse defends against the
// sandboxed agent — which can write none of the consulted files while
// confined (they are sandbox-protected paths).
//
// Platform caveat: on Linux the sandbox blocks unix sockets only when the
// optional seccomp filter (`@anthropic-ai/sandbox-runtime`) is installed;
// there is no way to verify that from here. macOS Seatbelt denies sockets
// by default whenever the sandbox is on.
//
// ## Index
// - SandboxFuse            — armed flag + project_root + socket_path, held by DaemonServer
// - SandboxFuse::refusal   — evaluate the boundary; Some(refusal Response) when tripped
// - [fuse-scopes]          scope_paths()  — settings files consulted, precedence order
// - [fuse-evaluate]        evaluate()     — the checks themselves
// - [fuse-entry-covers]    entry_covers() — does an allowlist entry reach the socket

use std::path::{Path, PathBuf};

use super::protocol::Response;

/// The fuse the daemon consults before serving any privileged operation.
///
/// `armed` mirrors `[daemon] require_sandbox` from the consumer's sealed
/// protocol.toml. When false, `refusal` never fires and the daemon behaves
/// as a plain secret-holder.
pub struct SandboxFuse {
    pub armed: bool,
    pub project_root: PathBuf,
    pub socket_path: PathBuf,
}

/// Why the fuse refused. `reason` is a stable machine-readable code for
/// consumers (hooks distinguish "boundary missing" from other errors and
/// fail closed); `message` names the offending file or key.
struct FuseTrip {
    reason: &'static str,
    message: String,
}

fn trip(reason: &'static str, message: String) -> FuseTrip {
    FuseTrip { reason, message }
}

impl SandboxFuse {
    /// Evaluate the boundary. `None` means serve the request; `Some` is the
    /// refusal to send back. Cheap enough to run per request: three small
    /// JSON files at most, and re-reading is the point — the settings may
    /// legitimately change under a running daemon in either direction.
    pub fn refusal(&self) -> Option<Response> {
        if !self.armed {
            return None;
        }
        match evaluate(
            &scope_paths(&self.project_root),
            &self.socket_path,
            &self.project_root,
        ) {
            Ok(()) => None,
            Err(t) => Some(Response::err_with_reason(
                "sandbox_required",
                &t.message,
                t.reason,
            )),
        }
    }
}

// [fuse-scopes]
/// The settings files consulted, highest precedence first. A missing file is
/// simply skipped; only present-but-unreadable files refuse.
fn scope_paths(project_root: &Path) -> Vec<PathBuf> {
    let mut scopes = vec![
        project_root.join(".claude/settings.local.json"),
        project_root.join(".claude/settings.json"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            scopes.push(PathBuf::from(home).join(".claude/settings.json"));
        }
    }
    scopes
}

// [fuse-evaluate]
/// The boundary checks. Effective values follow scope precedence; the
/// deny-direction scans (socket allowances, excluded commands) consider
/// *every* scope, because an allowance anywhere is a hole regardless of
/// which scope would win a merge.
fn evaluate(scopes: &[PathBuf], socket_path: &Path, project_root: &Path) -> Result<(), FuseTrip> {
    let mut docs: Vec<(&Path, serde_json::Value)> = Vec::new();
    for path in scopes {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => docs.push((path.as_path(), v)),
                Err(e) => {
                    return Err(trip(
                        "settings_unreadable",
                        format!("cannot parse {}: {}", path.display(), e),
                    ));
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(trip(
                    "settings_unreadable",
                    format!("cannot read {}: {}", path.display(), e),
                ));
            }
        }
    }

    let effective = |key: &str| -> Option<&serde_json::Value> {
        docs.iter()
            .find_map(|(_, doc)| doc.get("sandbox").and_then(|s| s.get(key)))
    };

    if effective("enabled").and_then(|v| v.as_bool()) != Some(true) {
        return Err(trip(
            "sandbox_not_enabled",
            "sandbox.enabled is not true in any consulted settings scope — the session is not \
             confined"
                .to_string(),
        ));
    }
    if effective("allowUnsandboxedCommands").and_then(|v| v.as_bool()) != Some(false) {
        return Err(trip(
            "unsandboxed_commands_allowed",
            "sandbox.allowUnsandboxedCommands must be explicitly false — a denied command must \
             not be retryable outside the sandbox"
                .to_string(),
        ));
    }
    if effective("failIfUnavailable").and_then(|v| v.as_bool()) != Some(true) {
        return Err(trip(
            "sandbox_fail_open",
            "sandbox.failIfUnavailable must be explicitly true — a sandbox that cannot \
             initialize must halt the session, not run open"
                .to_string(),
        ));
    }

    for (path, doc) in &docs {
        let Some(sandbox) = doc.get("sandbox") else {
            continue;
        };
        let network = sandbox.get("network");
        if network
            .and_then(|n| n.get("allowAllUnixSockets"))
            .and_then(|v| v.as_bool())
            == Some(true)
        {
            return Err(trip(
                "socket_allowlisted",
                format!(
                    "sandbox.network.allowAllUnixSockets is true in {} — the daemon socket is \
                     reachable from inside the sandbox",
                    path.display()
                ),
            ));
        }
        if let Some(entries) = network
            .and_then(|n| n.get("allowUnixSockets"))
            .and_then(|v| v.as_array())
        {
            for entry in entries.iter().filter_map(|e| e.as_str()) {
                if entry_covers(entry, socket_path) {
                    return Err(trip(
                        "socket_allowlisted",
                        format!(
                            "sandbox.network.allowUnixSockets entry '{}' in {} reaches the \
                             daemon socket {}",
                            entry,
                            path.display(),
                            socket_path.display()
                        ),
                    ));
                }
            }
        }
        if let Some(cmds) = sandbox.get("excludedCommands").and_then(|v| v.as_array()) {
            if !cmds.is_empty() {
                return Err(trip(
                    "excluded_commands_present",
                    format!(
                        "sandbox.excludedCommands is non-empty in {} — an excluded command runs \
                         unsandboxed, and e.g. a repo-local git hook lets the agent ride one to \
                         the socket",
                        path.display()
                    ),
                ));
            }
        }
    }

    // The socket must live outside the project tree: inside it, the path sits
    // in agent-writable territory (deletable, squattable between daemon
    // restarts), and the arming flow is expected to have relocated it via
    // SAHJHAN_DAEMON_SOCKET.
    if canon(socket_path).starts_with(canon(project_root)) {
        return Err(trip(
            "socket_inside_project",
            format!(
                "daemon socket {} is inside the project root {} — start the daemon with \
                 SAHJHAN_DAEMON_SOCKET pointing outside it",
                socket_path.display(),
                project_root.display()
            ),
        ));
    }

    Ok(())
}

// [fuse-entry-covers]
/// Does an `allowUnixSockets` entry reach the daemon socket? Exact path or
/// directory-prefix match; an entry carrying a glob is treated as covering
/// (conservative — the fuse must not guess what a pattern won't match).
fn entry_covers(entry: &str, socket_path: &Path) -> bool {
    if entry.contains('*') {
        return true;
    }
    let entry = Path::new(entry);
    socket_path == entry || socket_path.starts_with(entry)
}

/// Canonicalize for containment comparison, tolerating paths that do not
/// exist yet: the nearest existing ancestor is canonicalized and the missing
/// tail re-appended. Without this, one side resolving `/var` →
/// `/private/var` while the other stays literal would defeat the
/// containment test on macOS.
fn canon(p: &Path) -> PathBuf {
    let mut base = p.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match base.canonicalize() {
            Ok(c) => {
                let mut out = c;
                for comp in tail.iter().rev() {
                    out.push(comp);
                }
                return out;
            }
            Err(_) => match (base.parent(), base.file_name()) {
                (Some(parent), Some(name)) => {
                    tail.push(name.to_os_string());
                    base = parent.to_path_buf();
                }
                _ => return p.to_path_buf(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_local(root: &Path, json: &str) {
        let dir = root.join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.local.json"), json).unwrap();
    }

    /// A settings body that satisfies every effective-value check.
    const GOOD: &str =
        r#"{"sandbox":{"enabled":true,"allowUnsandboxedCommands":false,"failIfUnavailable":true}}"#;

    /// Scopes list for a test project: just the project-local file (the unit
    /// tests never consult a real HOME).
    fn scopes(root: &Path) -> Vec<PathBuf> {
        vec![root.join(".claude/settings.local.json")]
    }

    fn reason(result: Result<(), FuseTrip>) -> &'static str {
        result.err().map(|t| t.reason).unwrap_or("ok")
    }

    #[test]
    fn passes_with_full_config_and_external_socket() {
        let dir = tempfile::tempdir().unwrap();
        write_local(dir.path(), GOOD);
        let sock = Path::new("/tmp/elsewhere/d.sock");
        assert!(evaluate(&scopes(dir.path()), sock, dir.path()).is_ok());
    }

    #[test]
    fn refuses_with_no_settings_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let sock = Path::new("/tmp/elsewhere/d.sock");
        assert_eq!(
            reason(evaluate(&scopes(dir.path()), sock, dir.path())),
            "sandbox_not_enabled"
        );
    }

    #[test]
    fn refuses_without_explicit_unsandboxed_false() {
        let dir = tempfile::tempdir().unwrap();
        write_local(dir.path(), r#"{"sandbox":{"enabled":true}}"#);
        let sock = Path::new("/tmp/elsewhere/d.sock");
        assert_eq!(
            reason(evaluate(&scopes(dir.path()), sock, dir.path())),
            "unsandboxed_commands_allowed"
        );
    }

    #[test]
    fn refuses_without_fail_if_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        write_local(
            dir.path(),
            r#"{"sandbox":{"enabled":true,"allowUnsandboxedCommands":false}}"#,
        );
        let sock = Path::new("/tmp/elsewhere/d.sock");
        assert_eq!(
            reason(evaluate(&scopes(dir.path()), sock, dir.path())),
            "sandbox_fail_open"
        );
    }

    #[test]
    fn refuses_when_socket_is_allowlisted() {
        let dir = tempfile::tempdir().unwrap();
        write_local(
            dir.path(),
            r#"{"sandbox":{"enabled":true,"allowUnsandboxedCommands":false,"failIfUnavailable":true,"network":{"allowUnixSockets":["/tmp/elsewhere/d.sock"]}}}"#,
        );
        let sock = Path::new("/tmp/elsewhere/d.sock");
        assert_eq!(
            reason(evaluate(&scopes(dir.path()), sock, dir.path())),
            "socket_allowlisted"
        );
    }

    #[test]
    fn refuses_when_socket_dir_is_allowlisted() {
        let dir = tempfile::tempdir().unwrap();
        write_local(
            dir.path(),
            r#"{"sandbox":{"enabled":true,"allowUnsandboxedCommands":false,"failIfUnavailable":true,"network":{"allowUnixSockets":["/tmp/elsewhere"]}}}"#,
        );
        let sock = Path::new("/tmp/elsewhere/d.sock");
        assert_eq!(
            reason(evaluate(&scopes(dir.path()), sock, dir.path())),
            "socket_allowlisted"
        );
    }

    #[test]
    fn glob_allowlist_entry_is_treated_as_covering() {
        let dir = tempfile::tempdir().unwrap();
        write_local(
            dir.path(),
            r#"{"sandbox":{"enabled":true,"allowUnsandboxedCommands":false,"failIfUnavailable":true,"network":{"allowUnixSockets":["/tmp/**"]}}}"#,
        );
        let sock = Path::new("/somewhere/else/d.sock");
        assert_eq!(
            reason(evaluate(&scopes(dir.path()), sock, dir.path())),
            "socket_allowlisted"
        );
    }

    #[test]
    fn refuses_with_excluded_commands() {
        let dir = tempfile::tempdir().unwrap();
        write_local(
            dir.path(),
            r#"{"sandbox":{"enabled":true,"allowUnsandboxedCommands":false,"failIfUnavailable":true,"excludedCommands":["git"]}}"#,
        );
        let sock = Path::new("/tmp/elsewhere/d.sock");
        assert_eq!(
            reason(evaluate(&scopes(dir.path()), sock, dir.path())),
            "excluded_commands_present"
        );
    }

    #[test]
    fn refuses_socket_inside_project_root() {
        let dir = tempfile::tempdir().unwrap();
        write_local(dir.path(), GOOD);
        let sock = dir.path().join("data/daemon.sock");
        assert_eq!(
            reason(evaluate(&scopes(dir.path()), &sock, dir.path())),
            "socket_inside_project"
        );
    }

    #[test]
    fn refuses_unparseable_settings() {
        let dir = tempfile::tempdir().unwrap();
        write_local(dir.path(), "{not json");
        let sock = Path::new("/tmp/elsewhere/d.sock");
        assert_eq!(
            reason(evaluate(&scopes(dir.path()), sock, dir.path())),
            "settings_unreadable"
        );
    }

    #[test]
    fn higher_scope_wins_for_effective_values() {
        let dir = tempfile::tempdir().unwrap();
        // Local disables; project enables. Local wins → refuse.
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("settings.local.json"),
            r#"{"sandbox":{"enabled":false}}"#,
        )
        .unwrap();
        std::fs::write(claude.join("settings.json"), GOOD).unwrap();
        let both = vec![
            claude.join("settings.local.json"),
            claude.join("settings.json"),
        ];
        let sock = Path::new("/tmp/elsewhere/d.sock");
        assert_eq!(
            reason(evaluate(&both, sock, dir.path())),
            "sandbox_not_enabled"
        );
    }

    #[test]
    fn deny_scan_considers_every_scope() {
        let dir = tempfile::tempdir().unwrap();
        // Local is fully confining, but a lower-precedence scope allowlists
        // the socket — the allowance is a hole regardless of merge order.
        let claude = dir.path().join(".claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("settings.local.json"), GOOD).unwrap();
        std::fs::write(
            claude.join("settings.json"),
            r#"{"sandbox":{"network":{"allowUnixSockets":["/tmp/elsewhere/d.sock"]}}}"#,
        )
        .unwrap();
        let both = vec![
            claude.join("settings.local.json"),
            claude.join("settings.json"),
        ];
        let sock = Path::new("/tmp/elsewhere/d.sock");
        assert_eq!(
            reason(evaluate(&both, sock, dir.path())),
            "socket_allowlisted"
        );
    }

    #[test]
    fn disarmed_fuse_never_refuses() {
        let fuse = SandboxFuse {
            armed: false,
            project_root: PathBuf::from("/nonexistent"),
            socket_path: PathBuf::from("/nonexistent/daemon.sock"),
        };
        assert!(fuse.refusal().is_none());
    }
}
