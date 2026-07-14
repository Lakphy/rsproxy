use clap::{Args, Subcommand, ValueEnum};

use super::ClientArgs;

#[derive(Args)]
pub(crate) struct ProxyArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    /// Backend to inspect or preview [default: current operating system]. Cross-platform values are
    /// most useful with --dry-run; execution requires the corresponding native tools.
    #[arg(
        long,
        global = true,
        value_enum,
        value_name = "PLATFORM",
        help_heading = "Platform"
    )]
    pub(crate) platform: Option<ProxyPlatformArg>,
    /// macOS network service name, for example Wi-Fi. Required for macOS on/off unless --all is
    /// used; status without a service lists all services.
    #[arg(long, global = true, value_name = "NAME", help_heading = "Platform")]
    pub(crate) service: Option<String>,
    /// Print the native commands and intended changes without modifying system settings.
    #[arg(long, global = true, help_heading = "Safety")]
    pub(crate) dry_run: bool,
    #[command(subcommand)]
    pub(crate) command: Option<ProxyCommand>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ProxyPlatformArg {
    #[value(alias = "darwin")]
    Macos,
    #[value(alias = "win")]
    Windows,
    Linux,
}

#[derive(Subcommand)]
pub(crate) enum ProxyCommand {
    /// Inspect current HTTP/HTTPS proxy settings.
    #[command(
        long_about = "Read the current platform proxy configuration without changing it. On macOS, omit --service to inspect every network service.",
        after_help = "EXAMPLES:\n  rsproxy proxy status\n  rsproxy proxy status --json\n  rsproxy proxy status --service Wi-Fi\n  rsproxy proxy status --platform linux --dry-run"
    )]
    Status(ProxyStatusArgs),
    /// Route operating-system HTTP/HTTPS traffic through rsproxy.
    #[command(
        visible_alias = "enable",
        long_about = "Configure and enable both HTTP and HTTPS proxy routing. The target defaults to the resolved rsproxy listener. On macOS, select one network service with --service or explicitly select every enabled service with --all.",
        after_help = "SAFE WORKFLOW:\n  rsproxy start\n  rsproxy proxy on --all --dry-run\n  rsproxy proxy on --all\n  rsproxy proxy status\n\nFor one macOS service, use `--service Wi-Fi` instead of `--all`. Restore routing with the matching `rsproxy proxy off` command."
    )]
    On(ProxyMutationArgs),
    /// Disable operating-system HTTP/HTTPS proxy routing.
    #[command(
        visible_alias = "disable",
        long_about = "Disable HTTP and HTTPS proxy routing through the native platform backend. On macOS, use the same --service selection used when enabling the proxy, or --all to cover every enabled service.",
        after_help = "EXAMPLES:\n  rsproxy proxy off --all --dry-run\n  rsproxy proxy off --all\n  rsproxy proxy status\n\nFor one macOS service:\n  rsproxy proxy off --service Wi-Fi"
    )]
    Off(ProxyMutationArgs),
}

#[derive(Args)]
pub(crate) struct ProxyStatusArgs {}

#[derive(Args)]
pub(crate) struct ProxyMutationArgs {
    /// Proxy hostname or IP written to system settings [default: resolved rsproxy listener host].
    #[arg(long, value_name = "HOST", help_heading = "Proxy target")]
    pub(crate) host: Option<String>,
    /// Proxy port written to system settings [default: resolved rsproxy listener port, built-in
    /// 8899].
    #[arg(long, value_name = "PORT", help_heading = "Proxy target")]
    pub(crate) port: Option<u16>,
    /// Comma-separated hosts or domains that should connect directly instead of using rsproxy.
    #[arg(long, value_name = "HOSTS", help_heading = "Proxy target")]
    pub(crate) bypass: Option<String>,
    /// Apply a macOS mutation to every enabled network service. Required there when --service is
    /// omitted; ignored by Windows and Linux backends.
    #[arg(long, help_heading = "Platform")]
    pub(crate) all: bool,
}
