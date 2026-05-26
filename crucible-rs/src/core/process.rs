// src/core/process.rs — Process lifecycle: double-fork, stop, scan /proc.
// Uses only libc directly for fd ops to avoid nix feature issues.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

const REAP_DEADLINE: Duration = Duration::from_secs(5);
const REAP_POLL:     Duration = Duration::from_millis(200);

// ── Double-fork detach ────────────────────────────────────────────────────

pub fn detached_fork(
    cmd: &[String],
    env: &std::collections::HashMap<String, String>,
    cwd: &str,
    log_path: &Path,
) -> u32 {
    use nix::unistd::{fork, ForkResult, setsid, pipe, close, read, write, dup2, execvpe};

    let (r, w) = pipe().expect("pipe");

    match unsafe { fork() }.expect("fork") {
        ForkResult::Parent { child } => {
            let _ = close(w);
            let mut buf = [0u8; 32];
            let n = read(r, &mut buf).unwrap_or(0);
            let _ = close(r);
            let _ = nix::sys::wait::waitpid(child, None);
            String::from_utf8_lossy(&buf[..n]).trim().parse::<u32>().unwrap_or(0)
        }
        ForkResult::Child => {
            let _ = close(r);
            let _ = setsid();

            match unsafe { fork() }.expect("fork2") {
                ForkResult::Parent { child } => {
                    let pid_s = child.as_raw().to_string();
                    let _ = write(w, pid_s.as_bytes());
                    let _ = close(w);
                    std::process::exit(0);
                }
                ForkResult::Child => {
                    let _ = close(w);

                    // Redirect fd 0 → /dev/null, fd 1+2 → log file using raw libc
                    unsafe {
                        let null = libc::open(
                            b"/dev/null\0".as_ptr() as *const libc::c_char,
                            libc::O_RDONLY,
                        );
                        if null >= 0 {
                            libc::dup2(null, 0);
                            libc::close(null);
                        }

                        let log_cstr = std::ffi::CString::new(
                            log_path.to_string_lossy().as_bytes()
                        ).unwrap_or_default();
                        let lf = libc::open(
                            log_cstr.as_ptr(),
                            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                            0o644_i32,
                        );
                        if lf >= 0 {
                            libc::dup2(lf, 1);
                            libc::dup2(lf, 2);
                            libc::close(lf);
                        }
                    }

                    if !cwd.is_empty() { let _ = std::env::set_current_dir(cwd); }

                    let program = std::ffi::CString::new(cmd[0].as_bytes()).unwrap();
                    let args: Vec<std::ffi::CString> = cmd.iter()
                        .map(|s| std::ffi::CString::new(s.as_bytes()).unwrap())
                        .collect();
                    let envp: Vec<std::ffi::CString> = env.iter()
                        .map(|(k, v)| std::ffi::CString::new(format!("{k}={v}").as_bytes()).unwrap())
                        .collect();
                    let _ = execvpe(&program, &args, &envp);
                    std::process::exit(1);
                }
            }
        }
    }
}

// ── PID alive check ───────────────────────────────────────────────────────

pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 { return false; }
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    match kill(Pid::from_raw(pid as i32), None) {
        Ok(()) => true,
        Err(nix::errno::Errno::EPERM) => true,
        _ => false,
    }
}

// ── Stop a set of PIDs ────────────────────────────────────────────────────

pub fn stop_pids(pids: &[u32], on_done: impl FnOnce() + Send + 'static) {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    let pids = pids.to_vec();
    for &pid in &pids {
        let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
    }
    std::thread::spawn(move || {
        let deadline = Instant::now() + REAP_DEADLINE;
        while Instant::now() < deadline {
            if pids.iter().all(|&p| !pid_alive(p)) { on_done(); return; }
            std::thread::sleep(REAP_POLL);
        }
        for &pid in &pids {
            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
        }
        on_done();
    });
}

// ── Walk /proc for descendants ────────────────────────────────────────────

pub fn descendants(pid: u32) -> Vec<u32> {
    let mut result = Vec::new();
    let mut stack = vec![pid];
    while let Some(p) = stack.pop() {
        let task = std::path::PathBuf::from(format!("/proc/{p}/task"));
        let Ok(rd) = std::fs::read_dir(&task) else { continue };
        for tid in rd.flatten() {
            let child_file = tid.path().join("children");
            if let Ok(content) = std::fs::read_to_string(&child_file) {
                for s in content.split_whitespace() {
                    if let Ok(child) = s.parse::<u32>() {
                        result.push(child);
                        stack.push(child);
                    }
                }
            }
        }
    }
    result
}

// ── UUID /proc scan ───────────────────────────────────────────────────────

pub fn scan_uuid_pids(game_uuid: &str) -> HashSet<u32> {
    let needle = format!("CRUCIBLE_GAME_ID={game_uuid}").into_bytes();
    let mut found = HashSet::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return found };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let s = name.to_string_lossy();
        if !s.chars().all(|c| c.is_ascii_digit()) { continue; }
        let env_path = entry.path().join("environ");
        if let Ok(data) = std::fs::read(&env_path) {
            if data.windows(needle.len()).any(|w| w == needle) {
                if let Ok(pid) = s.parse::<u32>() { found.insert(pid); }
            }
        }
    }
    found
}

// ── Screensaver inhibit (dbus) ────────────────────────────────────────────

pub fn inhibit_screensaver() -> Option<u32> {
    let out = std::process::Command::new("dbus-send")
        .args(["--session","--dest=org.freedesktop.ScreenSaver",
               "--type=method_call","--print-reply",
               "/org/freedesktop/ScreenSaver",
               "org.freedesktop.ScreenSaver.Inhibit",
               "string:Crucible","string:Game is running"])
        .output().ok()?;
    if !out.status.success() { return None; }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(rest) = line.trim().strip_prefix("uint32 ") {
            return rest.trim().parse().ok();
        }
    }
    None
}

pub fn uninhibit_screensaver(cookie: u32) {
    let _ = std::process::Command::new("dbus-send")
        .args(["--session","--dest=org.freedesktop.ScreenSaver",
               "--type=method_call",
               "/org/freedesktop/ScreenSaver",
               "org.freedesktop.ScreenSaver.UnInhibit",
               &format!("uint32:{cookie}")])
        .output();
}
