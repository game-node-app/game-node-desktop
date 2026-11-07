use gamenode_app::daemon::{connect, proto, proto::*};
use serde_json::{Value, json};
use std::{
    error::Error,
    fmt,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use tonic::{Code, Request, transport::Channel};

#[derive(Debug)]
struct CliError {
    message: String,
    usage: bool,
    code: &'static str,
    data: Option<Value>,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for CliError {}

fn usage(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(CliError {
        message: message.into(),
        usage: true,
        code: "INVALID_ARGUMENT",
        data: None,
    })
}

fn command_error(message: impl Into<String>, code: &'static str, data: Value) -> Box<dyn Error> {
    Box::new(CliError {
        message: message.into(),
        usage: false,
        code,
        data: Some(data),
    })
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let json_output = remove_flag(&mut args, "--json");
    let no_wait = remove_flag(&mut args, "--no-wait");
    match run(args, no_wait).await {
        Ok((text, data)) => {
            if json_output {
                println!(
                    "{}",
                    json!({
                        "apiVersion": "gamenode.cli/v1",
                        "ok": true,
                        "data": data,
                        "error": null,
                    })
                );
            } else {
                println!("{text}");
            }
        }
        Err(error) => {
            let typed_error = error.downcast_ref::<CliError>();
            let exit_code = typed_error.map_or(1, |error| if error.usage { 2 } else { 1 });
            if json_output {
                println!(
                    "{}",
                    json!({
                        "apiVersion": "gamenode.cli/v1",
                        "ok": false,
                        "data": typed_error.and_then(|error| error.data.clone()),
                        "error": {
                            "code": typed_error.map_or("CLI_ERROR", |error| error.code),
                            "message": error.to_string(),
                            "retryable": false,
                        },
                    })
                );
            } else {
                if let Some(data) = typed_error.and_then(|error| error.data.as_ref()) {
                    eprintln!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
                }
                eprintln!("gamenode-cli: {error}");
            }
            std::process::exit(exit_code);
        }
    }
}

fn remove_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let present = args.iter().any(|arg| arg == flag);
    args.retain(|arg| arg != flag);
    present
}

type CommandOutput = (String, Value);

async fn run(args: Vec<String>, no_wait: bool) -> Result<CommandOutput, Box<dyn Error>> {
    let mut args = args.into_iter();
    let group = args.next().unwrap_or_else(|| "help".into());
    let action = args.next().unwrap_or_default();
    let rest = args.collect::<Vec<_>>();
    if group == "help" || group == "--help" || group == "-h" {
        return Ok((help(), json!({ "help": help() })));
    }
    if group == "daemon" && action == "start" {
        let started = start_daemon().await?;
        return Ok((
            if started {
                "daemon started"
            } else {
                "daemon already running"
            }
            .into(),
            json!({ "started": started, "running": true }),
        ));
    }
    validate_args(&group, &action, &rest)?;

    let socket = socket_path()?;
    let mut client = match connect(socket.clone()).await {
        Ok(client) => client,
        Err(error) if group == "daemon" && action == "status" => {
            let text = "daemon stopped".to_owned();
            let data = json!({ "running": false, "version": null, "platform": null });
            let _ = error;
            return Ok((text, data));
        }
        Err(_) if group == "daemon" && action == "stop" => {
            return Ok(("daemon already stopped".into(), json!({ "stopped": true })));
        }
        Err(_) => {
            start_daemon().await?;
            connect(socket.clone()).await?
        }
    };

    match (group.as_str(), action.as_str()) {
        ("daemon", "status") => {
            let s = client
                .get_status(Request::new(Empty {}))
                .await?
                .into_inner();
            Ok((
                format!("running version={} platform={}", s.version, s.platform),
                json!({ "running": true, "version": s.version, "platform": s.platform }),
            ))
        }
        ("daemon", "stop") => {
            client.stop(Request::new(Empty {})).await?;
            wait_daemon_stopped(&socket).await?;
            Ok(("daemon stopped".into(), json!({ "stopped": true })))
        }
        ("operations", "status") => {
            let operation_id = rest[0].clone();
            let operation = client
                .get_operation_status(Request::new(OperationRequest {
                    operation_id: operation_id.clone(),
                }))
                .await?
                .into_inner();
            let data = operation_json(operation)?;
            if data["state"] == "partial" {
                let providers = scan_failure_names(&data).join(", ");
                return Err(command_error(
                    format!("scan completed with provider failures: {providers}"),
                    "SCAN_PARTIAL",
                    data,
                ));
            }
            Ok((serde_json::to_string_pretty(&data)?, data))
        }
        ("auth", "status") => {
            let s = client
                .get_auth_status(Request::new(Empty {}))
                .await?
                .into_inner();
            let text = if s.account.is_empty() {
                format!(
                    "{}\nsync: {}\nplaytime source: {}",
                    s.state,
                    if s.sync_enabled { "on" } else { "off" },
                    s.playtime_source
                )
            } else {
                format!(
                    "{} ({})\nsync: {}\nplaytime source: {}",
                    s.state,
                    s.account,
                    if s.sync_enabled { "on" } else { "off" },
                    s.playtime_source
                )
            };
            Ok((text, auth_json(s)))
        }
        ("auth", "login") => {
            let provider = if rest.first().is_some_and(|arg| arg == "--provider") {
                rest.get(1)
            } else {
                rest.first()
            }
            .ok_or_else(|| {
                usage("usage: gamenode-cli auth login <google|discord|twitter> [--no-wait]")
            })?
            .clone();
            let mut s = client
                .start_login(Request::new(StartLoginRequest { provider }))
                .await?
                .into_inner();
            if !no_wait {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(10 * 60);
                let mut attempted = false;
                loop {
                    if s.state == "pending" || s.state == "verifying" {
                        attempted = true;
                    }
                    if s.state == "signed_in" {
                        return Ok(("signed_in".into(), auth_json(s)));
                    }
                    if attempted && matches!(s.state.as_str(), "signed_out" | "restore_failed") {
                        return Err(
                            "login did not complete; check the browser flow and run auth status"
                                .into(),
                        );
                    }
                    if tokio::time::Instant::now() >= deadline {
                        return Err(
                            "timed out waiting for login; the daemon may still be processing it"
                                .into(),
                        );
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    s = client
                        .get_auth_status(Request::new(Empty {}))
                        .await?
                        .into_inner();
                }
            }
            Ok((s.state.clone(), auth_json(s)))
        }
        ("auth", "logout") => {
            client.sign_out(Request::new(Empty {})).await?;
            let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
            loop {
                let s = client
                    .get_auth_status(Request::new(Empty {}))
                    .await?
                    .into_inner();
                if s.state == "signed_out" {
                    return Ok(("signed_out".into(), auth_json(s)));
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err(format!("logout is still {}", s.state).into());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        ("settings", "sync") => {
            let enabled = match rest.first().map(String::as_str) {
                Some("on") => true,
                Some("off") => false,
                _ => return Err(usage("usage: gamenode-cli settings sync <on|off>")),
            };
            client
                .set_sync(Request::new(SyncSettings { enabled }))
                .await?;
            let state = if enabled { "on" } else { "off" };
            Ok((format!("sync {state}"), json!({ "syncEnabled": enabled })))
        }
        ("settings", "playtime-source") => {
            let source = rest
                .first()
                .ok_or_else(|| {
                    usage("usage: gamenode-cli settings playtime-source <store|internal>")
                })?
                .clone();
            client
                .set_playtime_source(Request::new(PlaytimeSourceRequest {
                    source: source.clone(),
                }))
                .await?;
            Ok((
                format!("playtime source {source}"),
                json!({ "playtimeSource": source }),
            ))
        }
        ("games", "list") => {
            let s = get_installations(&mut client).await?;
            let entries = s
                .entries
                .into_iter()
                .map(|g| {
                    json!({
                        "entryKey": g.entry_key, "provider": g.provider, "storeId": g.store_id,
                        "title": g.title, "installDir": g.install_dir,
                        "match": serde_json::from_str::<Value>(&g.r#match).unwrap_or(Value::Null),
                        "running": g.running,
                    })
                })
                .collect::<Vec<_>>();
            let text = format!(
                "{} installation(s){}",
                entries.len(),
                if s.scanning {
                    " (scan in progress)"
                } else {
                    ""
                }
            );
            Ok((
                installation_list_text(&text, &entries),
                json!({ "access": s.access, "scanning": s.scanning, "entries": entries }),
            ))
        }
        ("games", "scan") => {
            let _ = get_installations(&mut client).await?;
            let scan = client
                .start_scan(Request::new(Empty {}))
                .await?
                .into_inner();
            if no_wait {
                return Ok((
                    format!("scan started: {}", scan.operation_id),
                    json!({ "operationId": scan.operation_id, "kind": "scan", "state": "running", "result": null }),
                ));
            }
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10 * 60);
            loop {
                let operation = client
                    .get_operation_status(Request::new(OperationRequest {
                        operation_id: scan.operation_id.clone(),
                    }))
                    .await?
                    .into_inner();
                if operation.state == "completed" || operation.state == "partial" {
                    let data = operation_json(operation)?;
                    if data["state"] == "partial" {
                        let providers = scan_failure_names(&data).join(", ");
                        return Err(command_error(
                            format!("scan completed with provider failures: {providers}"),
                            "SCAN_PARTIAL",
                            data,
                        ));
                    }
                    let result = &data["result"];
                    let entries = result["entries"].as_array().cloned().unwrap_or_default();
                    let header = format!("scan complete: {} installation(s)", entries.len());
                    return Ok((installation_list_text(&header, &entries), data));
                }
                if operation.state == "cancelled" {
                    return Err("scan was interrupted because the library closed".into());
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err("timed out waiting for scan; it continues in the daemon".into());
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
        ("games", "play") | ("games", "install") | ("games", "remove") => {
            let entry_key = rest
                .first()
                .ok_or_else(|| usage("game entry key required"))?
                .clone();
            let request = Request::new(GameRequest {
                entry_key: entry_key.clone(),
            });
            match action.as_str() {
                "play" => {
                    client.play_game(request).await?;
                }
                "install" => {
                    client.install_game(request).await?;
                }
                _ => {
                    client.remove_game(request).await?;
                }
            }
            Ok((
                format!("{action} requested"),
                json!({ "accepted": true, "entryKey": entry_key }),
            ))
        }
        ("games", "match") => {
            let entry_key = rest
                .first()
                .ok_or_else(|| usage("game entry key required"))?
                .clone();
            let game_id = rest
                .get(1)
                .ok_or_else(|| usage("GameNode game id required"))?
                .parse::<u64>()?;
            client
                .match_game(Request::new(MatchRequest {
                    entry_key: entry_key.clone(),
                    game_id,
                }))
                .await?;
            Ok((
                "match saved".into(),
                json!({ "entryKey": entry_key, "gameId": game_id.to_string() }),
            ))
        }
        ("games", "status") => {
            let entry_key = rest[0].clone();
            let status = requested_status(&rest)
                .expect("validated status option")
                .to_owned();
            let reply = client
                .add_game(Request::new(AddGameRequest {
                    entry_key: entry_key.clone(),
                    status: status.clone(),
                    collection_ids: Vec::new(),
                    status_only: true,
                    clear_collections: false,
                }))
                .await?
                .into_inner();
            let data = serde_json::from_str::<Value>(&reply.summary_json)?;
            Ok((
                format!("status set to {status}"),
                json!({ "entryKey": entry_key, "status": status, "result": data }),
            ))
        }
        ("games", "search") => {
            let entry_key = rest
                .first()
                .ok_or_else(|| usage("game entry key required"))?
                .clone();
            let query = rest.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
            if query.trim().is_empty() {
                return Err(usage("search query required"));
            }
            let reply = client
                .search_matches(Request::new(SearchMatchesRequest { entry_key, query }))
                .await?
                .into_inner();
            let data = serde_json::from_str::<Value>(&reply.results_json)?;
            Ok((serde_json::to_string_pretty(&data)?, data))
        }
        ("games", "add") => {
            let entry_key = rest
                .first()
                .ok_or_else(|| usage("game entry key required"))?
                .clone();
            let mut status = String::from("playing");
            let mut collection_ids = Vec::new();
            let mut index = 1;
            while index < rest.len() {
                let arg = &rest[index];
                if arg == "--collection" {
                    index += 1;
                    collection_ids.push(
                        rest.get(index)
                            .ok_or_else(|| usage("collection id required after --collection"))?
                            .clone(),
                    );
                } else if let Some(value) = arg.strip_prefix("--status=") {
                    status = value.to_owned();
                } else if arg == "--status" {
                    index += 1;
                    status = rest
                        .get(index)
                        .ok_or_else(|| usage("status required after --status"))?
                        .clone();
                } else if arg == "--no-collections" {
                    // Emptying the requested set is an explicit operation.
                } else if arg.starts_with('-') {
                    return Err(usage(format!("unknown option: {arg}")));
                } else {
                    status = arg.clone();
                }
                index += 1;
            }
            let reply = client
                .add_game(Request::new(AddGameRequest {
                    entry_key,
                    status,
                    collection_ids,
                    status_only: false,
                    clear_collections: rest.iter().any(|arg| arg == "--no-collections"),
                }))
                .await?
                .into_inner();
            let data = serde_json::from_str::<Value>(&reply.summary_json)?;
            Ok((serde_json::to_string_pretty(&data)?, data))
        }
        ("games", "no-match") | ("games", "clear-match") => {
            let entry_key = rest
                .first()
                .ok_or_else(|| usage("game entry key required"))?
                .clone();
            let request = Request::new(GameRequest {
                entry_key: entry_key.clone(),
            });
            if action == "no-match" {
                client.mark_no_game(request).await?;
            } else {
                client.clear_match(request).await?;
            }
            Ok((
                format!("{} saved", action),
                json!({ "entryKey": entry_key }),
            ))
        }
        _ => Err(usage(format!(
            "unknown command: {group} {action}\n{}",
            help()
        ))),
    }
}

async fn get_installations(
    client: &mut proto::client_client::ClientClient<Channel>,
) -> Result<InstallationSnapshot, Box<dyn Error>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        match client.list_installations(Request::new(Empty {})).await {
            Ok(response) => return Ok(response.into_inner()),
            Err(status) if status.code() == Code::Unavailable => {
                if tokio::time::Instant::now() >= deadline {
                    return Err("timed out waiting for the local library to load".into());
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(status) => return Err(Box::new(status)),
        }
    }
}

fn validate_args(group: &str, action: &str, rest: &[String]) -> Result<(), Box<dyn Error>> {
    let exact = |count: usize| {
        if rest.len() == count {
            Ok(())
        } else {
            Err(usage(format!(
                "unexpected or missing arguments for {group} {action}"
            )))
        }
    };
    match (group, action) {
        ("daemon", "status" | "stop")
        | ("auth", "status" | "logout")
        | ("games", "list" | "scan") => exact(0),
        ("auth", "login") => {
            let provider = if rest.first().is_some_and(|arg| arg == "--provider") {
                if rest.len() != 2 {
                    return Err(usage(
                        "usage: gamenode-cli auth login --provider <google|discord|twitter>",
                    ));
                }
                &rest[1]
            } else {
                if rest.len() != 1 {
                    return Err(usage(
                        "usage: gamenode-cli auth login <google|discord|twitter>",
                    ));
                }
                &rest[0]
            };
            if !matches!(provider.as_str(), "google" | "discord" | "twitter") {
                return Err(usage(
                    "usage: gamenode-cli auth login <google|discord|twitter>",
                ));
            }
            Ok(())
        }
        ("settings", "sync") => {
            if rest.len() != 1 || !matches!(rest[0].as_str(), "on" | "off") {
                return Err(usage("usage: gamenode-cli settings sync <on|off>"));
            }
            Ok(())
        }
        ("settings", "playtime-source") => {
            if rest.len() != 1 || !matches!(rest[0].as_str(), "store" | "internal") {
                return Err(usage(
                    "usage: gamenode-cli settings playtime-source <store|internal>",
                ));
            }
            Ok(())
        }
        ("games", "play" | "install" | "remove" | "no-match" | "clear-match") => {
            if rest.len() != 1 {
                return Err(usage("exactly one game entry key is required"));
            }
            Ok(())
        }
        ("games", "match") => {
            if rest.len() != 2 {
                return Err(usage(
                    "usage: gamenode-cli games match <entry-key> <game-id>",
                ));
            }
            rest[1]
                .parse::<u64>()
                .map(|_| ())
                .map_err(|_| usage("GameNode game id must be an unsigned integer"))
        }
        ("operations", "status") => {
            if rest.len() != 1 {
                return Err(usage(
                    "usage: gamenode-cli operations status <operation-id>",
                ));
            }
            Ok(())
        }
        ("games", "status") => {
            let syntax_ok = matches!(
                rest,
                [_, option] if option.starts_with("--status=")
            ) || matches!(rest, [_, option, _] if option == "--status");
            if !syntax_ok {
                return Err(usage(
                    "usage: gamenode-cli games status <entry-key> --status <status>",
                ));
            }
            let status = requested_status(rest).unwrap();
            if !valid_status(status) {
                return Err(usage(
                    "invalid status; use playing, finished, planned, dropped, or ongoing",
                ));
            }
            Ok(())
        }
        ("games", "search") => {
            if rest.len() < 2 {
                return Err(usage(
                    "usage: gamenode-cli games search <entry-key> <query>",
                ));
            }
            Ok(())
        }
        ("games", "add") => {
            if rest.is_empty() {
                return Err(usage(
                    "usage: gamenode-cli games add <entry-key> [status] [--collection id]",
                ));
            }
            let mut index = 1;
            while index < rest.len() {
                if rest[index] == "--collection" {
                    index += 1;
                    if index == rest.len() {
                        return Err(usage("collection id required after --collection"));
                    }
                } else if rest[index] == "--no-collections" {
                    if rest.iter().any(|arg| arg == "--collection") {
                        return Err(usage(
                            "--no-collections cannot be combined with --collection",
                        ));
                    }
                } else if rest[index] == "--status" {
                    index += 1;
                    let Some(status) = rest.get(index).map(String::as_str) else {
                        return Err(usage("status required after --status"));
                    };
                    if !valid_status(status) {
                        return Err(usage("invalid game status"));
                    }
                } else if let Some(status) = rest[index].strip_prefix("--status=") {
                    if !valid_status(status) {
                        return Err(usage("invalid game status"));
                    }
                } else if rest[index].starts_with('-') {
                    return Err(usage(format!("unknown option: {}", rest[index])));
                } else if !valid_status(&rest[index]) {
                    return Err(usage(
                        "invalid status; use playing, finished, planned, dropped, or ongoing",
                    ));
                }
                index += 1;
            }
            Ok(())
        }
        _ => Err(usage(format!(
            "unknown command: {group} {action}\n{}",
            help()
        ))),
    }
}

fn auth_json(s: AuthStatus) -> Value {
    json!({ "state": s.state, "account": s.account, "syncEnabled": s.sync_enabled,
        "playtimeSource": s.playtime_source })
}

fn operation_json(operation: OperationStatus) -> Result<Value, Box<dyn Error>> {
    let result = if operation.result_json.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&operation.result_json)?
    };
    Ok(json!({
        "operationId": operation.operation_id,
        "kind": operation.kind,
        "state": operation.state,
        "result": result,
    }))
}

fn scan_failure_names(operation: &Value) -> Vec<String> {
    ["failedProviders", "persistenceFailures"]
        .into_iter()
        .flat_map(|field| {
            operation["result"][field]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

fn requested_status(args: &[String]) -> Option<&str> {
    args.iter().enumerate().find_map(|(index, arg)| {
        arg.strip_prefix("--status=").or_else(|| {
            if arg == "--status" {
                args.get(index + 1).map(String::as_str)
            } else {
                None
            }
        })
    })
}

fn valid_status(status: &str) -> bool {
    matches!(
        status,
        "playing" | "finished" | "planned" | "dropped" | "ongoing"
    )
}

fn installation_list_text(header: &str, entries: &[Value]) -> String {
    let mut lines = vec![header.to_owned()];
    lines.extend(entries.iter().map(|entry| {
        format!(
            "{}\t{}\t{}\t{}",
            entry["entryKey"].as_str().unwrap_or_default(),
            entry["title"].as_str().unwrap_or_default(),
            entry["match"],
            entry["installDir"].as_str().unwrap_or_default(),
        )
    }));
    lines.join("\n")
}

async fn wait_daemon_stopped(socket: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        if connect(socket.to_path_buf()).await.is_err() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("daemon accepted stop but has not exited".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn start_daemon() -> Result<bool, Box<dyn Error>> {
    if connect(socket_path()?).await.is_ok() {
        return Ok(false);
    }
    let executable = std::env::current_exe()?;
    let daemon = executable.with_file_name(if cfg!(windows) {
        "gamenode-daemon.exe"
    } else {
        "gamenode-daemon"
    });
    let mut child = Command::new(daemon)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or("could not capture daemon output")?;
    let stderr_task = std::thread::spawn(move || {
        let mut output = Vec::new();
        let _ = stderr.read_to_end(&mut output);
        String::from_utf8_lossy(&output).trim().to_owned()
    });
    let socket = socket_path()?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if connect(socket.clone()).await.is_ok() {
            return Ok(true);
        }
        if let Some(status) = child.try_wait()? {
            if connect(socket.clone()).await.is_ok() {
                let _ = stderr_task.join();
                return Ok(false);
            }
            let details = stderr_task.join().unwrap_or_default();
            return Err(if details.is_empty() {
                format!("daemon exited before becoming ready ({status})")
            } else {
                format!("daemon exited before becoming ready ({status}): {details}")
            }
            .into());
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let details = stderr_task.join().unwrap_or_default();
            return Err(if details.is_empty() {
                "daemon did not become ready within 30 seconds".to_owned()
            } else {
                format!("daemon did not become ready within 30 seconds: {details}")
            }
            .into());
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

fn socket_path() -> Result<PathBuf, Box<dyn Error>> {
    #[cfg(unix)]
    {
        Ok(gamenode_platform::ipc::Endpoint::for_current_user()?
            .dir()
            .join("client.sock"))
    }
    #[cfg(not(unix))]
    {
        Err("the CLI transport currently requires Unix domain sockets".into())
    }
}

fn help() -> String {
    "GameNode CLI\n  daemon start|status|stop\n  auth status|login <google|discord|twitter>|login --provider <provider>|logout\n  operations status <operation-id>\n  settings sync <on|off>|playtime-source <store|internal>\n  games list|scan [--no-wait]|play|install|remove <entry-key>|search <entry-key> <query>|match <entry-key> <game-id>|add <entry-key> [status] [--collection id]|status <entry-key> --status <status>|no-match|clear-match <entry-key>\nOptions: --json, --no-wait, --no-collections".into()
}
