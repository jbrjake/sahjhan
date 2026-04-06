// src/cli/daemon_cmd.rs
//
// Daemon process management CLI commands.
//
// ## Index
// - [cmd-daemon-start]          cmd_daemon_start()        — start daemon in foreground
// - [cmd-daemon-stop]           cmd_daemon_stop()         — stop running daemon
// - [cmd-daemon-status]         cmd_daemon_status()       — query daemon status
// - [resolve-socket-path]       resolve_socket_path()     — resolve daemon socket path
// - [connect-and-request]       connect_and_request()     — send JSON request to daemon socket

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use crate::daemon::DaemonServer;

use super::commands::{
    load_config, resolve_config_dir, resolve_data_dir, EXIT_CONFIG_ERROR, EXIT_SUCCESS,
};

// [cmd-daemon-start]
pub fn cmd_daemon_start(config_dir: &str) -> i32 {
    let config_dir_abs = resolve_config_dir(config_dir);
    let config = match load_config(&config_dir_abs) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };
    let data_dir_abs = resolve_data_dir(&config.paths.data_dir);

    let server = match DaemonServer::new(config_dir_abs, data_dir_abs, 0) {
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

// [cmd-daemon-stop]
pub fn cmd_daemon_stop(config_dir: &str) -> i32 {
    let config_dir_abs = resolve_config_dir(config_dir);
    let config = match load_config(&config_dir_abs) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };
    let data_dir_abs = resolve_data_dir(&config.paths.data_dir);

    let pid_path = data_dir_abs.join("daemon.pid");
    let socket_path = data_dir_abs.join("daemon.sock");

    if !pid_path.exists() {
        eprintln!("daemon: no PID file found — daemon is not running");
        return EXIT_CONFIG_ERROR;
    }

    let pid_str = match std::fs::read_to_string(&pid_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("daemon: cannot read PID file: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let pid: i32 = match pid_str.trim().parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("daemon: invalid PID in file: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    // Send SIGTERM
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }

    // Wait up to 5 seconds for process to exit
    let mut stopped = false;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if !alive {
            stopped = true;
            break;
        }
    }

    if !stopped {
        // Force kill
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }

    // Clean up PID and socket files
    let _ = std::fs::remove_file(&pid_path);
    let _ = std::fs::remove_file(&socket_path);

    EXIT_SUCCESS
}

// [cmd-daemon-status]
pub fn cmd_daemon_status(config_dir: &str) -> i32 {
    let socket_path = match resolve_socket_path(config_dir) {
        Ok(p) => p,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    match connect_and_request(&socket_path, r#"{"op":"status"}"#) {
        Ok(response) => {
            println!("{}", response);
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("daemon: {}", e);
            EXIT_CONFIG_ERROR
        }
    }
}

// [resolve-socket-path]
/// Resolve the daemon Unix socket path from config.
///
/// Returns an error if the socket file does not exist (daemon not running).
pub(crate) fn resolve_socket_path(config_dir: &str) -> Result<PathBuf, (i32, String)> {
    let config_dir_abs = resolve_config_dir(config_dir);
    let config = load_config(&config_dir_abs)?;
    let data_dir_abs = resolve_data_dir(&config.paths.data_dir);
    let socket_path = data_dir_abs.join("daemon.sock");

    if !socket_path.exists() {
        return Err((
            EXIT_CONFIG_ERROR,
            "sahjhan daemon is not running. Start it with `sahjhan daemon start`.".to_string(),
        ));
    }

    Ok(socket_path)
}

// [connect-and-request]
/// Connect to the daemon socket, send a JSON request, and return the response.
pub(crate) fn connect_and_request(
    socket_path: &Path,
    request_json: &str,
) -> Result<String, String> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("cannot connect to daemon socket: {}", e))?;

    // Write request + newline
    writeln!(stream, "{}", request_json)
        .map_err(|e| format!("cannot write to daemon socket: {}", e))?;

    // Read response line
    let reader = BufReader::new(&stream);
    let mut lines = reader.lines();
    match lines.next() {
        Some(Ok(line)) => Ok(line.trim().to_string()),
        Some(Err(e)) => Err(format!("cannot read from daemon socket: {}", e)),
        None => Err("daemon closed connection without responding".to_string()),
    }
}
