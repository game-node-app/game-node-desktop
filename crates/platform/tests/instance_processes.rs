//! Real-process checks of single-instance election, forwarding and crash recovery.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const PROBE: &str = env!("CARGO_BIN_EXE_instance_probe");

struct Run {
    child: Child,
}

impl Run {
    fn start(endpoint: &str, extra: &[&str]) -> Self {
        let mut probe = std::path::PathBuf::from(PROBE);
        // Cross-built tests executed on Windows cannot use the Linux build path.
        // Cargo places the helper beside the parent directory of the test binary.
        if !probe.is_file() {
            let executable = std::env::current_exe().expect("test executable path");
            probe = executable
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join(std::path::Path::new(PROBE).file_name().unwrap());
        }
        let child = Command::new(probe)
            .args(["--endpoint", endpoint])
            .args(extra)
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn probe");
        Self { child }
    }

    /// Waits for exit and returns stdout lines and success.
    fn finish(mut self) -> (Vec<String>, bool) {
        let stdout = self.child.stdout.take().unwrap();
        let lines = BufReader::new(stdout).lines().map(Result::unwrap).collect();
        let status = self.child.wait().unwrap();
        (lines, status.success())
    }

    /// Blocks until the first stdout line appears.
    fn first_line(&mut self) -> String {
        let stdout = self.child.stdout.as_mut().unwrap();
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).unwrap();
        line.trim().to_owned()
    }
}

fn endpoint(tag: &str) -> (String, Option<tempfile::TempDir>) {
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().unwrap();
        (
            dir.path().join(tag).to_string_lossy().into_owned(),
            Some(dir),
        )
    }
    #[cfg(windows)]
    {
        (
            format!("gamenode-process-test-{tag}-{}", std::process::id()),
            None,
        )
    }
}

#[test]
fn simultaneous_launches_elect_one_owner_and_hand_over_the_rest() {
    let (endpoint, _dir) = endpoint("race");
    const LAUNCHES: usize = 8;
    let runs: Vec<_> = (0..LAUNCHES)
        .map(|_| Run::start(&endpoint, &["--serve-ms", "3000"]))
        .collect();
    let outputs: Vec<_> = runs.into_iter().map(Run::finish).collect();

    assert!(
        outputs.iter().all(|(_, ok)| *ok),
        "every launch exits cleanly: {outputs:?}"
    );
    let owners: Vec<_> = outputs
        .iter()
        .filter(|(l, _)| l.first().map(String::as_str) == Some("OWNER"))
        .collect();
    assert_eq!(owners.len(), 1, "exactly one owner: {outputs:?}");
    let forwarded = outputs.iter().filter(|(l, _)| l == &["FORWARDED"]).count();
    assert_eq!(forwarded, LAUNCHES - 1);
    let activations = owners[0]
        .0
        .iter()
        .filter(|l| l.starts_with("MSG") && l.contains("activate"))
        .count();
    assert_eq!(
        activations,
        LAUNCHES - 1,
        "the owner received every hand-over: {:?}",
        owners[0].0
    );
}

#[test]
fn a_callback_argument_reaches_the_owner_intact() {
    let (endpoint, _dir) = endpoint("callback");
    let mut owner = Run::start(&endpoint, &["--serve-ms", "2500"]);
    assert_eq!(owner.first_line(), "OWNER");

    let url = r#"app.gamenode://home?desktop_attempt=abc&cookie=a%20b%3Bc"; echo pwned"#;
    let (lines, ok) = Run::start(&endpoint, &["--callback", url]).finish();
    assert!(ok);
    assert_eq!(
        lines,
        ["FORWARDED"],
        "the second process exits after forwarding"
    );

    let (rest, ok) = owner.finish();
    assert!(ok);
    let expected = format!(
        r#"MSG {{"type":"auth-callback","url":{}}}"#,
        serde_json::to_string(url).unwrap()
    );
    assert_eq!(rest, [expected]);
}

#[test]
fn a_crashed_owner_leaves_nothing_that_blocks_the_next_launch() {
    let (endpoint, _dir) = endpoint("crash");
    let (lines, ok) = Run::start(&endpoint, &["--crash"]).finish();
    assert_eq!(lines, ["OWNER"]);
    assert!(!ok, "the probe aborted");
    #[cfg(unix)]
    assert!(
        std::path::Path::new(&endpoint)
            .join("instance.sock")
            .exists(),
        "a stale socket was left behind"
    );

    let mut next = Run::start(&endpoint, &["--serve-ms", "400"]);
    assert_eq!(
        next.first_line(),
        "OWNER",
        "the stale endpoint was recovered"
    );
    let (_, ok) = next.finish();
    assert!(ok);
}

#[test]
fn a_launch_waits_for_an_owner_that_is_still_starting() {
    let (endpoint, _dir) = endpoint("slow");
    let mut owner = Run::start(
        &endpoint,
        &["--delay-serve-ms", "1200", "--serve-ms", "2500"],
    );
    assert_eq!(owner.first_line(), "OWNER");
    // The owner has claimed the endpoint but is not serving yet: the second launch waits.
    let started = std::time::Instant::now();
    let (lines, ok) = Run::start(&endpoint, &[]).finish();
    assert!(ok && lines == ["FORWARDED"], "{lines:?}");
    assert!(
        started.elapsed() >= Duration::from_millis(500),
        "it really waited"
    );

    let (rest, ok) = owner.finish();
    assert!(ok);
    assert_eq!(rest, [r#"MSG {"type":"activate"}"#]);
}
