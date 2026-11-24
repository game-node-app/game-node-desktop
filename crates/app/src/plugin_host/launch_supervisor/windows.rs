//! Windows custom launches, created suspended and assigned to a Job Object before execution.

use super::{LaunchTree, SpawnOutcome, TreeState};
use gamenode_core::drm_free::CustomLaunchCommand;
use std::collections::BTreeMap;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JobObjectBasicAccountingInformation, QueryInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION,
    ResumeThread, STARTUPINFOW, TerminateProcess,
};

struct WindowsTree {
    job: isize,
    process: isize,
    assigned: bool,
}

impl LaunchTree for WindowsTree {
    fn state(&mut self) -> TreeState {
        if !self.assigned {
            // An unassigned suspended process can only be released after termination is confirmed.
            use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
            use windows_sys::Win32::System::Threading::WaitForSingleObject;
            return match unsafe { WaitForSingleObject(self.process as HANDLE, 0) } {
                WAIT_OBJECT_0 => TreeState::Exited,
                WAIT_TIMEOUT => TreeState::Running,
                _ => TreeState::Unknown,
            };
        }
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        let ok = unsafe {
            QueryInformationJobObject(
                self.job as HANDLE,
                JobObjectBasicAccountingInformation,
                &mut info as *mut _ as *mut _,
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            TreeState::Unknown
        } else if info.ActiveProcesses == 0 {
            TreeState::Exited
        } else {
            TreeState::Running
        }
    }
    fn terminate(&mut self) -> Result<(), ()> {
        let ok = unsafe {
            if self.assigned {
                TerminateJobObject(self.job as HANDLE, 1)
            } else {
                TerminateProcess(self.process as HANDLE, 1)
            }
        };
        if ok == 0 { Err(()) } else { Ok(()) }
    }
}
impl Drop for WindowsTree {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.process as HANDLE);
            CloseHandle(self.job as HANDLE);
        }
    }
}

fn wide(text: &std::ffi::OsStr) -> Option<Vec<u16>> {
    let mut result: Vec<u16> = text.encode_wide().collect();
    if result.contains(&0) {
        return None;
    }
    result.push(0);
    Some(result)
}

/// Follows the Windows CRT argument rules, including trailing backslashes before a quote.
fn quote_argument(arg: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', slashes));
                slashes = 0;
                out.push(c);
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

fn environment(command: &CustomLaunchCommand) -> Option<Vec<u16>> {
    let mut values: BTreeMap<String, (std::ffi::OsString, std::ffi::OsString)> = BTreeMap::new();
    for (key, value) in std::env::vars_os() {
        let upper = key.to_string_lossy().to_ascii_uppercase();
        if matches!(
            upper.as_str(),
            "PATH"
                | "PATHEXT"
                | "SYSTEMROOT"
                | "WINDIR"
                | "TEMP"
                | "TMP"
                | "USERPROFILE"
                | "HOMEDRIVE"
                | "HOMEPATH"
                | "APPDATA"
                | "LOCALAPPDATA"
                | "PROGRAMDATA"
                | "COMSPEC"
                | "NUMBER_OF_PROCESSORS"
                | "PROCESSOR_ARCHITECTURE"
                | "PROCESSOR_IDENTIFIER"
                | "PROCESSOR_LEVEL"
                | "PROCESSOR_REVISION"
        ) {
            values.insert(upper, (key, value));
        }
    }
    for key in &command.environment.unset {
        values.remove(&key.to_ascii_uppercase());
    }
    for (key, value) in &command.environment.set {
        values.insert(key.to_ascii_uppercase(), (key.into(), value.into()));
    }
    let mut block = Vec::new();
    for (_, (key, value)) in values {
        let mut pair = key;
        pair.push("=");
        pair.push(value);
        block.extend(wide(&pair)?);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    Some(block)
}

/// The caller rechecks the approved file identity and directory before invoking this function.
pub(super) fn spawn(command: &CustomLaunchCommand) -> SpawnOutcome {
    if command.executable.is_empty()
        || command.working_directory.is_empty()
        || !command.environment.validate()
        || command.arguments.iter().any(|s| s.contains('\0'))
    {
        return SpawnOutcome::NotStarted;
    }
    let Some(executable) = wide(std::ffi::OsStr::new(&command.executable)) else {
        return SpawnOutcome::NotStarted;
    };
    let Some(directory) = wide(std::ffi::OsStr::new(&command.working_directory)) else {
        return SpawnOutcome::NotStarted;
    };
    let Some(mut environment) = environment(command) else {
        return SpawnOutcome::NotStarted;
    };
    let commandline = std::iter::once(command.executable.as_str())
        .chain(command.arguments.iter().map(String::as_str))
        .map(quote_argument)
        .collect::<Vec<_>>()
        .join(" ");
    let Some(mut commandline) = wide(std::ffi::OsStr::new(&commandline)) else {
        return SpawnOutcome::NotStarted;
    };
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return SpawnOutcome::NotStarted;
    }
    let mut startup = STARTUPINFOW::default();
    startup.cb = std::mem::size_of_val(&startup) as u32;
    let mut process = PROCESS_INFORMATION::default();
    let created = unsafe {
        CreateProcessW(
            executable.as_ptr(),
            commandline.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            environment.as_mut_ptr() as *const _,
            directory.as_ptr(),
            &startup,
            &mut process,
        )
    };
    if created == 0 {
        unsafe {
            CloseHandle(job);
        }
        return SpawnOutcome::NotStarted;
    }
    let assigned = unsafe { AssignProcessToJobObject(job, process.hProcess) } != 0;
    let tree = WindowsTree {
        job: job as isize,
        process: process.hProcess as isize,
        assigned,
    };
    if !assigned {
        // The new process is still suspended. Do not release the lease unless its death can be
        // confirmed; the retained process handle also avoids PID-reuse confusion.
        unsafe {
            TerminateProcess(process.hProcess, 1);
            CloseHandle(process.hThread);
        }
        return SpawnOutcome::Uncertain(Box::new(tree));
    }
    let resumed = unsafe { ResumeThread(process.hThread) };
    unsafe {
        CloseHandle(process.hThread);
    }
    if resumed == u32::MAX {
        SpawnOutcome::Uncertain(Box::new(tree))
    } else {
        SpawnOutcome::Started(Box::new(tree))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn windows_arguments_quote_backslashes_and_quotes() {
        assert_eq!(quote_argument(""), "\"\"");
        assert_eq!(quote_argument("a b"), "\"a b\"");
        assert_eq!(quote_argument("a\\\"b\\"), "\"a\\\\\\\"b\\\\\"");
    }

    /// Executed by the test harness as a separate process inside the supervised job.
    #[test]
    fn windows_tree_fixture_process() {
        match std::env::var("GAMENODE_WINDOWS_TREE_FIXTURE").as_deref() {
            Ok("root") => {
                let child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "plugin_host::launch_supervisor::windows::tests::windows_tree_fixture_process", "--nocapture"])
                    .env("GAMENODE_WINDOWS_TREE_FIXTURE", "child")
                    .spawn().unwrap();
                std::fs::write(
                    std::env::var_os("GAMENODE_WINDOWS_PIDFILE").unwrap(),
                    child.id().to_string(),
                )
                .unwrap();
            }
            Ok("child") => std::thread::sleep(Duration::from_secs(30)),
            _ => {}
        }
    }

    #[test]
    fn job_tracks_child_after_wrapper_exit_and_fences_explicit_stop() {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("child.pid");
        let executable = std::env::current_exe().unwrap();
        let mut environment = gamenode_core::drm_free::LaunchEnvironmentDelta::default();
        environment
            .set
            .insert("GAMENODE_WINDOWS_TREE_FIXTURE".into(), "root".into());
        environment.set.insert(
            "GAMENODE_WINDOWS_PIDFILE".into(),
            pidfile.to_string_lossy().into_owned(),
        );
        let command = CustomLaunchCommand {
            executable: executable.to_string_lossy().into_owned(),
            file_identity: String::new(),
            arguments: vec![
                "--exact".into(),
                "plugin_host::launch_supervisor::windows::tests::windows_tree_fixture_process"
                    .into(),
                "--nocapture".into(),
            ],
            working_directory: dir.path().to_string_lossy().into_owned(),
            environment,
        };
        let mut tree = match spawn(&command) {
            SpawnOutcome::Started(tree) => tree,
            _ => panic!("job did not start"),
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !pidfile.exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid = std::fs::read_to_string(pidfile)
            .unwrap()
            .parse::<u32>()
            .unwrap();
        let child = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        assert!(!child.is_null());
        // The wrapper has returned, but the descendant remains in the same job.
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(tree.state(), TreeState::Running);
        tree.terminate().unwrap();
        while tree.state() != TreeState::Exited {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(unsafe { WaitForSingleObject(child, 0) }, WAIT_OBJECT_0);
        unsafe {
            CloseHandle(child);
        }
    }
}
