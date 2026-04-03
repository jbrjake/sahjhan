// src/daemon/platform.rs
//
// OS-specific APIs for daemon caller verification and process hardening.
// All platform-conditional code lives here; no other module should use
// `#[cfg(target_os)]` or call libc directly.
//
// ## Index
// - get_peer_pid              -- [get-peer-pid]       extract connecting PID from Unix socket
// - get_exe_path              -- [get-exe-path]       resolve PID to executable path
// - get_cmdline               -- [get-cmdline]        read process command-line arguments
// - get_parent_pid            -- [get-parent-pid]     look up parent PID
// - deny_debug_attach         -- [deny-debug-attach]  prevent debugger attachment
// - try_mlock                 -- [try-mlock]          best-effort memory locking
// - check_preload_env         -- [check-preload-env]  detect injected shared libraries

use std::io;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// [get-peer-pid] — Extract connecting PID from a Unix domain socket.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub fn get_peer_pid<S: AsRawFd>(socket: &S) -> io::Result<u32> {
    let fd = socket.as_raw_fd();
    let mut pid: libc::pid_t = 0;
    let mut len: libc::socklen_t = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;

    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            &mut pid as *mut libc::pid_t as *mut libc::c_void,
            &mut len,
        )
    };

    if ret != 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(pid as u32)
}

#[cfg(target_os = "linux")]
pub fn get_peer_pid<S: AsRawFd>(socket: &S) -> io::Result<u32> {
    let fd = socket.as_raw_fd();
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len: libc::socklen_t = std::mem::size_of::<libc::ucred>() as libc::socklen_t;

    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };

    if ret != 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(cred.pid as u32)
}

// ---------------------------------------------------------------------------
// [get-exe-path] — Resolve a PID to its executable path.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub fn get_exe_path(pid: u32) -> io::Result<PathBuf> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];

    let ret = unsafe {
        libc::proc_pidpath(
            pid as libc::c_int,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        )
    };

    if ret <= 0 {
        return Err(io::Error::last_os_error());
    }

    // ret is the length of the path (not including null terminator)
    let path_str = std::str::from_utf8(&buf[..ret as usize])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(PathBuf::from(path_str))
}

#[cfg(target_os = "linux")]
pub fn get_exe_path(pid: u32) -> io::Result<PathBuf> {
    std::fs::read_link(format!("/proc/{}/exe", pid))
}

// ---------------------------------------------------------------------------
// [get-cmdline] — Read the command-line arguments for a process.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub fn get_cmdline(pid: u32) -> io::Result<Vec<String>> {
    // Use sysctl with CTL_KERN, KERN_PROCARGS2 to get the process arguments.
    // Format: 4-byte argc (i32), then exec_path (null-terminated), then
    // padding nulls, then argc argument strings (each null-terminated).
    let mut mib: [libc::c_int; 3] = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];

    // First call: get buffer size
    let mut size: libc::size_t = 0;
    let ret = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return Err(io::Error::last_os_error());
    }

    // Second call: get actual data
    let mut buf = vec![0u8; size];
    let ret = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return Err(io::Error::last_os_error());
    }

    buf.truncate(size);

    if buf.len() < 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "KERN_PROCARGS2 buffer too small",
        ));
    }

    // First 4 bytes: argc as a little-endian i32
    let argc = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let mut pos = 4;

    // Skip the exec_path (null-terminated string)
    while pos < buf.len() && buf[pos] != 0 {
        pos += 1;
    }

    // Skip past the null terminator and any padding nulls
    while pos < buf.len() && buf[pos] == 0 {
        pos += 1;
    }

    // Now read argc null-terminated argument strings
    let mut args = Vec::with_capacity(argc);
    for _ in 0..argc {
        if pos >= buf.len() {
            break;
        }
        let start = pos;
        while pos < buf.len() && buf[pos] != 0 {
            pos += 1;
        }
        if let Ok(s) = std::str::from_utf8(&buf[start..pos]) {
            args.push(s.to_string());
        }
        // Skip the null terminator
        if pos < buf.len() {
            pos += 1;
        }
    }

    Ok(args)
}

#[cfg(target_os = "linux")]
pub fn get_cmdline(pid: u32) -> io::Result<Vec<String>> {
    let data = std::fs::read(format!("/proc/{}/cmdline", pid))?;
    let args = data
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    Ok(args)
}

// ---------------------------------------------------------------------------
// [get-parent-pid] — Look up the parent PID of a process.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub fn get_parent_pid(pid: u32) -> io::Result<u32> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;

    let ret = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut libc::proc_bsdinfo as *mut libc::c_void,
            size,
        )
    };

    if ret <= 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(info.pbi_ppid)
}

#[cfg(target_os = "linux")]
pub fn get_parent_pid(pid: u32) -> io::Result<u32> {
    let status = std::fs::read_to_string(format!("/proc/{}/status", pid))?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("PPid:") {
            let ppid: u32 = rest.trim().parse().map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("bad PPid: {}", e))
            })?;
            return Ok(ppid);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "PPid not found in /proc/status",
    ))
}

// ---------------------------------------------------------------------------
// [deny-debug-attach] — Prevent debugger attachment to this process.
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
pub fn deny_debug_attach() {
    unsafe {
        libc::ptrace(libc::PT_DENY_ATTACH, 0, std::ptr::null_mut(), 0);
    }
}

#[cfg(target_os = "linux")]
pub fn deny_debug_attach() {
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0);
    }
}

// ---------------------------------------------------------------------------
// [try-mlock] — Best-effort memory page locking (prevent swap).
// ---------------------------------------------------------------------------

pub fn try_mlock(ptr: *const u8, len: usize) -> io::Result<()> {
    let ret = unsafe { libc::mlock(ptr as *const libc::c_void, len) };
    if ret != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// [check-preload-env] — Detect injected shared libraries via environment.
// ---------------------------------------------------------------------------

pub fn check_preload_env() -> Option<&'static str> {
    if std::env::var_os("LD_PRELOAD").is_some() {
        return Some("LD_PRELOAD");
    }
    if std::env::var_os("DYLD_INSERT_LIBRARIES").is_some() {
        return Some("DYLD_INSERT_LIBRARIES");
    }
    None
}

// ---------------------------------------------------------------------------
// Compile-time guard: unsupported platforms get a clear error.
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("daemon::platform only supports macOS and Linux");
