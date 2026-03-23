//! Process management utilities for safe signal handling.

/// Send a signal to a process group.
/// Returns Ok(()) on success, Err if pid is invalid or kill fails.
#[cfg(unix)]
pub fn kill_process_group(pid: u32, signal: i32) -> anyhow::Result<()> {
    if pid == 0 {
        anyhow::bail!("refusing to signal pid 0 (would affect current process group)");
    }
    // SAFETY: pid is verified non-zero. Negating pid targets the process group.
    // libc::kill with a negative pid sends the signal to all processes in that group.
    // This is safe because: (1) pid is a valid u32 != 0, (2) the cast to i32 and
    // negation produces a valid negative pid for process group signaling.
    let ret = unsafe { libc::kill(-(pid as i32), signal) };
    if ret == 0 {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "kill({}, signal={}) failed: {}",
            pid,
            signal,
            std::io::Error::last_os_error()
        ))
    }
}

/// Send a signal to a single process (not process group).
#[cfg(unix)]
pub fn kill_process(pid: u32, signal: i32) -> anyhow::Result<()> {
    if pid == 0 {
        anyhow::bail!("refusing to signal pid 0");
    }
    // SAFETY: pid is verified non-zero. libc::kill with positive pid sends
    // the signal to that specific process only.
    let ret = unsafe { libc::kill(pid as i32, signal) };
    if ret == 0 {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "kill({}, signal={}) failed: {}",
            pid,
            signal,
            std::io::Error::last_os_error()
        ))
    }
}

/// Graceful shutdown: SIGTERM → wait up to `timeout` → SIGKILL if still alive.
#[cfg(unix)]
pub async fn graceful_kill_group(pid: u32, timeout: std::time::Duration) -> anyhow::Result<()> {
    // First try SIGTERM
    if let Err(e) = kill_process_group(pid, libc::SIGTERM) {
        tracing::debug!(pid, error = %e, "SIGTERM failed, trying SIGKILL");
        return kill_process_group(pid, libc::SIGKILL);
    }

    // Wait for process to exit
    tokio::time::sleep(timeout).await;

    // If still alive, SIGKILL
    // kill with signal 0 checks if process exists
    if kill_process_group(pid, 0).is_ok() {
        kill_process_group(pid, libc::SIGKILL)?;
    }

    Ok(())
}
