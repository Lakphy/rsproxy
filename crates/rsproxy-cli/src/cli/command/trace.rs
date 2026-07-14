use clap::{Args, Subcommand};
use std::path::PathBuf;

use super::ClientArgs;

#[derive(Args)]
pub(crate) struct ValuesArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    /// Values subcommand. Defaults to `ls` when omitted.
    #[command(subcommand)]
    pub(crate) command: Option<ValuesCommand>,
}

#[derive(Subcommand)]
pub(crate) enum ValuesCommand {
    /// List stored value names.
    #[command(
        name = "ls",
        visible_alias = "list",
        long_about = "List value-file names in deterministic order. The daemon is queried when available; otherwise <storage>/values is read directly.",
        after_help = "EXAMPLES:\n  rsproxy values ls\n  rsproxy values ls --json | jq -r '.[]'"
    )]
    List(ValuesListArgs),
    /// Print one stored value.
    #[command(
        long_about = "Print one named value. Plain output contains only the stored text, without an added newline, so it can be redirected or used in command substitution.",
        after_help = "EXAMPLES:\n  rsproxy values cat api-token\n  rsproxy values cat response-template > template.html\n  rsproxy values cat api-token --json | jq -r .value"
    )]
    Cat(ValueKeyArgs),
    /// Create or replace a value from a file or stdin.
    #[command(
        long_about = "Create or replace one named text value. Read from --file when supplied; otherwise read all text from stdin. The value is persisted below <storage>/values.",
        after_help = "EXAMPLES:\n  printf '%s' 'staging-token' | rsproxy values set api-token\n  rsproxy values set response-template --file ./template.html\n\nUse `values cat KEY` to verify the stored text."
    )]
    Set(ValueSetArgs),
    /// Remove one stored value.
    #[command(
        name = "rm",
        visible_aliases = ["remove", "delete"],
        long_about = "Remove one named value from storage and notify the running daemon when reachable. This command does not prompt for confirmation.",
        after_help = "EXAMPLES:\n  rsproxy values cat api-token > api-token.bak\n  rsproxy values rm api-token"
    )]
    Remove(ValueKeyArgs),
}

#[derive(Args)]
pub(crate) struct ValuesListArgs {}

#[derive(Args)]
pub(crate) struct ValueKeyArgs {
    /// Value-file name.
    #[arg(value_name = "KEY")]
    pub(crate) key: String,
}

#[derive(Args)]
pub(crate) struct ValueSetArgs {
    /// Value-file name to create or replace.
    #[arg(value_name = "KEY")]
    pub(crate) key: String,
    /// Read value text from FILE instead of stdin.
    #[arg(long, value_name = "FILE")]
    pub(crate) file: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct TraceArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    /// Trace subcommand. Defaults to `ls` when omitted.
    #[command(subcommand)]
    pub(crate) command: Option<TraceCommand>,
}

#[derive(Subcommand)]
pub(crate) enum TraceCommand {
    /// List the newest captured sessions.
    #[command(
        name = "ls",
        visible_alias = "list",
        long_about = "List recent captured sessions from the running daemon, newest first. Increase --limit to inspect a larger window, or use `trace follow` for a live stream.",
        after_help = "EXAMPLES:\n  rsproxy trace ls\n  rsproxy trace ls --limit 100\n  rsproxy trace ls --json | jq '.[] | {id, method, url, status}'"
    )]
    List(TraceListArgs),
    /// Fetch one captured session by ID.
    #[command(
        long_about = "Fetch full stored details for one captured session, including headers, bounded body previews, rule matches, timings, and errors when available.",
        after_help = "EXAMPLES:\n  rsproxy trace ls\n  rsproxy trace get 42\n  rsproxy trace get 42 --json | jq '._rsproxy // .'"
    )]
    Get(TraceGetArgs),
    /// Stream newly captured sessions as NDJSON.
    #[command(
        long_about = "Stream up to 100 recent backlog sessions, then newly completed sessions. Each session is printed as one JSON line. The stream continues until --count is reached or it is interrupted.",
        after_help = "EXAMPLES:\n  rsproxy trace follow\n  rsproxy trace follow --count 10\n  rsproxy trace follow --poll-ms 1000 | jq --unbuffered '.url'\n\nPress Ctrl-C to stop an unbounded follow stream."
    )]
    Follow(TraceFollowArgs),
    /// Show trace memory, queue, and disk-spill statistics.
    #[command(
        long_about = "Show collector capacity, memory use, event/drop counts, disk-spill segments, eviction, compression, and corruption counters for the running daemon.",
        after_help = "EXAMPLES:\n  rsproxy trace stats\n  rsproxy trace stats --json | jq"
    )]
    Stats(TraceStatsArgs),
    /// Delete captured sessions from memory and disk spill.
    #[command(
        long_about = "Clear the daemon's in-memory trace store and delete its on-disk trace segments. Active proxying continues. This operation cannot be undone and does not prompt for confirmation.",
        after_help = "EXAMPLES:\n  rsproxy trace export --output sessions.json\n  rsproxy trace clear\n  rsproxy trace stats"
    )]
    Clear(TraceClearArgs),
    /// Export captured sessions as JSON or HAR.
    #[command(
        long_about = "Export available captured sessions from the running daemon. The default format is rsproxy JSON; --har selects HAR 1.2. Output is printed to stdout unless --output is supplied.",
        after_help = "EXAMPLES:\n  rsproxy trace export --output sessions.json\n  rsproxy trace export --har --output sessions.har\n  rsproxy trace export --har | jq '.log.entries | length'"
    )]
    Export(TraceExportArgs),
    /// Replay a captured session by ID.
    #[command(
        long_about = "Ask the running daemon to replay one captured session by ID and print the replay result. This is the trace-scoped alias of the top-level `rsproxy replay`. Find session IDs with `rsproxy trace ls` or the TUI.",
        after_help = "EXAMPLES:\n  rsproxy trace ls\n  rsproxy trace replay 42\n\nReplay sends a new outbound request and therefore can repeat side effects of the original request."
    )]
    Replay(TraceReplayArgs),
}

#[derive(Args)]
pub(crate) struct TraceStatsArgs {}

#[derive(Args)]
pub(crate) struct TraceClearArgs {}

#[derive(Args)]
pub(crate) struct TraceListArgs {
    /// Maximum number of recent sessions to return.
    #[arg(short = 'n', long, default_value_t = 20, value_name = "COUNT")]
    pub(crate) limit: usize,
}

#[derive(Args)]
pub(crate) struct TraceGetArgs {
    /// Session ID shown by `trace ls` or the TUI.
    #[arg(value_name = "ID")]
    pub(crate) id: String,
}

#[derive(Args)]
pub(crate) struct TraceReplayArgs {
    /// Captured session ID shown by `trace ls` or the TUI.
    #[arg(value_name = "ID")]
    pub(crate) id: String,
}

#[derive(Args)]
pub(crate) struct TraceFollowArgs {
    /// Stop after printing COUNT sessions. Omit it to follow until interrupted; zero exits
    /// immediately.
    #[arg(long, value_name = "COUNT")]
    pub(crate) count: Option<usize>,
    /// Requested heartbeat/poll interval in milliseconds, clamped to 100..30000 [default: 500].
    #[arg(long, value_name = "MILLISECONDS")]
    pub(crate) poll_ms: Option<u64>,
}

#[derive(Args)]
pub(crate) struct TraceExportArgs {
    /// Export HAR 1.2 instead of rsproxy's native JSON format.
    #[arg(long)]
    pub(crate) har: bool,
    /// Write the export to FILE instead of stdout.
    #[arg(short = 'o', long, value_name = "FILE")]
    pub(crate) output: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct TuiArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    /// Maximum recent sessions displayed [default: 20].
    #[arg(short = 'n', long, value_name = "COUNT")]
    pub(crate) limit: Option<usize>,
    /// Case-insensitive text filter applied to session summary fields such as method and URL.
    #[arg(long, value_name = "TEXT")]
    pub(crate) filter: Option<String>,
    /// Initial detail tab.
    #[arg(long, value_parser = ["overview", "headers", "body", "rules"], value_name = "TAB")]
    pub(crate) tab: Option<String>,
    /// Automatic refresh interval in milliseconds; values below 100 are raised to 100 [default:
    /// 1000].
    #[arg(long, value_name = "MILLISECONDS")]
    pub(crate) interval_ms: Option<u64>,
    /// Print one snapshot and exit without entering raw/alternate-screen terminal mode.
    #[arg(long)]
    pub(crate) once: bool,
}

#[derive(Args)]
pub(crate) struct ReplayArgs {
    /// Captured session ID shown by `trace ls` or the TUI.
    #[arg(value_name = "ID")]
    pub(crate) id: String,
    #[command(flatten)]
    pub(crate) client: ClientArgs,
}
