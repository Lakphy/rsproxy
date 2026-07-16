use clap::{ArgAction, Args, Subcommand};
use std::path::PathBuf;

use super::ClientArgs;

#[derive(Args)]
pub(crate) struct RulesArgs {
    #[command(flatten)]
    pub(crate) client: ClientArgs,
    /// Rules subcommand. Defaults to `ls` when omitted.
    #[command(subcommand)]
    pub(crate) command: Option<RulesCommand>,
}

#[derive(Subcommand)]
pub(crate) enum RulesCommand {
    /// Validate rules read from FILE or stdin.
    #[command(
        long_about = "Parse and validate a standalone rule file without changing active rules. If FILE is omitted, all input is read from stdin. Diagnostics include the source line and parser guidance.",
        after_help = "EXAMPLES:\n  rsproxy rules check ./debug.rules\n  printf '%s\n' 'example.test status(503)' | rsproxy rules check\n\nA successful check prints the number of parsed rules."
    )]
    Check(RulesCheckArgs),
    /// List rule groups.
    #[command(
        name = "ls",
        visible_alias = "list",
        long_about = "List rule groups in evaluation order, including enabled state and rule count. The running daemon is queried when available; otherwise groups are loaded from storage.",
        after_help = "EXAMPLES:\n  rsproxy rules ls\n  rsproxy rules ls --json | jq\n\nUse `rules cat GROUP` to inspect a group's source."
    )]
    List(RulesListArgs),
    /// Print a rule group (defaults to `default`).
    #[command(
        long_about = "Print the exact source text of one rule group. The group name defaults to `default`. Plain output contains only rule text so it can be redirected to a file.",
        after_help = "EXAMPLES:\n  rsproxy rules cat\n  rsproxy rules cat mobile > mobile.rules\n  rsproxy rules cat mobile --json | jq -r .text"
    )]
    Cat(OptionalGroupArgs),
    /// Edit a rule group (defaults to `default`).
    #[command(
        long_about = "Open one rule group in $VISUAL, then $EDITOR, or vi. The edited text is validated before it replaces the group; invalid content is rejected and the active group is preserved.",
        after_help = "EXAMPLES:\n  rsproxy rules edit\n  EDITOR='code --wait' rsproxy rules edit mobile\n\nThe group defaults to `default` and is created when it does not yet exist."
    )]
    Edit(OptionalGroupArgs),
    /// Replace a rule group from FILE or stdin (defaults to `default`).
    #[command(
        long_about = "Validate and atomically replace one rule group. Read from --file when supplied; otherwise read the complete rule text from stdin. The group name defaults to `default`.",
        after_help = "EXAMPLES:\n  rsproxy rules set default --file ./debug.rules\n  rsproxy rules set mobile --file ./mobile.rules\n  printf '%s\n' 'example.test status(503)' | rsproxy rules set\n\nUse `rules check FILE` first when preparing a larger ruleset."
    )]
    Set(RulesSetArgs),
    /// Remove a rule group.
    #[command(
        name = "rm",
        visible_aliases = ["remove", "delete"],
        long_about = "Permanently remove one named rule group from the daemon/storage. This command does not prompt for confirmation.",
        after_help = "EXAMPLES:\n  rsproxy rules cat mobile > mobile.rules.bak\n  rsproxy rules rm mobile\n\nUse `rules disable GROUP` instead when you may need the group again."
    )]
    Remove(RequiredGroupArgs),
    /// Enable a rule group.
    #[command(
        visible_alias = "on",
        long_about = "Enable an existing rule group so it participates in rule resolution. Group ordering is unchanged.",
        after_help = "EXAMPLES:\n  rsproxy rules enable mobile\n  rsproxy rules ls"
    )]
    Enable(RequiredGroupArgs),
    /// Disable a rule group.
    #[command(
        visible_alias = "off",
        long_about = "Disable an existing rule group without deleting its source. Disabled groups remain visible in `rules ls` and can be enabled later.",
        after_help = "EXAMPLES:\n  rsproxy rules disable mobile\n  rsproxy rules ls"
    )]
    Disable(RequiredGroupArgs),
    /// Detect rules shadowed by earlier, broader rules.
    #[command(
        long_about = "Compile rules and report later rules that can never take effect because an earlier, condition-free rule with a broader matcher always wins their single-action family first. Rules resolve in group order then line order (first match wins per family), so a leading wildcard rule silently swallows more specific rules below it. With --file, lint that standalone file; otherwise lint enabled groups from the daemon or selected storage.",
        after_help = "EXAMPLES:\n  rsproxy rules lint\n  rsproxy rules lint --file ./candidate.rules\n\nEXIT STATUS:\n  0 when no shadowed rules are found; 1 when findings exist.\n\nThe check is conservative: it only reports provable shadowing, so a clean run does not guarantee the ordering is correct. Put specific rules above broader wildcard rules within a group."
    )]
    Lint(RulesSourceArgs),
    /// Print rule index statistics.
    #[command(
        long_about = "Compile rules and report the shape of the matching indexes. With --file, inspect that standalone file; otherwise inspect enabled groups from the daemon or selected storage.",
        after_help = "EXAMPLES:\n  rsproxy rules stats\n  rsproxy rules stats --file ./large.rules\n  rsproxy rules stats --json | jq '{rules, indexed_rules, global_rules}'"
    )]
    Stats(RulesSourceArgs),
    /// Benchmark rule resolution.
    #[command(
        long_about = "Benchmark local rule matching for one simulated request and report p50, p99, and maximum resolver time in nanoseconds. This measures only rule resolution, not network or proxy latency.",
        after_help = "EXAMPLES:\n  rsproxy rules bench https://api.example.com/users\n  rsproxy rules bench --url https://api.example.com/users --iterations 50000 --warmup 1000\n  rsproxy rules bench https://api.example.com --file ./candidate.rules --json\n\nRequest metadata options match those accepted by `rules test`."
    )]
    Bench(RulesBenchArgs),
    /// Explain the rules applied to a request.
    #[command(
        long_about = "Simulate request metadata and explain every rule match, source line, and resulting action without sending network traffic. Add response metadata to exercise response-phase conditions and actions.",
        after_help = "EXAMPLES:\n  rsproxy rules test https://api.example.com/users\n  rsproxy rules test https://api.test -X POST -H 'Content-Type: application/json' -d '{}'\n  rsproxy rules test https://api.test --response-status 404\n\nThe active daemon is queried when available; otherwise the selected storage is evaluated locally."
    )]
    Test(RulesTestArgs),
}

#[derive(Args)]
pub(crate) struct RulesListArgs {}

#[derive(Args)]
pub(crate) struct RulesCheckArgs {
    /// Rule file to validate. Omit FILE to read rule text from stdin.
    #[arg(value_name = "FILE")]
    pub(crate) file: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct OptionalGroupArgs {
    /// Rule group name.
    #[arg(default_value = "default", value_name = "GROUP")]
    pub(crate) group: String,
}

#[derive(Args)]
pub(crate) struct RequiredGroupArgs {
    /// Rule group name.
    #[arg(value_name = "GROUP")]
    pub(crate) group: String,
}

#[derive(Args)]
pub(crate) struct RulesSetArgs {
    /// Rule group to replace.
    #[arg(default_value = "default", value_name = "GROUP")]
    pub(crate) group: String,
    /// Read rule text from FILE instead of stdin.
    #[arg(long, value_name = "FILE")]
    pub(crate) file: Option<PathBuf>,
}

#[derive(Args)]
pub(crate) struct RulesSourceArgs {
    /// Compile this standalone rule file instead of active rule groups.
    #[arg(long, value_name = "FILE")]
    pub(crate) file: Option<PathBuf>,
}

#[derive(Clone, Default, Args)]
pub(crate) struct RequestArgs {
    /// Simulated HTTP method.
    #[arg(
        short = 'X',
        long,
        default_value = "GET",
        value_name = "METHOD",
        help_heading = "Request metadata"
    )]
    pub(crate) method: String,
    /// Simulated request header in `Name: value` form. Repeat for multiple headers.
    #[arg(short = 'H', long, action = ArgAction::Append, value_name = "NAME:VALUE", help_heading = "Request metadata")]
    pub(crate) header: Vec<String>,
    /// Simulated request body as a command-line string.
    #[arg(
        short = 'd',
        long,
        value_name = "TEXT",
        help_heading = "Request metadata"
    )]
    pub(crate) body: Option<String>,
    /// Simulated downstream client IP used by clientIp conditions and templates.
    #[arg(long, value_name = "IP", help_heading = "Request metadata")]
    pub(crate) client_ip: Option<String>,
    /// Simulated resolved server IP. A literal IP in the URL is inferred automatically.
    #[arg(long, value_name = "IP", help_heading = "Request metadata")]
    pub(crate) server_ip: Option<String>,
}

#[derive(Args)]
pub(crate) struct RulesTestArgs {
    /// Absolute request URL to evaluate, including scheme and host.
    #[arg(value_name = "URL")]
    pub(crate) url: String,
    #[command(flatten)]
    pub(crate) request: RequestArgs,
    /// Simulated response status (100..599). Supplying any response option switches the explain to
    /// response phase; status defaults to 200 when only response headers are supplied.
    #[arg(long, value_name = "CODE", help_heading = "Response metadata")]
    pub(crate) response_status: Option<String>,
    /// Simulated response header in `Name: value` form. Repeat for multiple headers.
    #[arg(long, action = ArgAction::Append, value_name = "NAME:VALUE", help_heading = "Response metadata")]
    pub(crate) response_header: Vec<String>,
}

#[derive(Args)]
pub(crate) struct RulesBenchArgs {
    /// URL to benchmark (may also be supplied with --url).
    #[arg(value_name = "URL")]
    pub(crate) positional_url: Option<String>,
    /// URL to benchmark when not supplied positionally.
    #[arg(long, value_name = "URL")]
    pub(crate) url: Option<String>,
    #[command(flatten)]
    pub(crate) request: RequestArgs,
    #[command(flatten)]
    pub(crate) source: RulesSourceArgs,
    /// Number of measured rule resolutions [default: 10000].
    #[arg(short = 'n', long, value_name = "COUNT")]
    pub(crate) iterations: Option<usize>,
    /// Number of unmeasured warmup resolutions [default: 100].
    #[arg(long, value_name = "COUNT")]
    pub(crate) warmup: Option<usize>,
}
