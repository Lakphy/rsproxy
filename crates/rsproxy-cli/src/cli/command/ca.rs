use clap::{Args, Subcommand};
use std::path::PathBuf;

use super::ClientArgs;

#[derive(Args)]
pub(crate) struct CaArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    #[command(subcommand)]
    pub(crate) command: Option<CaCommand>,
}

#[derive(Subcommand)]
pub(crate) enum CaCommand {
    /// Create the local root certificate and private key.
    #[command(
        long_about = "Create a self-signed root CA below <storage>/ca. Existing complete CA state is reused unless --force is supplied; partial state is rejected to avoid silently replacing key material.",
        after_help = "EXAMPLES:\n  rsproxy ca init\n  rsproxy ca init --name 'My development proxy CA'\n\nRun `rsproxy ca install --dry-run` to preview the separate operating-system trust step."
    )]
    Init(CaInitArgs),
    /// Show CA files, initialization state, and certificate fingerprint.
    #[command(
        long_about = "Inspect the local CA without changing it. With --keychain on macOS, also report whether the CA fingerprint is present in that keychain. Running `rsproxy ca` without a subcommand is equivalent to this status command.",
        after_help = "EXAMPLES:\n  rsproxy ca status\n  rsproxy ca status --json\n  rsproxy ca status --keychain ~/Library/Keychains/login.keychain-db"
    )]
    Status(CaStatusArgs),
    /// Export the public root certificate as PEM.
    #[command(
        long_about = "Export only the public root certificate. The private key is never included. Output is written as PEM to stdout unless --out is provided.",
        after_help = "EXAMPLES:\n  rsproxy ca export --out rsproxy-root-ca.pem\n  rsproxy ca export | openssl x509 -noout -subject -fingerprint -sha256"
    )]
    Export(CaExportArgs),
    /// Issue or reuse a cached diagnostic leaf certificate.
    #[command(
        long_about = "Issue a leaf certificate signed by the initialized rsproxy root CA and cache its certificate, key, and chain below <storage>/ca/leaf. HOST may be a DNS name or IP address, without a URL scheme or path.",
        after_help = "EXAMPLES:\n  rsproxy ca issue api.example.test\n  rsproxy ca issue 127.0.0.1\n  rsproxy ca issue api.example.test --force\n\nThis command is primarily for diagnostics and local test origins; normal HTTPS interception issues certificates automatically."
    )]
    Issue(CaIssueArgs),
    /// Add the local root CA to the operating-system trust store.
    #[command(
        long_about = "Trust the initialized root CA through the native operating-system backend. This changes system or user trust settings and may require elevated privileges.",
        after_help = "SAFE WORKFLOW:\n  rsproxy ca install --dry-run\n  rsproxy ca install\n  rsproxy ca status\n\nOnly install a CA whose storage directory and fingerprint you recognize. Remove it later with `rsproxy ca uninstall`."
    )]
    Install(CaTrustArgs),
    /// Remove the local root CA from the operating-system trust store.
    #[command(
        long_about = "Remove trust for this rsproxy root CA through the native operating-system backend. Local CA files remain in <storage>/ca and may be deleted separately after rsproxy is stopped.",
        after_help = "EXAMPLES:\n  rsproxy ca uninstall --dry-run\n  rsproxy ca uninstall\n\nOn macOS, pass the same --keychain used during installation when it was not the default keychain."
    )]
    Uninstall(CaTrustArgs),
}

#[derive(Args)]
pub(crate) struct CaInitArgs {
    /// Replace an existing or partial CA, invalidating previously issued leaf certificates. Trust
    /// for the old certificate may need to be removed separately.
    #[arg(long)]
    pub(crate) force: bool,
    /// Common name embedded in the root certificate [default: rsproxy local root CA].
    #[arg(long, value_name = "COMMON_NAME")]
    pub(crate) name: Option<String>,
}

#[derive(Args)]
pub(crate) struct CaStatusArgs {
    /// macOS keychain file to inspect for this CA's fingerprint.
    #[arg(long, value_name = "FILE")]
    pub(crate) keychain: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct CaExportArgs {
    /// Write the PEM certificate to FILE instead of stdout.
    #[arg(short = 'o', long = "out", value_name = "FILE")]
    pub(crate) output: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct CaIssueArgs {
    /// DNS name or IP address to place in the leaf certificate SAN.
    #[arg(value_name = "HOST")]
    pub(crate) host: String,
    /// Regenerate and overwrite the cached leaf certificate for HOST.
    #[arg(long)]
    pub(crate) force: bool,
}

#[derive(Args)]
pub(crate) struct CaTrustArgs {
    /// macOS keychain file to modify. Omit it to use the platform default.
    #[arg(long, value_name = "FILE")]
    pub(crate) keychain: Option<PathBuf>,
    /// Print the native trust-store command without executing it.
    #[arg(long)]
    pub(crate) dry_run: bool,
}
