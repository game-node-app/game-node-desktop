//! Linux launch helper: a dedicated subreaper owns the complete descendant tree.
//!
//! The helper exits successfully only after `wait` reports that no children remain. If it loses
//! contact with the app, it continues supervising rather than terminating an already running game.

use super::{LaunchTree, SpawnOutcome, TreeState};
use gamenode_core::drm_free::{CustomLaunchCommand, InstallationHost, LaunchEnvironmentDelta};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

pub(crate) const HELPER_ARG: &str = "--gamenode-internal-launch-supervisor";
const MAX_REQUEST: usize = 128 * 1024;

#[derive(Serialize, Deserialize)]
struct Request {
    executable: String,
    file_identity: String,
    arguments: Vec<String>,
    working_directory: String,
    set: BTreeMap<String, String>,
    unset: BTreeSet<String>,
}

struct HelperTree {
    child: Child,
    input: Option<UnixStream>,
    exit: Option<std::process::ExitStatus>,
}

impl LaunchTree for HelperTree {
    fn state(&mut self) -> TreeState {
        let result = match self.exit {
            Some(status) => Some(status),
            None => match self.child.try_wait() {
                Ok(result) => {
                    self.exit = result;
                    result
                }
                Err(_) => return TreeState::Unknown,
            },
        };
        match result {
            None => TreeState::Running,
            Some(status) if status.success() => TreeState::Exited,
            Some(_) => TreeState::Unknown,
        }
    }
    fn terminate(&mut self) -> Result<(), ()> {
        if self.state() == TreeState::Exited {
            return Ok(());
        }
        let input = self.input.as_mut().ok_or(())?;
        let result = input
            .write_all(b"STOP\n")
            .and_then(|()| input.flush())
            .map_err(|_| ());
        if result.is_err() && self.state() == TreeState::Exited {
            Ok(())
        } else {
            result
        }
    }
}

/// The OS process handle fences the helper identity against PID reuse.
pub(super) fn spawn(command: &CustomLaunchCommand) -> SpawnOutcome {
    #[cfg(test)]
    if let Some(executable) = std::env::var_os("GAMENODE_TEST_HELPER_BINARY") {
        return spawn_with_executable(command, std::path::Path::new(&executable));
    }
    // Resolve the running image rather than its install pathname: an update may replace or
    // unlink that pathname while this desktop is still running.
    spawn_with_executable(command, std::path::Path::new("/proc/self/exe"))
}

fn spawn_with_executable(
    command: &CustomLaunchCommand,
    executable: &std::path::Path,
) -> SpawnOutcome {
    if command.executable.is_empty()
        || command.working_directory.is_empty()
        || !command.environment.validate()
        || command.executable.contains('\0')
        || command.working_directory.contains('\0')
        || command.arguments.iter().any(|arg| arg.contains('\0'))
    {
        return SpawnOutcome::NotStarted;
    }
    let request = Request {
        executable: command.executable.clone(),
        file_identity: command.file_identity.clone(),
        arguments: command.arguments.clone(),
        working_directory: command.working_directory.clone(),
        set: command.environment.set.clone(),
        unset: command.environment.unset.clone(),
    };
    let Ok(mut request) = serde_json::to_vec(&request) else {
        return SpawnOutcome::NotStarted;
    };
    if request.len() >= MAX_REQUEST {
        return SpawnOutcome::NotStarted;
    }
    request.push(b'\n');
    let Ok((input, helper_input)) = UnixStream::pair() else {
        return SpawnOutcome::NotStarted;
    };
    if input
        .set_write_timeout(Some(Duration::from_secs(5)))
        .is_err()
    {
        return SpawnOutcome::NotStarted;
    }
    let helper_input: std::os::fd::OwnedFd = helper_input.into();
    let Ok(mut child) = Command::new(executable)
        .arg(HELPER_ARG)
        .stdin(Stdio::from(helper_input))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return SpawnOutcome::NotStarted;
    };
    let output = child.stdout.take();
    let mut tree = HelperTree {
        child,
        input: Some(input),
        exit: None,
    };
    let Some(output) = output else {
        return SpawnOutcome::Uncertain(Box::new(tree));
    };
    let Some(input) = tree.input.as_mut() else {
        return SpawnOutcome::Uncertain(Box::new(tree));
    };
    let written = input
        .write_all(&request)
        .and_then(|()| input.flush())
        .is_ok();
    if !written {
        return SpawnOutcome::Uncertain(Box::new(tree));
    }
    let (tx, rx) = mpsc::sync_channel(1);
    if std::thread::Builder::new()
        .name("custom-launch-ready".into())
        .spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(output)
                .take(64)
                .read_line(&mut line)
                .map(|_| line);
            let _ = tx.send(result);
        })
        .is_err()
    {
        return SpawnOutcome::Uncertain(Box::new(tree));
    }
    match rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(line)) if line == "READY\n" => SpawnOutcome::Started(Box::new(tree)),
        // A negative acknowledgement is sent only before the target is created.
        Ok(Ok(line)) if line == "NOT_STARTED\n" => {
            let _ = std::thread::Builder::new()
                .name("custom-launch-reap".into())
                .spawn(move || {
                    let _ = tree.child.wait();
                });
            SpawnOutcome::NotStarted
        }
        _ => SpawnOutcome::Uncertain(Box::new(tree)),
    }
}

fn safe_key(key: &std::ffi::OsStr) -> bool {
    let key = key.to_string_lossy();
    matches!(
        key.as_ref(),
        "PATH"
            | "HOME"
            | "USER"
            | "LOGNAME"
            | "LANG"
            | "DISPLAY"
            | "WAYLAND_DISPLAY"
            | "DBUS_SESSION_BUS_ADDRESS"
    ) || key.starts_with("LC_")
        || key.starts_with("XDG_")
}

/// Called before application initialization when the internal helper argument is present.
pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = BufReader::new(std::io::stdin());
    let mut line = String::new();
    let count = input
        .by_ref()
        .take((MAX_REQUEST + 1) as u64)
        .read_line(&mut line)?;
    if count == 0 || count > MAX_REQUEST || !line.ends_with('\n') {
        println!("NOT_STARTED");
        return Err("invalid launch request".into());
    }
    let request: Request = match serde_json::from_str(&line) {
        Ok(value) => value,
        Err(_) => {
            println!("NOT_STARTED");
            return Err("invalid launch request".into());
        }
    };
    let environment = LaunchEnvironmentDelta {
        set: request.set,
        unset: request.unset,
    };
    let preparation = (|| -> Result<_, Box<dyn std::error::Error>> {
        if !environment.validate() {
            return Err("invalid environment".into());
        }
        // PR_SET_CHILD_SUBREAPER accepts a boolean; rustix represents nonzero as a Pid.
        rustix::process::set_child_subreaper(rustix::process::Pid::from_raw(1))?;
        // Fencing stop requests requires pidfd support. Check it before creating the game.
        let _ = rustix::process::pidfd_open(
            rustix::process::getpid(),
            rustix::process::PidfdFlags::empty(),
        )?;
        let inspected =
            gamenode_platform::installations::LinuxInstallations.inspect(&request.executable)?;
        if inspected.file_identity != request.file_identity {
            return Err("executable identity changed".into());
        }
        let working = std::fs::canonicalize(&request.working_directory)?;
        if !working.is_dir() {
            return Err("invalid working directory".into());
        }
        let mut target = Command::new(inspected.executable);
        target
            .args(&request.arguments)
            .current_dir(working)
            .env_clear();
        for (key, value) in std::env::vars_os().filter(|(key, _)| safe_key(key)) {
            target.env(key, value);
        }
        target.envs(&environment.set);
        for key in &environment.unset {
            target.env_remove(key);
        }
        target
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        Ok(target.spawn()?)
    })();
    // Keep the root Child handle for the helper lifetime; `wait` below also reaps adopted
    // descendants after a wrapper exits or a descendant changes its process group.
    let _root = match preparation {
        Ok(child) => child,
        Err(error) => {
            println!("NOT_STARTED");
            return Err(error);
        }
    };
    let _ = writeln!(std::io::stdout(), "READY");
    // Once a target exists, losing the acknowledgement must not abandon its descendants.
    let _ = std::io::stdout().flush();
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("custom-launch-control".into())
        .spawn(move || {
            let mut message = String::new();
            while input.read_line(&mut message).is_ok_and(|n| n > 0) {
                if message == "STOP\n" {
                    let _ = tx.send(());
                }
                message.clear();
            }
        });
    let mut stopping = false;
    loop {
        stopping |= rx.try_recv().is_ok();
        loop {
            match rustix::process::wait(rustix::process::WaitOptions::NOHANG) {
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(rustix::io::Errno::CHILD) => return Ok(()),
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(Box::new(error)),
            }
        }
        if stopping {
            signal_direct_children();
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Signals only current direct children using pidfds. Repeating this after reaping parents catches
/// newly adopted descendants, including children that changed sessions or process groups.
fn signal_direct_children() {
    let pid = rustix::process::getpid().as_raw_nonzero().get();
    let Ok(children) = std::fs::read_to_string(format!("/proc/self/task/{pid}/children")) else {
        return;
    };
    for value in children.split_whitespace() {
        let Some(child) = value
            .parse::<i32>()
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        else {
            continue;
        };
        let Ok(fd) = rustix::process::pidfd_open(child, rustix::process::PidfdFlags::empty())
        else {
            continue;
        };
        // A PID can be recycled between reading children and opening its pidfd. Confirm the
        // current parent after opening; the pidfd itself then fences against subsequent reuse.
        let Ok(status) = std::fs::read_to_string(format!("/proc/{value}/status")) else {
            continue;
        };
        let parent = status.lines().find_map(|line| {
            line.strip_prefix("PPid:")
                .and_then(|v| v.trim().parse::<i32>().ok())
        });
        if parent == Some(pid) {
            let _ = rustix::process::pidfd_send_signal(&fd, rustix::process::Signal::KILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::grants::PluginIdentity;
    use crate::plugin_host::launch_supervisor::LaunchSupervisor;
    use crate::plugin_host::resource_leases::{
        ResourceLeaseCoordinator, ResourceLeaseError, ResourceLeaseKey, ResourceLeases,
    };
    use std::time::Instant;

    fn helper() -> Option<std::path::PathBuf> {
        std::env::var_os("GAMENODE_TEST_HELPER_BINARY").map(Into::into)
    }
    fn shell(script: &str) -> CustomLaunchCommand {
        let registration = gamenode_platform::installations::LinuxInstallations
            .inspect("/bin/sh")
            .unwrap();
        CustomLaunchCommand {
            executable: registration.executable,
            file_identity: registration.file_identity,
            arguments: vec!["-c".into(), script.into()],
            working_directory: "/".into(),
            environment: Default::default(),
        }
    }
    fn started(command: &CustomLaunchCommand, executable: &std::path::Path) -> Box<dyn LaunchTree> {
        match spawn_with_executable(command, executable) {
            SpawnOutcome::Started(tree) => tree,
            _ => panic!("helper did not start a verified launch"),
        }
    }
    fn wait_exit(tree: &mut dyn LaunchTree) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match tree.state() {
                TreeState::Exited => return,
                TreeState::Unknown => panic!("helper state became unknown"),
                TreeState::Running => {}
            }
            assert!(Instant::now() < deadline, "helper did not finish");
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    #[test]
    fn detached_descendant_is_reaped_after_wrapper_exit() {
        let Some(helper) = helper() else {
            return;
        };
        let command = shell("/usr/bin/setsid /bin/sh -c 'sleep 1' &");
        let mut tree = started(&command, &helper);
        assert_eq!(tree.state(), TreeState::Running);
        wait_exit(tree.as_mut());
    }
    #[test]
    fn explicit_stop_fences_detached_descendants() {
        let Some(helper) = helper() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("pid");
        let mut command = shell(
            "/usr/bin/setsid /bin/sh -c 'echo $$ > \"$1\"; exec sleep 30' fixture \"$1\" & wait",
        );
        command
            .arguments
            .extend(["fixture".into(), pidfile.to_string_lossy().into_owned()]);
        let mut tree = started(&command, &helper);
        assert_eq!(tree.state(), TreeState::Running);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !pidfile.exists() {
            assert!(Instant::now() < deadline, "detached child did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid = std::fs::read_to_string(pidfile)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();
        let pidfd = rustix::process::pidfd_open(
            rustix::process::Pid::from_raw(pid).unwrap(),
            rustix::process::PidfdFlags::empty(),
        )
        .unwrap();
        tree.terminate().unwrap();
        wait_exit(tree.as_mut());
        assert_eq!(
            rustix::process::pidfd_send_signal(&pidfd, rustix::process::Signal::CONT),
            Err(rustix::io::Errno::SRCH)
        );
    }
    #[test]
    fn changed_executable_identity_is_rejected_before_spawn() {
        let Some(helper) = helper() else {
            return;
        };
        let mut command = shell("exit 0");
        command.file_identity.push_str("-changed");
        assert!(matches!(
            spawn_with_executable(&command, &helper),
            SpawnOutcome::NotStarted
        ));
    }

    #[test]
    fn command_keeps_reviewed_working_directory_environment_and_arguments() {
        let Some(helper) = helper() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let working = dir.path().join("game folder %i");
        std::fs::create_dir(&working).unwrap();
        let output = dir.path().join("result");
        let mut command = shell("printf '%s:%s:%s' \"$PWD\" \"$GAMENODE_FIXTURE\" \"$1\" > \"$2\"");
        command.working_directory = working.to_string_lossy().into_owned();
        command
            .environment
            .set
            .insert("GAMENODE_FIXTURE".into(), "reviewed value".into());
        command.arguments.extend([
            "fixture".into(),
            "argument with spaces".into(),
            output.to_string_lossy().into_owned(),
        ]);
        let mut tree = started(&command, &helper);
        wait_exit(tree.as_mut());
        assert_eq!(
            std::fs::read_to_string(output).unwrap(),
            format!("{}:reviewed value:argument with spaces", working.display())
        );
    }

    #[test]
    fn proton_style_wrapper_keeps_reviewed_plan_and_tracks_its_child() {
        use std::os::unix::fs::PermissionsExt;
        let Some(helper) = helper() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let working = dir.path().join("prefix with spaces");
        std::fs::create_dir(&working).unwrap();
        let wrapper = dir.path().join("proton wrapper");
        std::fs::write(&wrapper, "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$@\" \"$PLAN_MARKER\" > \"$CAPTURE\"\n/usr/bin/setsid /bin/sh -c 'sleep 1' &\n").unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let registration = gamenode_platform::installations::LinuxInstallations
            .inspect(wrapper.to_str().unwrap())
            .unwrap();
        let output = dir.path().join("wrapper.out");
        let mut environment = LaunchEnvironmentDelta::default();
        environment
            .set
            .insert("CAPTURE".into(), output.to_string_lossy().into_owned());
        environment
            .set
            .insert("PLAN_MARKER".into(), "reviewed".into());
        let command = CustomLaunchCommand {
            executable: registration.executable,
            file_identity: registration.file_identity,
            arguments: vec![
                "run".into(),
                "--prefix".into(),
                "/prefix with spaces".into(),
                "--".into(),
                "Z:/Games/Example/game.exe".into(),
                "argument with spaces".into(),
            ],
            working_directory: working.to_string_lossy().into_owned(),
            environment,
        };
        let mut tree = started(&command, &helper);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !output.exists() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(output).unwrap(),
            format!(
                "{}\nrun\n--prefix\n/prefix with spaces\n--\nZ:/Games/Example/game.exe\nargument with spaces\nreviewed\n",
                working.display()
            )
        );
        assert_eq!(tree.state(), TreeState::Running);
        wait_exit(tree.as_mut());
    }

    #[test]
    fn monitor_keeps_lease_until_detached_descendant_exits() {
        let Some(helper) = helper() else {
            return;
        };
        let key = ResourceLeaseKey::derive(
            &PluginIdentity {
                plugin_id: "fixture".into(),
                publisher_fingerprint: "fixture".into(),
                profile_id: "fixture".into(),
            },
            1,
            "save",
            1,
            b"backing",
        )
        .unwrap();
        let leases = ResourceLeases::default();
        let supervisor = LaunchSupervisor::start_monitor().unwrap();
        let command = shell("/usr/bin/setsid /bin/sh -c 'sleep 1' &");
        supervisor
            .start(&leases, &[key], || spawn_with_executable(&command, &helper))
            .unwrap();
        assert!(matches!(
            leases.acquire_exclusive(&[key]),
            Err(ResourceLeaseError::Busy)
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        while leases.acquire_exclusive(&[key]).is_err() {
            assert!(
                Instant::now() < deadline,
                "monitor did not release the finished tree"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    #[test]
    fn unexpected_helper_exit_keeps_tree_state_unknown() {
        let (input, peer) = UnixStream::pair().unwrap();
        let peer: std::os::fd::OwnedFd = peer.into();
        let child = Command::new("/bin/sh")
            .args(["-c", "exit 9"])
            .stdin(Stdio::from(peer))
            .spawn()
            .unwrap();
        let mut tree = HelperTree {
            child,
            input: Some(input),
            exit: None,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while tree.state() == TreeState::Running {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(tree.state(), TreeState::Unknown);
    }
}
