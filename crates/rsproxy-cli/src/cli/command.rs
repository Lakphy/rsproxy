use clap::{Args, Parser, Subcommand, ValueEnum};
use std::fmt;
use std::path::PathBuf;

mod ca;
mod proxy;
mod rules;
mod trace;

pub use ca::{CaArgs, CaCommand, CaExportArgs, CaInitArgs, CaIssueArgs, CaStatusArgs, CaTrustArgs};
pub use proxy::{ProxyArgs, ProxyCommand, ProxyMutationArgs, ProxyPlatformArg};
pub use rules::{RequestArgs, RulesArgs, RulesBenchArgs, RulesCommand, RulesTestArgs};
pub use trace::{ReplayArgs, TraceArgs, TraceCommand, TuiArgs, ValuesArgs, ValuesCommand};

#[derive(Parser)]
#[command(
    name = "rsproxy",
    version,
    about = "Local intercepting proxy and rule engine"
)]
pub struct Cli {
    /// Emit machine-readable JSON where the selected command supports it.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<TopLevelCommand>,
}

#[derive(Subcommand)]
pub enum TopLevelCommand {
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
pub struct ClientArgs {
    /// Control endpoint (HOST:PORT, unix:/path.sock, or pipe:NAME).
    #[arg(long, global = true)]
    pub api: Option<String>,
    /// Control API bearer token.
    #[arg(long, global = true)]
    pub api_token: Option<String>,
    /// Data and runtime storage directory.
    #[arg(long, global = true)]
    pub storage: Option<PathBuf>,
    /// TOML configuration file.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
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
pub struct RuntimeArgs {
    #[command(flatten)]
    pub client: ClientArgs,

    #[arg(short = 'p', long)]
    pub port: Option<u16>,
    #[arg(long)]
    pub host: Option<String>,
    #[arg(long)]
    pub watch: bool,
    #[arg(long)]
    pub watch_debounce_ms: Option<u64>,
    #[arg(long)]
    pub proxy_auth: Option<String>,
    #[arg(long)]
    pub max_header_size: Option<String>,
    #[arg(long)]
    pub max_header_count: Option<usize>,
    #[arg(long)]
    pub body_buffer_limit: Option<String>,
    #[arg(long)]
    pub trace_body_limit: Option<String>,
    #[arg(long)]
    pub trace_filter: Option<String>,
    #[arg(long)]
    pub trace_queue_capacity: Option<usize>,
    #[arg(long)]
    pub trace_mem_budget: Option<String>,
    #[arg(long)]
    pub trace_segment_size: Option<String>,
    #[arg(long)]
    pub trace_disk_budget: Option<String>,
    #[arg(long)]
    pub trace_spill_compression: Option<String>,
    #[arg(long)]
    pub no_mitm: bool,
    #[arg(long)]
    pub strict_mitm: bool,
    #[arg(long)]
    pub mitm_cert_cache_capacity: Option<usize>,
    #[arg(long)]
    pub mitm_failure_cache_capacity: Option<usize>,
    #[arg(long)]
    pub mitm_failure_ttl_seconds: Option<u64>,
    #[arg(long)]
    pub connect_probe_timeout_ms: Option<u64>,
    #[arg(long)]
    pub h1_pool_max_active_per_key: Option<usize>,
    #[arg(long)]
    pub h1_pool_wait_timeout_ms: Option<u64>,
    #[arg(long)]
    pub h2_pool_max_active_streams_per_key: Option<usize>,
    #[arg(long)]
    pub h2_pool_wait_timeout_ms: Option<u64>,
    #[arg(long)]
    pub tcp_connect_timeout_ms: Option<u64>,
    #[arg(long)]
    pub dns_timeout_ms: Option<u64>,
    #[arg(long)]
    pub dns_cache: Option<u64>,
    #[arg(long, action = clap::ArgAction::Append)]
    pub dns_server: Vec<String>,
    #[arg(long)]
    pub client_tls_handshake_timeout_ms: Option<u64>,
    #[arg(long)]
    pub upstream_tls_handshake_timeout_ms: Option<u64>,
    #[arg(long)]
    pub upstream_ttfb_timeout_ms: Option<u64>,
    #[arg(long)]
    pub request_timeout_ms: Option<u64>,
    #[arg(long)]
    pub no_trace_body: bool,
}

impl RuntimeArgs {
    pub fn from_client(client: ClientArgs) -> Self {
        Self {
            client,
            ..Self::default()
        }
    }
}

#[derive(Args)]
pub struct CompletionsArgs {
    #[arg(value_enum)]
    pub shell: CompletionShell,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    #[value(alias = "pwsh")]
    Powershell,
}
