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
    about = "Local intercepting proxy and rule engine"
)]
/// Parsed top-level CLI state passed across the executable/library boundary.
///
/// Subcommand details remain owned by this crate; external callers inspect the
/// global JSON flag and pass the value to [`crate::run_parsed`].
pub struct Cli {
    /// Emit machine-readable JSON where the selected command supports it.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub(crate) command: Option<TopLevelCommand>,
}

#[derive(Subcommand)]
pub(crate) enum TopLevelCommand {
    /// Run the proxy in the foreground.
    Run(RuntimeArgs),
    /// Start the proxy as a daemon.
    Start(RuntimeArgs),
    /// Stop a running daemon.
    Stop(RuntimeArgs),
    /// Restart the daemon.
    Restart(RuntimeArgs),
    /// Query daemon status.
    Status(RuntimeArgs),
    /// Validate, manage, inspect, and benchmark rules.
    Rules(RulesArgs),
    /// Manage value files.
    Values(ValuesArgs),
    /// Inspect and export captured sessions.
    Trace(TraceArgs),
    /// Open the terminal user interface.
    Tui(TuiArgs),
    /// Replay a captured session.
    Replay(ReplayArgs),
    /// Manage the local certificate authority.
    Ca(CaArgs),
    /// Inspect or change the operating-system proxy.
    Proxy(ProxyArgs),
    /// Generate a shell completion script.
    Completions(CompletionsArgs),
}

#[derive(Clone, Default, Args)]
pub(crate) struct ClientArgs {
    /// Control endpoint (HOST:PORT, unix:/path.sock, or pipe:NAME).
    #[arg(long, global = true)]
    pub(crate) api: Option<String>,
    /// Control API bearer token.
    #[arg(long, global = true)]
    pub(crate) api_token: Option<String>,
    /// Data and runtime storage directory.
    #[arg(long, global = true)]
    pub(crate) storage: Option<PathBuf>,
    /// TOML configuration file.
    #[arg(long, global = true)]
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

    #[arg(short = 'p', long)]
    pub(crate) port: Option<u16>,
    #[arg(long)]
    pub(crate) host: Option<String>,
    #[arg(long)]
    pub(crate) watch: bool,
    #[arg(long)]
    pub(crate) watch_debounce_ms: Option<u64>,
    #[arg(long)]
    pub(crate) proxy_auth: Option<String>,
    #[arg(long)]
    pub(crate) max_header_size: Option<String>,
    #[arg(long)]
    pub(crate) max_header_count: Option<usize>,
    #[arg(long)]
    pub(crate) body_buffer_limit: Option<String>,
    #[arg(long)]
    pub(crate) trace_body_limit: Option<String>,
    #[arg(long)]
    pub(crate) trace_filter: Option<String>,
    #[arg(long)]
    pub(crate) trace_queue_capacity: Option<usize>,
    #[arg(long)]
    pub(crate) trace_mem_budget: Option<String>,
    #[arg(long)]
    pub(crate) trace_segment_size: Option<String>,
    #[arg(long)]
    pub(crate) trace_disk_budget: Option<String>,
    #[arg(long)]
    pub(crate) trace_spill_compression: Option<String>,
    #[arg(long)]
    pub(crate) no_mitm: bool,
    #[arg(long)]
    pub(crate) strict_mitm: bool,
    #[arg(long)]
    pub(crate) mitm_cert_cache_capacity: Option<usize>,
    #[arg(long)]
    pub(crate) mitm_failure_cache_capacity: Option<usize>,
    #[arg(long)]
    pub(crate) mitm_failure_ttl_seconds: Option<u64>,
    #[arg(long)]
    pub(crate) connect_probe_timeout_ms: Option<u64>,
    #[arg(long)]
    pub(crate) h1_pool_max_active_per_key: Option<usize>,
    #[arg(long)]
    pub(crate) h1_pool_wait_timeout_ms: Option<u64>,
    #[arg(long)]
    pub(crate) h2_pool_max_active_streams_per_key: Option<usize>,
    #[arg(long)]
    pub(crate) h2_pool_wait_timeout_ms: Option<u64>,
    #[arg(long)]
    pub(crate) tcp_connect_timeout_ms: Option<u64>,
    #[arg(long)]
    pub(crate) dns_timeout_ms: Option<u64>,
    #[arg(long)]
    pub(crate) dns_cache: Option<u64>,
    #[arg(long, action = clap::ArgAction::Append)]
    pub(crate) dns_server: Vec<String>,
    #[arg(long)]
    pub(crate) client_tls_handshake_timeout_ms: Option<u64>,
    #[arg(long)]
    pub(crate) upstream_tls_handshake_timeout_ms: Option<u64>,
    #[arg(long)]
    pub(crate) upstream_ttfb_timeout_ms: Option<u64>,
    #[arg(long)]
    pub(crate) request_timeout_ms: Option<u64>,
    #[arg(long)]
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
    #[arg(value_enum)]
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
