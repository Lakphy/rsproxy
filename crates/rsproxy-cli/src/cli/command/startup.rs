use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args)]
pub(crate) struct StartupArgs {
    #[command(subcommand)]
    pub(crate) command: StartupCommand,
}

#[derive(Subcommand)]
pub(crate) enum StartupCommand {
    /// Register rsproxy to start when the current user logs in.
    #[command(
        long_about = "Write the native per-user login startup entry and a small rsproxy launcher manifest. At login, the launcher starts the daemon, waits until it is ready, and enables system proxy routing unless --no-system-proxy is selected.",
        after_help = "EXAMPLES:\n  Preview registration:\n    rsproxy startup install --dry-run\n\n  Register and activate it now:\n    rsproxy startup install --start-now\n\n  Use an explicit runtime config:\n    rsproxy startup install --config ~/.rsproxy/config.toml --start-now\n\n  Start the daemon at login without changing system proxy settings:\n    rsproxy startup install --no-system-proxy"
    )]
    Install(StartupInstallArgs),
    /// Inspect the login startup entry and launcher manifest.
    #[command(
        long_about = "Report whether the native login startup entry and rsproxy launcher manifest are installed. This command does not start the daemon or mutate system proxy settings.",
        after_help = "EXAMPLES:\n  rsproxy startup status\n  rsproxy startup status --json"
    )]
    Status,
    /// Remove login startup and safely stop its active proxy by default.
    #[command(
        long_about = "Disable system proxy routing configured by startup, stop the selected daemon, and remove the login entry and launcher manifest. Use --keep-running to remove only future login startup while leaving current runtime and routing state unchanged.",
        after_help = "EXAMPLES:\n  Preview cleanup:\n    rsproxy startup uninstall --dry-run\n\n  Restore proxy settings, stop rsproxy, and unregister it:\n    rsproxy startup uninstall\n\n  Unregister future startup but leave the current daemon and proxy unchanged:\n    rsproxy startup uninstall --keep-running"
    )]
    Uninstall(StartupUninstallArgs),
    /// Internal login launcher entry point.
    #[command(hide = true)]
    Launch,
}

#[derive(Args)]
pub(crate) struct StartupInstallArgs {
    /// Runtime state directory to select at login. The resolved absolute path is stored in the
    /// launcher manifest.
    #[arg(long, value_name = "DIR", help_heading = "Runtime selection")]
    pub(crate) storage: Option<PathBuf>,
    /// Runtime TOML file to load at login. Relative paths are resolved before registration.
    #[arg(long, value_name = "FILE", help_heading = "Runtime selection")]
    pub(crate) config: Option<PathBuf>,
    /// Register only daemon startup without changing the operating-system proxy at login.
    #[arg(long, help_heading = "System proxy")]
    pub(crate) no_system_proxy: bool,
    /// macOS network service to configure automatically, for example Wi-Fi. When omitted, all
    /// enabled services are selected.
    #[arg(long, value_name = "NAME", help_heading = "System proxy")]
    pub(crate) service: Option<String>,
    /// Comma-separated hosts or domains that should bypass the automatic system proxy.
    #[arg(long, value_name = "HOSTS", help_heading = "System proxy")]
    pub(crate) bypass: Option<String>,
    /// Start the daemon and configure system proxy immediately after registration.
    #[arg(long, help_heading = "Activation")]
    pub(crate) start_now: bool,
    /// Print the target backend, files, and behavior without changing host state.
    #[arg(long, help_heading = "Safety")]
    pub(crate) dry_run: bool,
}

#[derive(Args)]
pub(crate) struct StartupUninstallArgs {
    /// Remove future login startup without stopping the current daemon or restoring proxy state.
    #[arg(long, help_heading = "Runtime cleanup")]
    pub(crate) keep_running: bool,
    /// Print cleanup behavior without changing host state.
    #[arg(long, help_heading = "Safety")]
    pub(crate) dry_run: bool,
}
