use super::*;

pub(super) fn trace_cmd(mut args: Vec<String>) -> Result<(), String> {
    if args.is_empty() {
        return Err("trace command required".to_string());
    }
    let sub = args.remove(0);
    let api = runtime_config(&args)?.api;
    match sub.as_str() {
        "ls" => {
            let limit = trace_list_limit(&args);
            let endpoint = if has_flag(&args, "--json") {
                "/api/sessions"
            } else {
                "/api/sessions.txt"
            };
            println!(
                "{}",
                api_request("GET", &api, &format!("{endpoint}?limit={limit}"), "")?
            );
            Ok(())
        }
        "get" => {
            let id = client_positional(&args).ok_or_else(|| "trace get requires id".to_string())?;
            println!(
                "{}",
                api_request("GET", &api, &format!("/api/sessions/{id}"), "")?
            );
            Ok(())
        }
        "stats" => {
            println!("{}", api_request("GET", &api, "/api/trace/stats", "")?);
            Ok(())
        }
        "clear" => {
            println!("{}", api_request("POST", &api, "/api/trace/clear", "")?);
            Ok(())
        }
        "follow" => trace_follow(args, &api),
        "export" => {
            let har = has_flag(&args, "--har");
            let endpoint = if har {
                "/api/sessions/export.har"
            } else {
                "/api/sessions/export.json"
            };
            let body = api_request("GET", &api, endpoint, "")?;
            if let Some(file) =
                option_value(&args, "-o").or_else(|| option_value(&args, "--output"))
            {
                fs::write(&file, body).map_err(|e| e.to_string())?;
                println!("wrote {file}");
            } else {
                println!("{body}");
            }
            Ok(())
        }
        _ => Err(format!("unknown trace command `{sub}`")),
    }
}

pub(super) fn trace_list_limit(args: &[String]) -> String {
    option_value(args, "-n")
        .or_else(|| option_value(args, "--limit"))
        .unwrap_or_else(|| "20".to_string())
}

pub(super) fn trace_follow(args: Vec<String>, api: &str) -> Result<(), String> {
    let count = option_value(&args, "--count")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(usize::MAX);
    let poll_ms = option_value(&args, "--poll-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(500);
    if count == 0 {
        return Ok(());
    }
    let mut seen = 0usize;
    api_stream_lines(
        api,
        &format!(
            "/api/sessions/follow?after=0&limit=100&heartbeat_ms={}",
            poll_ms.clamp(100, 30_000)
        ),
        |line| {
            println!("{line}");
            seen += 1;
            Ok(seen < count)
        },
    )
}

pub(super) fn values_cmd(mut args: Vec<String>) -> Result<(), String> {
    if args.is_empty() {
        return Err("values command required".to_string());
    }
    let sub = args.remove(0);
    let config = runtime_config(&args)?;
    let api = config.api;
    let storage = config.storage;
    match sub.as_str() {
        "ls" => match api_request(
            "GET",
            &api,
            if has_flag(&args, "--json") {
                "/api/values"
            } else {
                "/api/values.txt"
            },
            "",
        ) {
            Ok(body) => {
                println!("{body}");
                Ok(())
            }
            Err(_) => {
                let dir = storage.join("values");
                let mut names = Vec::new();
                if let Ok(entries) = fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        if entry.file_type().map(|ty| ty.is_file()).unwrap_or(false) {
                            names.push(entry.file_name().to_string_lossy().into_owned());
                        }
                    }
                }
                names.sort();
                if has_flag(&args, "--json") {
                    println!(
                        "{}",
                        serde_json::to_string(&names).map_err(|error| error.to_string())?
                    );
                } else {
                    for name in names {
                        println!("{name}");
                    }
                }
                Ok(())
            }
        },
        "cat" => {
            let key =
                client_positional(&args).ok_or_else(|| "values cat requires key".to_string())?;
            let value = match api_request(
                "GET",
                &api,
                &format!("/api/values/{}", percent_encode(&key)),
                "",
            ) {
                Ok(body) => body,
                Err(_) => fs::read_to_string(storage.join("values").join(&key))
                    .map_err(|e| e.to_string())?,
            };
            if has_flag(&args, "--json") {
                println!("{}", serde_json::json!({"key": key, "value": value}));
            } else {
                print!("{value}");
            }
            Ok(())
        }
        "set" => {
            let key =
                client_positional(&args).ok_or_else(|| "values set requires key".to_string())?;
            let body = if let Some(file) = option_value(&args, "--file") {
                fs::read_to_string(file).map_err(|e| e.to_string())?
            } else {
                read_stdin()?
            };
            fs::create_dir_all(storage.join("values")).map_err(|e| e.to_string())?;
            fs::write(storage.join("values").join(&key), &body).map_err(|e| e.to_string())?;
            match api_request(
                "PUT",
                &api,
                &format!("/api/values/{}", percent_encode(&key)),
                &body,
            ) {
                Ok(resp) => println!("{resp}"),
                Err(_) => println!("saved value {key} to {}", storage.display()),
            }
            Ok(())
        }
        "rm" => {
            let key =
                client_positional(&args).ok_or_else(|| "values rm requires key".to_string())?;
            let _ = fs::remove_file(storage.join("values").join(&key));
            match api_request(
                "DELETE",
                &api,
                &format!("/api/values/{}", percent_encode(&key)),
                "",
            ) {
                Ok(resp) => println!("{resp}"),
                Err(_) => println!("removed value {key} from {}", storage.display()),
            }
            Ok(())
        }
        _ => Err(format!("unknown values command `{sub}`")),
    }
}

pub(super) fn replay_cmd(args: Vec<String>) -> Result<(), String> {
    let api = runtime_config(&args)?.api;
    let id = client_positional(&args).ok_or_else(|| "replay requires session id".to_string())?;
    println!(
        "{}",
        api_request("POST", &api, &format!("/api/replay/{id}"), "")?
    );
    Ok(())
}

fn client_positional(args: &[String]) -> Option<String> {
    positional_skipping_values(
        args,
        &[
            "--api",
            "--api-token",
            "--config",
            "--storage",
            "--file",
            "-o",
            "--output",
            "-n",
            "--limit",
            "--count",
            "--poll-ms",
        ],
    )
}
