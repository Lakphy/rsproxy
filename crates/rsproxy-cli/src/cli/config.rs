mod dns;
mod file;

use super::api_auth::validate_api_token;
use super::command::RuntimeArgs;
use super::util::{parse_size, parse_trace_spill_compression};
use crate::app::{AppConfig, default_storage};
use crate::{CliResult, ConfigError};
use file::FileConfig;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

const CONFIG_FILE_NAME: &str = "config.toml";

pub(crate) fn runtime_config(args: &RuntimeArgs) -> CliResult<AppConfig> {
    runtime_config_with_default_path(args, Some(default_config_path()))
}

#[cfg(test)]
pub(super) fn runtime_config_without_default(args: &RuntimeArgs) -> CliResult<AppConfig> {
    runtime_config_with_default_path(args, None)
}

pub(super) fn runtime_config_with_default_path(
    args: &RuntimeArgs,
    default_path: Option<PathBuf>,
) -> CliResult<AppConfig> {
    let mut config = AppConfig::default();
    let mut api_explicit = false;
    if let Some(path) = selected_config_path(args, default_path) {
        let text = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        let file: FileConfig = toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.clone(),
            source,
        })?;
        api_explicit = file.has_explicit_api();
        file.apply(&mut config)?;
        config.config_path = Some(path);
    }
    apply_cli_overrides(args, &mut config, api_explicit)?;
    validate_mitm_mode(&config)?;
    Ok(config)
}

fn selected_config_path(args: &RuntimeArgs, default_path: Option<PathBuf>) -> Option<PathBuf> {
    args.client
        .config
        .clone()
        .or_else(|| default_path.filter(|path| path.is_file()))
}

fn default_config_path() -> PathBuf {
    default_storage().join(CONFIG_FILE_NAME)
}

fn apply_cli_overrides(
    args: &RuntimeArgs,
    config: &mut AppConfig,
    file_api_explicit: bool,
) -> CliResult<()> {
    if let Some(port) = args.port {
        config.port = port;
    }
    if let Some(host) = &args.host {
        config.host.clone_from(host);
    }
    if let Some(api) = &args.client.api {
        config.api.clone_from(api);
    }
    if let Some(storage) = &args.client.storage {
        config.engine_mut().storage.clone_from(storage);
    }
    if args.client.api.is_none() && !file_api_explicit {
        config.api = crate::app::default_api_for_storage(&config.engine().storage);
    }
    if args.watch {
        config.engine_mut().rules_watch = true;
    }
    if let Some(value) = args.watch_debounce_ms {
        config.engine_mut().rules_watch_debounce = positive_millis(value, "--watch-debounce-ms")?;
    }
    if let Some(token) = &args.client.api_token {
        config.api_token = Some(validate_api_token(token)?);
    }
    if let Some(auth) = &args.proxy_auth {
        config.engine_mut().proxy_auth = Some(parse_proxy_auth(auth)?);
    }
    if let Some(limit) = &args.max_header_size {
        config.engine_mut().max_header_size = parse_size(limit)?;
    }
    if let Some(limit) = args.max_header_count {
        config.engine_mut().max_header_count = positive_usize(limit, "--max-header-count")?;
    }
    if let Some(limit) = &args.body_buffer_limit {
        config.engine_mut().body_buffer_limit =
            positive_size(parse_size(limit)?, "--body-buffer-limit")?;
    }
    if let Some(limit) = &args.trace_body_limit {
        config.engine_mut().trace_body_limit = parse_size(limit)?;
    }
    if let Some(filter) = &args.trace_filter {
        apply_trace_filter(config, filter)?;
    }
    if let Some(capacity) = args.trace_queue_capacity {
        config.engine_mut().trace_queue_capacity =
            positive_usize(capacity, "--trace-queue-capacity")?;
    }
    if let Some(budget) = &args.trace_mem_budget {
        config.engine_mut().trace_memory_budget =
            positive_size(parse_size(budget)?, "--trace-mem-budget")?;
    }
    if let Some(size) = &args.trace_segment_size {
        config.engine_mut().trace_spill_segment_size =
            positive_size(parse_size(size)?, "--trace-segment-size")?;
    }
    if let Some(budget) = &args.trace_disk_budget {
        config.engine_mut().trace_disk_budget = parse_size(budget)?;
    }
    if let Some(compression) = &args.trace_spill_compression {
        config.engine_mut().trace_spill_compression = parse_trace_spill_compression(compression)?;
    }
    if args.no_mitm {
        config.engine_mut().no_mitm = true;
    }
    if args.strict_mitm {
        config.engine_mut().strict_mitm = true;
    }
    if let Some(capacity) = args.mitm_cert_cache_capacity {
        config.engine_mut().mitm_cert_cache_capacity = capacity;
    }
    if let Some(capacity) = args.mitm_failure_cache_capacity {
        config.engine_mut().mitm_failure_cache_capacity = capacity;
    }
    if let Some(ttl) = args.mitm_failure_ttl_seconds {
        config.engine_mut().mitm_failure_ttl = positive_seconds(ttl, "--mitm-failure-ttl-seconds")?;
    }
    if let Some(timeout) = args.connect_probe_timeout_ms {
        config.engine_mut().connect_probe_timeout =
            positive_millis(timeout, "--connect-probe-timeout-ms")?;
    }
    if let Some(limit) = args.h1_pool_max_active_per_key {
        config.engine_mut().h1_pool_max_active_per_key =
            positive_usize(limit, "--h1-pool-max-active-per-key")?;
    }
    if let Some(timeout) = args.h1_pool_wait_timeout_ms {
        config.engine_mut().h1_pool_wait_timeout =
            positive_millis(timeout, "--h1-pool-wait-timeout-ms")?;
    }
    if let Some(limit) = args.h2_pool_max_active_streams_per_key {
        config.engine_mut().h2_pool_max_active_streams_per_key =
            positive_usize(limit, "--h2-pool-max-active-streams-per-key")?;
    }
    if let Some(timeout) = args.h2_pool_wait_timeout_ms {
        config.engine_mut().h2_pool_wait_timeout =
            positive_millis(timeout, "--h2-pool-wait-timeout-ms")?;
    }
    if let Some(timeout) = args.tcp_connect_timeout_ms {
        config.engine_mut().tcp_connect_timeout =
            positive_millis(timeout, "--tcp-connect-timeout-ms")?;
    }
    if let Some(timeout) = args.dns_timeout_ms {
        config.engine_mut().dns_timeout = positive_millis(timeout, "--dns-timeout-ms")?;
    }
    if let Some(ttl) = args.dns_cache {
        config.engine_mut().dns_cache_ttl = Duration::from_secs(ttl);
    }
    if !args.dns_server.is_empty() {
        config.engine_mut().dns_servers = dns::parse_dns_servers(&args.dns_server)?;
    }
    if let Some(timeout) = args.client_tls_handshake_timeout_ms {
        config.engine_mut().client_tls_handshake_timeout =
            positive_millis(timeout, "--client-tls-handshake-timeout-ms")?;
    }
    if let Some(timeout) = args.upstream_tls_handshake_timeout_ms {
        config.engine_mut().upstream_tls_handshake_timeout =
            positive_millis(timeout, "--upstream-tls-handshake-timeout-ms")?;
    }
    if let Some(timeout) = args.upstream_ttfb_timeout_ms {
        config.engine_mut().upstream_ttfb_timeout =
            positive_millis(timeout, "--upstream-ttfb-timeout-ms")?;
    }
    if let Some(timeout) = args.request_timeout_ms {
        config.engine_mut().request_total_timeout =
            positive_millis(timeout, "--request-timeout-ms")?;
    }
    if args.no_trace_body {
        config.engine_mut().trace_body_limit = 0;
    }
    Ok(())
}

fn positive_millis(value: u64, field: &str) -> Result<Duration, ConfigError> {
    (value != 0)
        .then(|| Duration::from_millis(value))
        .ok_or_else(|| ConfigError::Invalid(format!("{field} must be greater than zero")))
}

fn positive_seconds(value: u64, field: &str) -> Result<Duration, ConfigError> {
    (value != 0)
        .then(|| Duration::from_secs(value))
        .ok_or_else(|| ConfigError::Invalid(format!("{field} must be greater than zero")))
}

fn positive_usize(value: usize, field: &str) -> Result<usize, ConfigError> {
    (value != 0)
        .then_some(value)
        .ok_or_else(|| ConfigError::Invalid(format!("{field} must be greater than zero")))
}

fn positive_size(value: usize, field: &str) -> Result<usize, ConfigError> {
    positive_usize(value, field)
}

fn validate_mitm_mode(config: &AppConfig) -> Result<(), ConfigError> {
    if config.engine().no_mitm && config.engine().strict_mitm {
        Err(ConfigError::Invalid(
            "--no-mitm and --strict-mitm cannot be used together".to_string(),
        ))
    } else {
        Ok(())
    }
}

fn parse_proxy_auth(input: &str) -> Result<String, ConfigError> {
    let Some((username, password)) = input.split_once(':') else {
        return Err(ConfigError::Invalid(
            "--proxy-auth must use user:pass format".to_string(),
        ));
    };
    if username.is_empty() || password.is_empty() {
        return Err(ConfigError::Invalid(
            "--proxy-auth username and password must not be empty".to_string(),
        ));
    }
    if username.chars().any(char::is_control) || password.chars().any(char::is_control) {
        return Err(ConfigError::Invalid(
            "--proxy-auth must not contain control characters".to_string(),
        ));
    }
    Ok(input.to_string())
}

fn apply_trace_filter(config: &mut AppConfig, input: &str) -> Result<(), ConfigError> {
    for raw in input.split(',') {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" => {}
            "headers-only" | "headers_only" | "headers" | "no-body" | "no_body" => {
                config.engine_mut().trace_body_limit = 0;
            }
            "media" | "media-body-off" | "media_body_off" | "no-media-body" | "no_media_body"
            | "exclude-media" | "exclude_media" => {
                config.engine_mut().trace_exclude_media_body = true;
            }
            "full" | "all" => config.engine_mut().trace_exclude_media_body = false,
            _ => {
                return Err(ConfigError::Invalid(
                    "--trace-filter supports headers-only, media, or full in this build"
                        .to_string(),
                ));
            }
        }
    }
    Ok(())
}
