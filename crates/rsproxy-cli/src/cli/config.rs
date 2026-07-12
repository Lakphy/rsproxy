use super::*;

mod file;

use file::FileConfig;

const CONFIG_FILE_NAME: &str = "config.toml";

pub(crate) fn runtime_config(args: &[String]) -> Result<AppConfig, String> {
    runtime_config_with_default_path(args, Some(default_config_path()))
}

#[cfg(test)]
pub(super) fn runtime_config_without_default(args: &[String]) -> Result<AppConfig, String> {
    runtime_config_with_default_path(args, None)
}

pub(super) fn runtime_config_with_default_path(
    args: &[String],
    default_path: Option<PathBuf>,
) -> Result<AppConfig, String> {
    let mut config = AppConfig::default();
    let mut api_explicit = false;
    if let Some(path) = selected_config_path(args, default_path)? {
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("read config {}: {error}", path.display()))?;
        let file: FileConfig = toml::from_str(&text)
            .map_err(|error| format!("parse config {}: {error}", path.display()))?;
        api_explicit = file.has_explicit_api();
        file.apply(&mut config)?;
        config.config_path = Some(path);
    }
    apply_cli_overrides(args, &mut config, api_explicit)?;
    validate_mitm_mode(&config)?;
    Ok(config)
}

fn selected_config_path(
    args: &[String],
    default_path: Option<PathBuf>,
) -> Result<Option<PathBuf>, String> {
    if let Some(index) = args.iter().position(|arg| arg == "--config") {
        let value = args
            .get(index + 1)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "--config requires a file path".to_string())?;
        return Ok(Some(PathBuf::from(value)));
    }
    Ok(default_path.filter(|path| path.is_file()))
}

fn default_config_path() -> PathBuf {
    default_storage().join(CONFIG_FILE_NAME)
}

fn apply_cli_overrides(
    args: &[String],
    config: &mut AppConfig,
    file_api_explicit: bool,
) -> Result<(), String> {
    if let Some(port) = option_value(args, "--port").or_else(|| option_value(args, "-p")) {
        config.port = port.parse().map_err(|_| "invalid --port".to_string())?;
    }
    if let Some(host) = option_value(args, "--host") {
        config.host = host;
    }
    let cli_api = option_value(args, "--api");
    if let Some(api) = &cli_api {
        config.api = api.clone();
    }
    if let Some(storage) = option_value(args, "--storage") {
        config.storage = PathBuf::from(storage);
    }
    if cli_api.is_none() && !file_api_explicit {
        config.api = crate::app::default_api_for_storage(&config.storage);
    }
    if has_flag(args, "--watch") {
        config.rules_watch = true;
    }
    if let Some(value) = option_value(args, "--watch-debounce-ms") {
        config.rules_watch_debounce = parse_cli_millis(&value, "--watch-debounce-ms")?;
    }
    if let Some(token) = option_value(args, "--api-token") {
        config.api_token = Some(validate_api_token(&token)?);
    }
    if let Some(auth) = option_value(args, "--proxy-auth") {
        config.proxy_auth = Some(parse_proxy_auth(&auth)?);
    }
    if let Some(limit) = option_value(args, "--max-header-size") {
        config.max_header_size = parse_size(&limit)?;
    }
    if let Some(limit) = option_value(args, "--max-header-count") {
        config.max_header_count = parse_positive_usize(&limit, "--max-header-count")?;
    }
    if let Some(limit) = option_value(args, "--body-buffer-limit") {
        config.body_buffer_limit = positive_size(parse_size(&limit)?, "--body-buffer-limit")?;
    }
    if let Some(limit) = option_value(args, "--trace-body-limit") {
        config.trace_body_limit = parse_size(&limit)?;
    }
    if let Some(filter) = option_value(args, "--trace-filter") {
        apply_trace_filter(config, &filter)?;
    }
    if let Some(capacity) = option_value(args, "--trace-queue-capacity") {
        config.trace_queue_capacity = parse_positive_usize(&capacity, "--trace-queue-capacity")?;
    }
    if let Some(budget) = option_value(args, "--trace-mem-budget") {
        config.trace_memory_budget = positive_size(parse_size(&budget)?, "--trace-mem-budget")?;
    }
    if let Some(size) = option_value(args, "--trace-segment-size") {
        config.trace_spill_segment_size =
            positive_size(parse_size(&size)?, "--trace-segment-size")?;
    }
    if let Some(budget) = option_value(args, "--trace-disk-budget") {
        config.trace_disk_budget = parse_size(&budget)?;
    }
    if let Some(compression) = option_value(args, "--trace-spill-compression") {
        config.trace_spill_compression = parse_trace_spill_compression(&compression)?;
    }
    if has_flag(args, "--no-mitm") {
        config.no_mitm = true;
    }
    if has_flag(args, "--strict-mitm") {
        config.strict_mitm = true;
    }
    if let Some(capacity) = option_value(args, "--mitm-cert-cache-capacity") {
        config.mitm_cert_cache_capacity = capacity
            .parse::<usize>()
            .map_err(|_| "--mitm-cert-cache-capacity must be numeric".to_string())?;
    }
    if let Some(capacity) = option_value(args, "--mitm-failure-cache-capacity") {
        config.mitm_failure_cache_capacity = capacity
            .parse::<usize>()
            .map_err(|_| "--mitm-failure-cache-capacity must be numeric".to_string())?;
    }
    if let Some(ttl) = option_value(args, "--mitm-failure-ttl-seconds") {
        let ttl = ttl
            .parse::<u64>()
            .map_err(|_| "--mitm-failure-ttl-seconds must be numeric".to_string())?;
        config.mitm_failure_ttl = positive_seconds(ttl, "--mitm-failure-ttl-seconds")?;
    }
    if let Some(timeout) = option_value(args, "--connect-probe-timeout-ms") {
        config.connect_probe_timeout = parse_cli_millis(&timeout, "--connect-probe-timeout-ms")?;
    }
    if let Some(limit) = option_value(args, "--h1-pool-max-active-per-key") {
        config.h1_pool_max_active_per_key =
            parse_positive_usize(&limit, "--h1-pool-max-active-per-key")?;
    }
    if let Some(timeout) = option_value(args, "--h1-pool-wait-timeout-ms") {
        config.h1_pool_wait_timeout = parse_cli_millis(&timeout, "--h1-pool-wait-timeout-ms")?;
    }
    if let Some(limit) = option_value(args, "--h2-pool-max-active-streams-per-key") {
        config.h2_pool_max_active_streams_per_key =
            parse_positive_usize(&limit, "--h2-pool-max-active-streams-per-key")?;
    }
    if let Some(timeout) = option_value(args, "--h2-pool-wait-timeout-ms") {
        config.h2_pool_wait_timeout = parse_cli_millis(&timeout, "--h2-pool-wait-timeout-ms")?;
    }
    if let Some(timeout) = option_value(args, "--tcp-connect-timeout-ms") {
        config.tcp_connect_timeout = parse_cli_millis(&timeout, "--tcp-connect-timeout-ms")?;
    }
    if let Some(timeout) = option_value(args, "--dns-timeout-ms") {
        config.dns_timeout = parse_cli_millis(&timeout, "--dns-timeout-ms")?;
    }
    if let Some(ttl) = option_value(args, "--dns-cache") {
        let ttl = ttl
            .parse::<u64>()
            .map_err(|_| "--dns-cache must be a number of seconds".to_string())?;
        config.dns_cache_ttl = Duration::from_secs(ttl);
    }
    let dns_servers = option_values(args, &["--dns-server"]);
    if !dns_servers.is_empty() {
        config.dns_servers = dns::parse_dns_servers(&dns_servers)?;
    }
    if let Some(timeout) = option_value(args, "--client-tls-handshake-timeout-ms") {
        config.client_tls_handshake_timeout =
            parse_cli_millis(&timeout, "--client-tls-handshake-timeout-ms")?;
    }
    if let Some(timeout) = option_value(args, "--upstream-tls-handshake-timeout-ms") {
        config.upstream_tls_handshake_timeout =
            parse_cli_millis(&timeout, "--upstream-tls-handshake-timeout-ms")?;
    }
    if let Some(timeout) = option_value(args, "--upstream-ttfb-timeout-ms") {
        config.upstream_ttfb_timeout = parse_cli_millis(&timeout, "--upstream-ttfb-timeout-ms")?;
    }
    if let Some(timeout) = option_value(args, "--request-timeout-ms") {
        config.request_total_timeout = parse_cli_millis(&timeout, "--request-timeout-ms")?;
    }
    if has_flag(args, "--no-trace-body") {
        config.trace_body_limit = 0;
    }
    Ok(())
}

fn parse_cli_millis(input: &str, option: &str) -> Result<Duration, String> {
    let value = input
        .parse::<u64>()
        .map_err(|_| format!("{option} must be numeric"))?;
    positive_millis(value, option)
}

fn positive_millis(value: u64, field: &str) -> Result<Duration, String> {
    if value == 0 {
        Err(format!("{field} must be greater than zero"))
    } else {
        Ok(Duration::from_millis(value))
    }
}

fn positive_seconds(value: u64, field: &str) -> Result<Duration, String> {
    if value == 0 {
        Err(format!("{field} must be greater than zero"))
    } else {
        Ok(Duration::from_secs(value))
    }
}

fn validate_mitm_mode(config: &AppConfig) -> Result<(), String> {
    if config.no_mitm && config.strict_mitm {
        Err("--no-mitm and --strict-mitm cannot be used together".to_string())
    } else {
        Ok(())
    }
}

fn positive_usize(value: usize, field: &str) -> Result<usize, String> {
    if value == 0 {
        Err(format!("{field} must be greater than zero"))
    } else {
        Ok(value)
    }
}

fn positive_size(value: usize, field: &str) -> Result<usize, String> {
    positive_usize(value, field)
}
