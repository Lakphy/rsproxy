use clap::{Args, Parser, Subcommand, ValueEnum};
use std::fmt;
use std::path::PathBuf;

mod ca;
mod proxy;
mod rules;
mod trace;

pub(crate) use ca::{
    CaArgs, CaCommand, CaExportArgs, CaInitArgs, CaIssueArgs, CaStatusArgs, CaTrustArgs,
};
pub(crate) use proxy::{ProxyArgs, ProxyCommand, ProxyMutationArgs, ProxyPlatformArg};
pub(crate) use rules::{RequestArgs, RulesArgs, RulesBenchArgs, RulesCommand, RulesTestArgs};
pub(crate) use trace::{ReplayArgs, TraceArgs, TraceCommand, TuiArgs, ValuesArgs, ValuesCommand};

#[derive(Parser)]
#[command(
    name = "rsproxy",
    version,
    about = "Intercept, inspect, rewrite, and replay HTTP/HTTPS traffic",
    long_about = "A local debugging proxy with a programmable rule engine.\n\n\
rsproxy can run in the foreground or as a background daemon, capture sessions, explain and edit \
rules, replay requests, and configure the operating-system proxy. HTTP works immediately. To \
inspect HTTPS traffic, initialize and trust the local CA first.",
    after_help = "QUICK START:\n  \
1. Create the local CA:       rsproxy ca init\n  \
2. Trust it for HTTPS:        rsproxy ca install --dry-run\n                              rsproxy ca install\n  \
3. Start the daemon:          rsproxy start\n  \
4. Route this machine:        rsproxy proxy on --all --dry-run\n                              rsproxy proxy on --all\n  \
5. Inspect captured traffic:  rsproxy tui\n\n\
When finished, restore the system proxy with `rsproxy proxy off --all`, then stop the daemon with \
`rsproxy stop`. Run `rsproxy help <COMMAND>` for command-specific examples.\n\n\
CONFIGURATION:\n  CLI options override TOML settings, which override built-in defaults. The default config is \
  $RSPROXY_HOME/config.toml or ~/.rsproxy/config.toml. Process logs are controlled by RSPROXY_LOG \
  and RSPROXY_LOG_FORMAT."
)]
/// Parsed top-level CLI state passed across the executable/library boundary.
///
/// Subcommand details remain owned by this crate; external callers inspect the
/// global JSON flag and pass the value to [`crate::run_parsed`].
pub struct Cli {
    /// Emit machine-readable JSON where the selected command supports it. Errors are written as
    /// one JSON document to stderr.
    #[arg(long, global = true, help_heading = "Output")]
    pub json: bool,

    #[command(subcommand)]
    pub(crate) command: Option<TopLevelCommand>,
}

#[derive(Subcommand)]
pub(crate) enum TopLevelCommand {
    /// Run the proxy in the foreground.
    #[command(
        long_about = "Run the proxy and control API in the foreground. Logs stay attached to the current terminal, making this the best mode for development, containers, and troubleshooting.",
        after_help = "EXAMPLES:\n  Start with built-in defaults:\n    rsproxy run\n\n  Use a custom port and reload rules after file changes:\n    rsproxy run --port 8899 --watch\n\n  Test HTTP without changing the system proxy:\n    curl -x http://127.0.0.1:8899 http://example.com\n\nFor HTTPS inspection, run `rsproxy ca init` and `rsproxy ca install` first."
    )]
    Run(RuntimeArgs),
    /// Start the proxy as a daemon.
    #[command(
        long_about = "Start rsproxy as a background daemon and wait until both the proxy and control endpoint are ready. Runtime files and logs are stored below the selected storage directory.",
        after_help = "EXAMPLES:\n  Start with built-in defaults:\n    rsproxy start\n\n  Start an isolated instance:\n    rsproxy start --storage ./tmp/rsproxy --port 18899\n\n  Verify it and view the log location:\n    rsproxy status\n\nNext steps: use `rsproxy proxy on --all --dry-run` to preview system proxy changes, or configure an application manually to use http://127.0.0.1:8899."
    )]
    Start(RuntimeArgs),
    /// Stop a running daemon.
    #[command(
        long_about = "Stop the daemon identified by the selected storage/configuration. Identity checks prevent an unrelated process from being terminated through a stale pidfile.",
        after_help = "EXAMPLES:\n  Restore the system proxy, then stop rsproxy:\n    rsproxy proxy off --all\n    rsproxy stop\n\n  Stop an isolated instance:\n    rsproxy stop --storage ./tmp/rsproxy\n\nOn macOS, replace --all with the same `--service NAME` used to enable routing. Use the same `--storage` or `--config` values that were passed to `rsproxy start`."
    )]
    Stop(RuntimeArgs),
    /// Restart the daemon.
    #[command(
        long_about = "Stop the selected daemon if it is running, then start it with the resolved configuration. Rules and values stored on disk are preserved.",
        after_help = "EXAMPLES:\n  Restart after changing config.toml:\n    rsproxy restart\n\n  Restart while overriding one setting:\n    rsproxy restart --port 18899\n\nCheck the new process with `rsproxy status`."
    )]
    Restart(RuntimeArgs),
    /// Query daemon status.
    #[command(
        long_about = "Query the control endpoint for daemon health, active configuration, rule/trace statistics, and runtime counters. The daemon must be running and the selected API/storage must match it.",
        after_help = "EXAMPLES:\n  Show status:\n    rsproxy status\n\n  Pretty-print selected fields with jq:\n    rsproxy status --json | jq '{version, proxy, rules, trace}'\n\n  Query a TCP control endpoint:\n    rsproxy status --api 127.0.0.1:8900 --api-token \"$RSPROXY_API_TOKEN\""
    )]
    Status(RuntimeArgs),
    /// Validate, manage, inspect, and benchmark rules.
    #[command(
        long_about = "Validate rule syntax, manage ordered rule groups, explain a simulated request, or benchmark matching. Management commands use the running daemon when available and otherwise fall back to the selected storage directory.",
        after_help = "COMMON WORKFLOW:\n  rsproxy rules check ./debug.rules\n  rsproxy rules set default --file ./debug.rules\n  rsproxy rules ls\n  rsproxy rules test https://api.example.com/users -H 'Accept: application/json'\n\nRun `rsproxy help rules <COMMAND>` for the input format and examples of each operation."
    )]
    Rules(RulesArgs),
    /// Manage value files.
    #[command(
        long_about = "Manage named text values referenced by rules and templates. Reads use the daemon when available and fall back to <storage>/values; writes are persisted to storage and notify the daemon when reachable.",
        after_help = "EXAMPLES:\n  printf '%s' 'staging-token' | rsproxy values set api-token\n  rsproxy values ls\n  rsproxy values cat api-token\n  rsproxy values rm api-token\n\nUse `--file FILE` with `values set` to avoid reading from stdin."
    )]
    Values(ValuesArgs),
    /// Inspect and export captured sessions.
    #[command(
        long_about = "Inspect, follow, clear, and export sessions captured by a running rsproxy daemon. Session bodies are previews bounded by the configured trace limits.",
        after_help = "EXAMPLES:\n  rsproxy trace ls --limit 20\n  rsproxy trace get 42 | jq\n  rsproxy trace follow\n  rsproxy trace export --har --output sessions.har\n\nUse `rsproxy trace stats` to inspect memory and disk-spill usage."
    )]
    Trace(TraceArgs),
    /// Open the terminal user interface.
    #[command(
        long_about = "Open an interactive terminal view of daemon status and recent sessions. Use --once for a non-interactive snapshot suitable for terminals without alternate-screen support.",
        after_help = "EXAMPLES:\n  Open the interactive TUI:\n    rsproxy tui\n\n  Start on the headers tab and filter recent sessions:\n    rsproxy tui --tab headers --filter example.com\n\n  Print one snapshot instead of entering interactive mode:\n    rsproxy tui --once --json\n\nKEYS: q quit, R refresh, r replay, / edit filter, Tab change detail tab, Up/Down select."
    )]
    Tui(TuiArgs),
    /// Replay a captured session.
    #[command(
        long_about = "Ask the running daemon to replay one captured session by ID and print the replay result. Find session IDs with `rsproxy trace ls` or the TUI.",
        after_help = "EXAMPLES:\n  rsproxy trace ls\n  rsproxy replay 42\n\nReplay sends a new outbound request and therefore can repeat side effects of the original request."
    )]
    Replay(ReplayArgs),
    /// Manage the local certificate authority.
    #[command(
        long_about = "Create and inspect rsproxy's local root CA, install or remove operating-system trust, export the public certificate, and issue diagnostic leaf certificates. CA material lives under <storage>/ca.",
        after_help = "HTTPS SETUP:\n  rsproxy ca init\n  rsproxy ca install --dry-run\n  rsproxy ca install\n  rsproxy ca status\n\nOnly trust a CA stored in a directory you control. Remove trust later with `rsproxy ca uninstall`."
    )]
    Ca(CaArgs),
    /// Inspect or change the operating-system proxy.
    #[command(
        long_about = "Inspect or update the host operating system's HTTP and HTTPS proxy settings. The target defaults to the proxy host/port resolved from config (127.0.0.1:8899 with built-in defaults).",
        after_help = "SAFE WORKFLOW:\n  rsproxy proxy status\n  rsproxy proxy on --all --dry-run\n  rsproxy proxy on --all\n  rsproxy proxy status\n\nAlways restore routing with `rsproxy proxy off --all` before deleting rsproxy state. On macOS, replace --all with `--service NAME` to change one service. Platform support uses networksetup on macOS, WinINet registry settings on Windows, and gsettings on Linux."
    )]
    Proxy(ProxyArgs),
    /// Generate a shell completion script.
    #[command(
        long_about = "Generate a completion script for the selected shell from the live command tree. The script is written to stdout and can be evaluated for one session or installed in the shell's completion directory.",
        after_help = "EXAMPLES:\n  Bash (current session):\n    source <(rsproxy completions bash)\n\n  Zsh (current session):\n    source <(rsproxy completions zsh)\n\n  Fish (install for the user):\n    rsproxy completions fish > ~/.config/fish/completions/rsproxy.fish\n\n  PowerShell (current session):\n    rsproxy completions powershell | Out-String | Invoke-Expression"
    )]
    Completions(CompletionsArgs),
}

#[derive(Clone, Default, Args)]
pub(crate) struct ClientArgs {
    /// Control endpoint used by client commands. Accepts HOST:PORT, unix:/path.sock, or pipe:NAME.
    /// Defaults to a storage-scoped Unix socket or the local Windows named pipe.
    #[arg(
        long,
        global = true,
        value_name = "ENDPOINT",
        help_heading = "Control and storage"
    )]
    pub(crate) api: Option<String>,
    /// Bearer token for TCP and Windows named-pipe control endpoints. Resolution order is this
    /// option, RSPROXY_API_TOKEN, config, then <storage>/run/api-token. Ignored for Unix sockets.
    #[arg(
        long,
        global = true,
        value_name = "TOKEN",
        help_heading = "Control and storage"
    )]
    pub(crate) api_token: Option<String>,
    /// State directory containing rules, values, CA material, traces, and runtime files. Defaults
    /// to $RSPROXY_HOME, $HOME/.rsproxy, or ./.rsproxy (in that order).
    #[arg(
        long,
        global = true,
        value_name = "DIR",
        help_heading = "Control and storage"
    )]
    pub(crate) storage: Option<PathBuf>,
    /// Read runtime settings from this TOML file. If omitted, the default storage config.toml is
    /// loaded when it exists. CLI options take precedence over file settings.
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help_heading = "Control and storage"
    )]
    pub(crate) config: Option<PathBuf>,
}

impl fmt::Debug for ClientArgs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClientArgs")
            .field("api", &self.api)
            .field("api_token", &self.api_token.as_ref().map(|_| "[REDACTED]"))
            .field("storage", &self.storage)
            .field("config", &self.config)
            .finish()
    }
}

#[derive(Clone, Default, Args)]
pub(crate) struct RuntimeArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,

    /// Proxy listener port. Use 0 only with `run` to request an ephemeral port.
    #[arg(
        short = 'p',
        long,
        value_name = "PORT",
        help_heading = "Proxy listener"
    )]
    pub(crate) port: Option<u16>,
    /// Proxy listener address. The built-in default is 127.0.0.1; use 0.0.0.0 only with suitable
    /// network controls and preferably --proxy-auth.
    #[arg(long, value_name = "HOST", help_heading = "Proxy listener")]
    pub(crate) host: Option<String>,
    /// Watch <storage>/rules and atomically reload valid external rule changes.
    #[arg(long, help_heading = "Rules")]
    pub(crate) watch: bool,
    /// Trailing-edge debounce for --watch in milliseconds. Must be greater than zero [built-in
    /// default: 200].
    #[arg(long, value_name = "MILLISECONDS", help_heading = "Rules")]
    pub(crate) watch_debounce_ms: Option<u64>,
    /// Require HTTP Basic proxy authentication using a non-empty user:password pair. Quote the
    /// value if it contains shell metacharacters.
    #[arg(long, value_name = "USER:PASSWORD", help_heading = "Proxy listener")]
    pub(crate) proxy_auth: Option<String>,
    /// Maximum accepted HTTP header bytes. Supports bytes, kb, mb, or gb [built-in default:
    /// 256kb].
    #[arg(long, value_name = "SIZE", help_heading = "Request limits")]
    pub(crate) max_header_size: Option<String>,
    /// Maximum number of HTTP headers. Must be greater than zero [built-in default: 256].
    #[arg(long, value_name = "COUNT", help_heading = "Request limits")]
    pub(crate) max_header_count: Option<usize>,
    /// Maximum body aggregated for body-dependent rules. Larger bodies stream unchanged. Supports
    /// bytes, kb, mb, or gb [built-in default: 8mb].
    #[arg(long, value_name = "SIZE", help_heading = "Request limits")]
    pub(crate) body_buffer_limit: Option<String>,
    /// Maximum request/response body preview retained per trace. Zero disables body previews.
    /// Supports bytes, kb, mb, or gb [built-in default: 64kb].
    #[arg(long, value_name = "SIZE", help_heading = "Trace capture")]
    pub(crate) trace_body_limit: Option<String>,
    /// Body capture policy: headers-only, media (skip media bodies), or full [built-in default:
    /// media]. Comma-separated policies are accepted.
    #[arg(long, value_name = "POLICY", help_heading = "Trace capture")]
    pub(crate) trace_filter: Option<String>,
    /// Maximum queued trace events before backpressure/drops. Must be greater than zero [built-in
    /// default: 8192].
    #[arg(long, value_name = "COUNT", help_heading = "Trace storage")]
    pub(crate) trace_queue_capacity: Option<usize>,
    /// In-memory trace budget. Supports bytes, kb, mb, or gb [built-in default: 256mb].
    #[arg(long, value_name = "SIZE", help_heading = "Trace storage")]
    pub(crate) trace_mem_budget: Option<String>,
    /// Target size of each on-disk trace segment. Supports bytes, kb, mb, or gb [built-in default:
    /// 64mb].
    #[arg(long, value_name = "SIZE", help_heading = "Trace storage")]
    pub(crate) trace_segment_size: Option<String>,
    /// Total on-disk trace budget; oldest segments are evicted first. Zero disables disk spill
    /// [built-in default: 2gb].
    #[arg(long, value_name = "SIZE", help_heading = "Trace storage")]
    pub(crate) trace_disk_budget: Option<String>,
    /// Trace segment compression: none or zstd[:LEVEL], where LEVEL is 1..22 [built-in default:
    /// none].
    #[arg(long, value_name = "MODE", help_heading = "Trace storage")]
    pub(crate) trace_spill_compression: Option<String>,
    /// Disable HTTPS interception globally and pass CONNECT tunnels through unchanged.
    #[arg(long, help_heading = "HTTPS interception")]
    pub(crate) no_mitm: bool,
    /// Fail visibly when HTTPS interception fails instead of remembering the host and falling back
    /// to passthrough. Cannot be combined with --no-mitm.
    #[arg(long, help_heading = "HTTPS interception")]
    pub(crate) strict_mitm: bool,
    /// Maximum generated leaf certificates kept in memory. Zero disables the memory cache
    /// [built-in default: 1024].
    #[arg(long, value_name = "COUNT", help_heading = "HTTPS interception")]
    pub(crate) mitm_cert_cache_capacity: Option<usize>,
    /// Maximum hosts remembered after a client TLS failure. Zero disables failure memory
    /// [built-in default: 1024].
    #[arg(long, value_name = "COUNT", help_heading = "HTTPS interception")]
    pub(crate) mitm_failure_cache_capacity: Option<usize>,
    /// Seconds to remember a client TLS failure before trying interception again. Must be greater
    /// than zero [built-in default: 300].
    #[arg(long, value_name = "SECONDS", help_heading = "HTTPS interception")]
    pub(crate) mitm_failure_ttl_seconds: Option<u64>,
    /// Time to classify the first bytes of a CONNECT tunnel before passthrough. Must be greater
    /// than zero [built-in default: 250ms].
    #[arg(long, value_name = "MILLISECONDS", help_heading = "HTTPS interception")]
    pub(crate) connect_probe_timeout_ms: Option<u64>,
    /// Maximum active HTTP/1 upstream connections per destination key. Must be greater than zero
    /// [built-in default: 256].
    #[arg(long, value_name = "COUNT", help_heading = "Connection pools")]
    pub(crate) h1_pool_max_active_per_key: Option<usize>,
    /// Maximum wait for an HTTP/1 pool lease. Must be greater than zero [built-in default:
    /// 15000ms].
    #[arg(long, value_name = "MILLISECONDS", help_heading = "Connection pools")]
    pub(crate) h1_pool_wait_timeout_ms: Option<u64>,
    /// Maximum active HTTP/2 streams per destination key. Must be greater than zero [built-in
    /// default: 256].
    #[arg(long, value_name = "COUNT", help_heading = "Connection pools")]
    pub(crate) h2_pool_max_active_streams_per_key: Option<usize>,
    /// Maximum wait for an HTTP/2 stream lease. Must be greater than zero [built-in default:
    /// 15000ms].
    #[arg(long, value_name = "MILLISECONDS", help_heading = "Connection pools")]
    pub(crate) h2_pool_wait_timeout_ms: Option<u64>,
    /// TCP connection timeout. Must be greater than zero [built-in default: 10000ms].
    #[arg(long, value_name = "MILLISECONDS", help_heading = "Network timeouts")]
    pub(crate) tcp_connect_timeout_ms: Option<u64>,
    /// DNS lookup timeout. Must be greater than zero [built-in default: 5000ms].
    #[arg(long, value_name = "MILLISECONDS", help_heading = "DNS")]
    pub(crate) dns_timeout_ms: Option<u64>,
    /// Maximum positive/negative DNS cache TTL in seconds. Zero disables DNS caching [built-in
    /// default: 60].
    #[arg(long, value_name = "SECONDS", help_heading = "DNS")]
    pub(crate) dns_cache: Option<u64>,
    /// Custom DNS server as IP or IP:PORT. Repeat the option or use comma-separated servers. If
    /// omitted, the system resolver is used.
    #[arg(long, action = clap::ArgAction::Append, value_name = "IP[:PORT]", help_heading = "DNS")]
    pub(crate) dns_server: Vec<String>,
    /// Timeout for the client-to-rsproxy TLS handshake. Must be greater than zero [built-in
    /// default: 10000ms].
    #[arg(long, value_name = "MILLISECONDS", help_heading = "Network timeouts")]
    pub(crate) client_tls_handshake_timeout_ms: Option<u64>,
    /// Timeout for the rsproxy-to-origin TLS handshake. Must be greater than zero [built-in
    /// default: 10000ms].
    #[arg(long, value_name = "MILLISECONDS", help_heading = "Network timeouts")]
    pub(crate) upstream_tls_handshake_timeout_ms: Option<u64>,
    /// Maximum wait for upstream response headers. Must be greater than zero [built-in default:
    /// 60000ms].
    #[arg(long, value_name = "MILLISECONDS", help_heading = "Network timeouts")]
    pub(crate) upstream_ttfb_timeout_ms: Option<u64>,
    /// End-to-end request deadline. Must be greater than zero [built-in default: 360000ms].
    #[arg(long, value_name = "MILLISECONDS", help_heading = "Network timeouts")]
    pub(crate) request_timeout_ms: Option<u64>,
    /// Disable request and response body previews while keeping trace metadata and headers. This is
    /// equivalent to a zero trace body limit.
    #[arg(long, help_heading = "Trace capture")]
    pub(crate) no_trace_body: bool,
}

impl RuntimeArgs {
    pub(crate) fn from_client(client: ClientArgs) -> Self {
        Self {
            client,
            ..Self::default()
        }
    }
}

#[derive(Args)]
pub(crate) struct CompletionsArgs {
    /// Shell whose completion script should be written to stdout.
    #[arg(value_enum, value_name = "SHELL")]
    pub(crate) shell: CompletionShell,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    #[value(alias = "pwsh")]
    Powershell,
}
