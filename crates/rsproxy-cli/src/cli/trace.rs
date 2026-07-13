use super::command::{ReplayArgs, RuntimeArgs, TraceArgs, TraceCommand, ValuesArgs, ValuesCommand};
use super::config::runtime_config;
use super::util::{percent_encode, read_stdin};
use crate::{CliError, CliResult};
use rsproxy_control::{api_request, api_stream_lines};
use std::fs;

pub(super) fn trace_cmd(args: TraceArgs, json: bool) -> CliResult<()> {
    let config = runtime_config(&RuntimeArgs::from_client(args.client))?;
    let api = config.api.clone();
    match args.command {
        TraceCommand::List(args) => {
            let endpoint = if json {
                "/api/sessions"
            } else {
                "/api/sessions.txt"
            };
            println!(
                "{}",
                api_request("GET", &api, &format!("{endpoint}?limit={}", args.limit), "",)?
            );
            Ok(())
        }
        TraceCommand::Get(args) => {
            println!(
                "{}",
                api_request("GET", &api, &format!("/api/sessions/{}", args.id), "")?
            );
            Ok(())
        }
        TraceCommand::Stats(_) => {
            println!("{}", api_request("GET", &api, "/api/trace/stats", "")?);
            Ok(())
        }
        TraceCommand::Clear(_) => {
            println!("{}", api_request("POST", &api, "/api/trace/clear", "")?);
            Ok(())
        }
        TraceCommand::Follow(args) => {
            let count = args.count.unwrap_or(usize::MAX);
            let poll_ms = args.poll_ms.unwrap_or(500);
            if count == 0 {
                return Ok(());
            }
            let mut seen = 0usize;
            api_stream_lines(
                &api,
                &format!(
                    "/api/sessions/follow?after=0&limit=100&heartbeat_ms={}",
                    poll_ms.clamp(100, 30_000)
                ),
                |line| {
                    println!("{line}");
                    seen += 1;
                    seen < count
                },
            )?;
            Ok(())
        }
        TraceCommand::Export(args) => {
            let endpoint = if args.har {
                "/api/sessions/export.har"
            } else {
                "/api/sessions/export.json"
            };
            let body = api_request("GET", &api, endpoint, "")?;
            if let Some(file) = args.output {
                fs::write(&file, body).map_err(|source| {
                    CliError::io(format!("write trace export {}", file.display()), source)
                })?;
                println!("wrote {}", file.display());
            } else {
                println!("{body}");
            }
            Ok(())
        }
    }
}

pub(super) fn values_cmd(args: ValuesArgs, json: bool) -> CliResult<()> {
    let config = runtime_config(&RuntimeArgs::from_client(args.client))?;
    let api = config.api.clone();
    let storage = config.engine().storage.clone();
    match args.command {
        ValuesCommand::List(_) => match api_request(
            "GET",
            &api,
            if json {
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
                if json {
                    println!(
                        "{}",
                        serde_json::to_string(&names).map_err(|source| CliError::Json {
                            context: "serialize value names",
                            source,
                        })?
                    );
                } else {
                    for name in names {
                        println!("{name}");
                    }
                }
                Ok(())
            }
        },
        ValuesCommand::Cat(args) => {
            let value = match api_request(
                "GET",
                &api,
                &format!("/api/values/{}", percent_encode(&args.key)),
                "",
            ) {
                Ok(body) => body,
                Err(_) => {
                    let path = storage.join("values").join(&args.key);
                    fs::read_to_string(&path).map_err(|source| {
                        CliError::io(format!("read value file {}", path.display()), source)
                    })?
                }
            };
            if json {
                println!("{}", serde_json::json!({"key": args.key, "value": value}));
            } else {
                print!("{value}");
            }
            Ok(())
        }
        ValuesCommand::Set(args) => {
            let body = if let Some(file) = args.file {
                fs::read_to_string(&file).map_err(|source| {
                    CliError::io(format!("read value input {}", file.display()), source)
                })?
            } else {
                read_stdin()?
            };
            let values_dir = storage.join("values");
            fs::create_dir_all(&values_dir).map_err(|source| {
                CliError::io(
                    format!("create values directory {}", values_dir.display()),
                    source,
                )
            })?;
            let value_path = values_dir.join(&args.key);
            fs::write(&value_path, &body).map_err(|source| {
                CliError::io(format!("write value file {}", value_path.display()), source)
            })?;
            match api_request(
                "PUT",
                &api,
                &format!("/api/values/{}", percent_encode(&args.key)),
                &body,
            ) {
                Ok(response) => println!("{response}"),
                Err(_) => println!("saved value {} to {}", args.key, storage.display()),
            }
            Ok(())
        }
        ValuesCommand::Remove(args) => {
            let _ = fs::remove_file(storage.join("values").join(&args.key));
            match api_request(
                "DELETE",
                &api,
                &format!("/api/values/{}", percent_encode(&args.key)),
                "",
            ) {
                Ok(response) => println!("{response}"),
                Err(_) => println!("removed value {} from {}", args.key, storage.display()),
            }
            Ok(())
        }
    }
}

pub(super) fn replay_cmd(args: ReplayArgs) -> CliResult<()> {
    let api = runtime_config(&RuntimeArgs::from_client(args.client))?.api;
    println!(
        "{}",
        api_request("POST", &api, &format!("/api/replay/{}", args.id), "")?
    );
    Ok(())
}
