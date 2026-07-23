use super::command::{ReplayArgs, RuntimeArgs, TraceArgs, TraceCommand, ValuesArgs, ValuesCommand};
use super::config::runtime_config;
use super::util::{percent_encode, read_stdin_bounded, read_utf8_file_bounded};
use crate::app::AppConfig;
use crate::{CliError, CliResult};
use rsproxy_control::{api_request, api_request_with_timeout, api_stream_lines};
use rsproxy_rules::MAX_RULE_EXTERNAL_VALUE_BYTES;
use std::fs;
use std::time::Duration;

/// Default number of recent sessions listed by `trace` with no subcommand,
/// matching the `--limit` default of `trace ls`.
const DEFAULT_TRACE_LIST_LIMIT: usize = 20;

pub(super) fn trace_cmd(args: TraceArgs, json: bool) -> CliResult<()> {
    let config = runtime_config(&RuntimeArgs::from_client(args.client))?;
    let api = config.api.clone();
    // `rsproxy trace` with no subcommand defaults to listing recent sessions,
    // matching the default-status behavior of `ca` and `proxy`.
    let Some(command) = args.command else {
        return run_trace_list(&api, DEFAULT_TRACE_LIST_LIMIT, json);
    };
    match command {
        TraceCommand::List(args) => run_trace_list(&api, args.limit, json),
        TraceCommand::Get(args) => {
            let body = api_request("GET", &api, &format!("/api/sessions/{}", args.id), "")?;
            println!("{}", super::output::trace_detail(&body, json)?);
            Ok(())
        }
        TraceCommand::Stats(_) => {
            let body = api_request("GET", &api, "/api/trace/stats", "")?;
            println!("{}", super::output::trace_stats(&body, json)?);
            Ok(())
        }
        TraceCommand::Clear(_) => {
            let body = api_request("POST", &api, "/api/trace/clear", "")?;
            println!(
                "{}",
                super::output::mutation(&body, json, "cleared captured sessions")?
            );
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
        TraceCommand::Replay(args) => replay_with_config(&config, &args.id, json),
    }
}

fn run_trace_list(api: &str, limit: usize, json: bool) -> CliResult<()> {
    let endpoint = if json {
        "/api/sessions"
    } else {
        "/api/sessions.txt"
    };
    println!(
        "{}",
        api_request("GET", api, &format!("{endpoint}?limit={limit}"), "")?
    );
    Ok(())
}

pub(super) fn values_cmd(args: ValuesArgs, json: bool) -> CliResult<()> {
    let config = runtime_config(&RuntimeArgs::from_client(args.client))?;
    let api = config.api.clone();
    let storage = config.engine().storage.clone();
    // `rsproxy values` with no subcommand defaults to listing stored values,
    // matching the default-status behavior of `ca` and `proxy`.
    let Some(command) = args.command else {
        return run_values_list(&api, &storage, json);
    };
    match command {
        ValuesCommand::List(_) => run_values_list(&api, &storage, json),
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
                    read_utf8_file_bounded(&path, MAX_RULE_EXTERNAL_VALUE_BYTES, "rule value")?
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
                read_utf8_file_bounded(&file, MAX_RULE_EXTERNAL_VALUE_BYTES, "rule value")?
            } else {
                read_stdin_bounded(MAX_RULE_EXTERNAL_VALUE_BYTES, "rule value")?
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
                Ok(response) => println!(
                    "{}",
                    super::output::mutation(&response, json, &format!("saved value {}", args.key))?
                ),
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
                Ok(response) => println!(
                    "{}",
                    super::output::mutation(
                        &response,
                        json,
                        &format!("removed value {}", args.key)
                    )?
                ),
                Err(_) => println!("removed value {} from {}", args.key, storage.display()),
            }
            Ok(())
        }
    }
}

fn run_values_list(api: &str, storage: &std::path::Path, json: bool) -> CliResult<()> {
    match api_request(
        "GET",
        api,
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
    }
}

pub(super) fn replay_cmd(args: ReplayArgs, json: bool) -> CliResult<()> {
    let config = runtime_config(&RuntimeArgs::from_client(args.client))?;
    replay_with_config(&config, &args.id, json)
}

/// Replays one captured session against the resolved control endpoint. Shared by
/// the top-level `replay` command and the trace-scoped `trace replay` subcommand.
fn replay_with_config(config: &AppConfig, id: &str, json: bool) -> CliResult<()> {
    let timeout = config
        .engine()
        .request_total_timeout
        .saturating_add(Duration::from_secs(1));
    let body = api_request_with_timeout(
        "POST",
        &config.api,
        &format!("/api/replay/{id}"),
        "",
        timeout,
    )?;
    println!("{}", super::output::replay(&body, json)?);
    Ok(())
}
