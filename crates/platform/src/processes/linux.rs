//! `/proc` process inspection for external runtime-use checks and playtime tracking.
//!
//! The identity of a process is `(pid, start time)`: Linux reuses PIDs, so a start-time change
//! means a different process. `created_at` is derived from `/proc/stat` `btime` and the `starttime`
//! field of `/proc/<pid>/stat`, assuming the common Linux userspace clock of 100 ticks per second.
//! Unreadable processes are skipped, exactly as the Windows adapter skips inaccessible handles.
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gamenode_core::ports::{ProcessError, ProcessIdentity, ProcessInfo, Processes};

/// Linux `USER_HZ`; every architecture GameNode ships for uses 100 except alpha.
const TICKS_PER_SECOND: u64 = 100;

#[derive(Default)]
pub struct HostProcesses;

impl HostProcesses {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Clone, Copy)]
struct Stat {
    parent: u32,
    start_ticks: u64,
}

fn boot_time() -> Option<SystemTime> {
    let text = std::fs::read_to_string("/proc/stat").ok()?;
    let seconds: u64 = text
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    Some(UNIX_EPOCH + Duration::from_secs(seconds))
}

fn stat(pid: u32) -> Option<Stat> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `comm` may contain spaces and parentheses; fields resume after its final ')'.
    let mut fields = text.rsplit_once(')')?.1.split_ascii_whitespace();
    let _state = fields.next()?;
    let parent = fields.next()?.parse().ok()?;
    let start_ticks = fields.nth(17)?.parse().ok()?;
    Some(Stat {
        parent,
        start_ticks,
    })
}

fn executable(pid: u32) -> Option<String> {
    let path = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    let path = path.to_string_lossy();
    // A replaced or unlinked binary keeps the approved pathname plus this kernel suffix; keep
    // the process visible so a backup is not allowed to race an emulator running the old image.
    Some(path.strip_suffix(" (deleted)").unwrap_or(&path).to_owned())
}

fn inspect(pid: u32, boot: SystemTime) -> Option<(ProcessInfo, u32)> {
    let stat = stat(pid)?;
    let exe_path = executable(pid)?;
    Some((
        ProcessInfo {
            pid,
            created_at: boot
                + Duration::from_millis(stat.start_ticks.saturating_mul(1000 / TICKS_PER_SECOND)),
            exe_path,
            parent: None,
        },
        stat.parent,
    ))
}

impl Processes for HostProcesses {
    fn list(&self) -> Result<Vec<ProcessInfo>, ProcessError> {
        let boot = boot_time().ok_or(ProcessError::Failed)?;
        let entries = std::fs::read_dir("/proc").map_err(|_| ProcessError::Failed)?;
        let mut identities = BTreeMap::new();
        let mut rows: Vec<(ProcessInfo, u32)> = Vec::new();
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            let Some((process, parent)) = inspect(pid, boot) else {
                continue;
            };
            identities.insert(
                pid,
                ProcessIdentity {
                    pid,
                    created_at: process.created_at,
                },
            );
            rows.push((process, parent));
        }
        Ok(rows
            .into_iter()
            .map(|(mut process, parent)| {
                process.parent = identities
                    .get(&parent)
                    .filter(|identity| identity.created_at <= process.created_at)
                    .cloned();
                process
            })
            .collect())
    }

    fn watch(
        &self,
        process: &ProcessInfo,
        on_exit: Box<dyn FnOnce(SystemTime) + Send>,
    ) -> Result<(), ProcessError> {
        let boot = boot_time().ok_or(ProcessError::Failed)?;
        let (current, _) = inspect(process.pid, boot).ok_or(ProcessError::Failed)?;
        if current.created_at != process.created_at {
            return Err(ProcessError::Failed);
        }
        let pid = rustix::process::Pid::from_raw(process.pid as i32).ok_or(ProcessError::Failed)?;
        let watched_pid = process.pid;
        let watched_created_at = process.created_at;
        let pidfd = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()).ok();
        std::thread::Builder::new()
            .name("gamenode-process-watch".into())
            .spawn(move || {
                let waited = pidfd.is_some_and(|fd| {
                    // A pidfd is readable once any process, child or not, exits. Polling it never
                    // reaps a child that the application owns.
                    let mut poll_fds = [rustix::event::PollFd::new(
                        &fd,
                        rustix::event::PollFlags::IN,
                    )];
                    loop {
                        match rustix::event::poll(&mut poll_fds, None) {
                            Ok(_) => return true,
                            Err(rustix::io::Errno::INTR) => continue,
                            Err(_) => return false,
                        }
                    }
                });
                if !waited {
                    // Kernels without pidfds, or a vanished descriptor: poll the identity.
                    while inspect(watched_pid, boot)
                        .is_some_and(|(seen, _)| seen.created_at == watched_created_at)
                    {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                on_exit(SystemTime::now());
            })
            .map_err(|_| ProcessError::Failed)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn canonical(path: &str) -> std::path::PathBuf {
        std::fs::canonicalize(path).unwrap()
    }

    #[test]
    fn a_child_is_listed_with_its_real_executable_and_parent() {
        let mut child = Command::new("/bin/sleep")
            .arg("5")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let adapter = HostProcesses::new();
        let process = adapter
            .list()
            .unwrap()
            .into_iter()
            .find(|process| process.pid == child.id())
            .expect("the child is listed");
        assert_eq!(
            std::path::Path::new(&process.exe_path),
            canonical("/bin/sleep")
        );
        let parent = process
            .parent
            .as_ref()
            .expect("the observing test process is its parent");
        assert_eq!(parent.pid, std::process::id());
        assert!(parent.created_at <= process.created_at);

        let (tx, rx) = std::sync::mpsc::channel();
        adapter
            .watch(
                &process,
                Box::new(move |at| {
                    tx.send(at).unwrap();
                }),
            )
            .unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        let at = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(at >= process.created_at);
        assert!(at <= SystemTime::now());
    }

    #[test]
    fn a_reused_pid_identity_is_refused() {
        let mut child = Command::new("/bin/sleep")
            .arg("5")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let adapter = HostProcesses::new();
        let current = adapter
            .list()
            .unwrap()
            .into_iter()
            .find(|process| process.pid == child.id())
            .unwrap();
        let stale = ProcessInfo {
            created_at: current.created_at - Duration::from_secs(1),
            ..current
        };
        assert_eq!(
            adapter.watch(&stale, Box::new(|_| panic!("must not report an exit"))),
            Err(ProcessError::Failed)
        );
        child.kill().unwrap();
        child.wait().unwrap();
    }
}
