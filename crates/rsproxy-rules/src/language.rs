//! Stable, machine-readable contract for the rule language surface.
//!
//! Named parser dispatch and CLI reference generation consume the same action,
//! condition, and property spellings. Matcher entries describe their canonical
//! symbolic forms. Closure tests prevent either surface from drifting.

/// Current rule-language compatibility version.
///
/// Additive syntax may keep this version. Removing a spelling or changing the
/// meaning of accepted source requires a version bump and migration notes.
pub const RULE_LANGUAGE_VERSION: u16 = 3;

/// Required directive for standalone and persisted v3 rule sources.
pub const RULE_LANGUAGE_HEADER: &str = "@language 3";

/// Maximum UTF-8 bytes accepted in one physical rule source line.
pub const MAX_RULE_SOURCE_LINE_BYTES: usize = 64 * 1024;

/// Maximum aggregate UTF-8 source bytes compiled into one snapshot.
pub const MAX_RULE_SNAPSHOT_SOURCE_BYTES: usize = 16 * 1024 * 1024;

/// Maximum bytes accepted in one caller-supplied rule-group identifier.
pub const MAX_RULE_GROUP_NAME_BYTES: usize = 128;

/// Maximum source groups compiled into one immutable snapshot.
pub const MAX_RULE_GROUPS_PER_SNAPSHOT: usize = 1024;

/// Maximum non-empty, non-comment source rule lines accepted in one snapshot.
pub const MAX_RULES_PER_SNAPSHOT: usize = 10_000;

/// Maximum source diagnostics returned from one parse attempt.
pub const MAX_RULE_DIAGNOSTICS: usize = 256;

/// Maximum actions accepted on one source rule.
pub const MAX_RULE_ACTIONS_PER_RULE: usize = 256;

/// Maximum total parsed actions retained by one immutable snapshot.
pub const MAX_RULE_ACTIONS_PER_SNAPSHOT: usize = 100_000;

/// Maximum total condition AST nodes accepted on one source rule.
pub const MAX_RULE_CONDITION_NODES_PER_RULE: usize = 256;

/// Maximum total condition AST nodes retained by one immutable snapshot.
pub const MAX_RULE_CONDITION_NODES_PER_SNAPSHOT: usize = 100_000;

/// Maximum body substring/regex condition leaves retained by one snapshot.
pub const MAX_RULE_BODY_CONDITIONS_PER_SNAPSHOT: usize = 256;

/// Maximum property tokens accepted on one source rule.
pub const MAX_RULE_PROPERTIES_PER_RULE: usize = 64;

/// Maximum top-level comma-separated arguments accepted by one DSL call.
pub const MAX_RULE_CALL_ARGUMENTS: usize = 256;

/// Maximum bytes loaded from one external `@key`, `<path>`, or mock file.
pub const MAX_RULE_EXTERNAL_VALUE_BYTES: usize = 8 * 1024 * 1024;

/// Maximum bytes produced by rendering one action value or replacement.
pub const MAX_RULE_RENDERED_VALUE_BYTES: usize = 8 * 1024 * 1024;

/// Maximum UTF-8 bytes retained for one rendered trace tag.
pub const MAX_RULE_RENDERED_TAG_BYTES: usize = 4096;

/// Maximum distinct rule-produced trace tags retained for one request.
pub const MAX_RULE_TAGS_PER_REQUEST: usize = 256;

/// Maximum bytes rendered for one value embedded in human explanation output.
pub const MAX_RULE_EXPLAIN_VALUE_BYTES: usize = 4096;

/// Maximum bytes returned from one human explanation.
pub const MAX_RULE_EXPLAIN_BYTES: usize = 8 * 1024 * 1024;

/// Maximum proxy hops parsed from one rendered `upstream(...)` value.
pub const MAX_RULE_UPSTREAM_HOPS: usize = 32;

/// Maximum `|`-separated filesystem candidates tried by one mock action.
pub const MAX_RULE_MOCK_FILE_CANDIDATES: usize = 32;

/// Maximum pairwise comparisons performed by one conservative shadow lint.
pub const MAX_RULE_LINT_COMPARISONS: usize = 1_000_000;

/// Maximum matcher-source bytes charged across one shadow-lint comparison run.
pub const MAX_RULE_LINT_COMPARISON_BYTES: usize = 256 * 1024 * 1024;

/// Maximum findings retained by one shadow or semantic lint report.
pub const MAX_RULE_LINT_FINDINGS: usize = 10_000;

/// Approximate source/message bytes retained by one lint report.
pub const MAX_RULE_LINT_REPORT_BYTES: usize = 4 * 1024 * 1024;

/// Maximum bytes produced when rendering one filesystem path from a rule.
pub const MAX_RULE_EXTERNAL_PATH_BYTES: usize = 4096;

/// Maximum bytes loaded from one PEM certificate-chain or private-key file.
pub const MAX_RULE_TLS_PEM_BYTES: usize = 1024 * 1024;

/// Maximum recursive matcher, condition, and delimiter nesting.
pub const MAX_RULE_PARSE_NESTING: usize = 32;

/// Maximum numbered wildcard captures retained from one matched rule.
pub const MAX_RULE_GLOB_CAPTURES: usize = 9;

/// True for bytes allowed in an RFC 9110 HTTP token (header names, methods).
pub fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Inclusive minimum HTTP status accepted by status predicates.
pub const MIN_HTTP_STATUS: u16 = 100;

/// Inclusive maximum HTTP status accepted by predicates and final-response actions.
pub const MAX_HTTP_STATUS: u16 = 599;

/// Inclusive minimum status accepted by final-response actions.
pub const MIN_FINAL_HTTP_STATUS: u16 = 200;

/// Complete set of redirect statuses accepted by `redirect`.
pub const REDIRECT_STATUSES: &[u16] = &[301, 302, 303, 307, 308];

/// Reports whether an HTTP status forbids generated response content.
pub(crate) fn status_forbids_body(status: u16) -> bool {
    !rsproxy_http::status_can_send_content(status)
}

/// One canonical DSL spelling/form and all compatibility aliases for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleSyntaxSpelling {
    /// Help topics that describe this spelling.
    pub topics: &'static [&'static str],
    /// Canonical spelling used by named dispatch or symbolic matcher form.
    pub canonical: &'static str,
    /// Additional spellings accepted by the parser.
    pub aliases: &'static [&'static str],
    /// Whether accepted source begins with the spelling and carries a suffix.
    pub prefix: bool,
}

impl RuleSyntaxSpelling {
    /// Reports whether `input` is the canonical spelling or an accepted alias.
    pub fn accepts(&self, input: &str) -> bool {
        if self.prefix {
            input
                .strip_prefix(self.canonical)
                .is_some_and(|suffix| !suffix.is_empty())
                || self.aliases.iter().any(|alias| {
                    input
                        .strip_prefix(alias)
                        .is_some_and(|suffix| !suffix.is_empty())
                })
        } else {
            self.canonical == input || self.aliases.contains(&input)
        }
    }
}

macro_rules! spelling {
    ($topics:expr, $canonical:literal $(, $alias:literal)* $(,)?) => {
        RuleSyntaxSpelling {
            topics: $topics,
            canonical: $canonical,
            aliases: &[$($alias),*],
            prefix: false,
        }
    };
}

/// Complete action token/call surface accepted by the parser.
pub const ACTION_SYNTAX: &[RuleSyntaxSpelling] = &[
    spelling!(&["action.host"], "host"),
    spelling!(&["action.upstream"], "upstream"),
    spelling!(&["action.direct"], "direct"),
    spelling!(&["action.mock"], "mock"),
    spelling!(
        &["action.mock"],
        "mock.raw",
        "mockRaw",
        "mock_raw",
        "mock-raw"
    ),
    spelling!(
        &["action.map.remote"],
        "map.remote",
        "mapRemote",
        "map_remote",
        "map-remote"
    ),
    spelling!(&["action.status"], "status"),
    spelling!(&["action.redirect"], "redirect"),
    spelling!(&["action.req.header"], "req.header"),
    spelling!(&["action.res.header"], "res.header"),
    spelling!(&["action.res.status"], "res.status"),
    spelling!(&["action.req.method"], "req.method"),
    spelling!(&["action.req.cookie"], "req.cookie"),
    spelling!(&["action.res.cookie"], "res.cookie"),
    spelling!(&["action.req.ua"], "req.ua"),
    spelling!(&["action.req.referer"], "req.referer"),
    spelling!(&["action.req.auth"], "req.auth"),
    spelling!(&["action.req.forwarded"], "req.forwarded"),
    spelling!(&["action.req.type"], "req.type"),
    spelling!(&["action.req.charset"], "req.charset"),
    spelling!(&["action.res.cors"], "res.cors"),
    spelling!(&["action.res.type"], "res.type"),
    spelling!(&["action.res.charset"], "res.charset"),
    spelling!(&["action.res.merge"], "res.merge"),
    spelling!(&["action.res.trailer"], "res.trailer"),
    spelling!(&["action.attachment"], "attachment"),
    spelling!(&["action.cache"], "cache"),
    spelling!(&["action.tls"], "tls"),
    spelling!(&["action.url.rewrite"], "url.rewrite"),
    spelling!(&["action.url.query"], "url.query"),
    spelling!(&["action.delete"], "delete"),
    spelling!(&["action.req.body.set"], "req.body.set"),
    spelling!(&["action.req.body.prepend"], "req.body.prepend"),
    spelling!(&["action.req.body.append"], "req.body.append"),
    spelling!(&["action.req.body.replace"], "req.body.replace"),
    spelling!(&["action.res.body.set"], "res.body.set"),
    spelling!(&["action.res.body.prepend"], "res.body.prepend"),
    spelling!(&["action.res.body.append"], "res.body.append"),
    spelling!(&["action.res.body.replace"], "res.body.replace"),
    spelling!(&["action.inject"], "inject"),
    spelling!(&["action.delay.req", "action.delay.res"], "delay"),
    spelling!(&["action.throttle.req", "action.throttle.res"], "throttle"),
    spelling!(&["action.bypass"], "bypass"),
    spelling!(&["action.hide"], "hide"),
    spelling!(&["action.tag"], "tag"),
    spelling!(&["action.skip"], "skip"),
];

/// Complete named condition surface accepted after `when`.
pub const CONDITION_SYNTAX: &[RuleSyntaxSpelling] = &[
    spelling!(&["condition.method"], "method"),
    spelling!(&["condition.host"], "host"),
    spelling!(&["condition.url"], "url"),
    spelling!(
        &["condition.client-ip"],
        "client.ip",
        "clientIp",
        "ip",
        "client_ip",
        "client-ip"
    ),
    spelling!(
        &["condition.server-ip"],
        "server.ip",
        "serverIp",
        "server_ip",
        "server-ip"
    ),
    spelling!(&["condition.header"], "header"),
    spelling!(
        &["condition.res.header"],
        "res.header",
        "resHeader",
        "res_header",
        "res-header"
    ),
    spelling!(&["condition.body"], "body"),
    spelling!(&["condition.status"], "status"),
    spelling!(&["condition.chance"], "chance"),
    spelling!(&["condition.env"], "env"),
    spelling!(&["condition.any"], "any"),
    spelling!(&["condition.all"], "all"),
    spelling!(&["condition.not"], "not"),
];

/// Complete matcher form surface represented in the language reference.
pub const MATCHER_SYNTAX: &[RuleSyntaxSpelling] = &[
    spelling!(&["matcher.glob"], "HOST|URL-GLOB"),
    spelling!(&["matcher.exact"], "=ABSOLUTE-URL"),
    spelling!(&["matcher.regex"], "/REGEX/"),
    spelling!(&["matcher.port"], ":PORT"),
    spelling!(&["matcher.not"], "!MATCHER"),
];

/// Complete rule-property surface accepted after actions and conditions.
pub const PROPERTY_SYNTAX: &[RuleSyntaxSpelling] = &[
    spelling!(&["property.important"], "@important"),
    spelling!(&["property.disabled"], "@disabled"),
    RuleSyntaxSpelling {
        topics: &["property.tag"],
        canonical: "@tag:",
        aliases: &[],
        prefix: true,
    },
];

/// Returns the canonical action parser spelling for an accepted name.
pub fn canonical_action_name(input: &str) -> Option<&'static str> {
    canonical_name(ACTION_SYNTAX, input)
}

pub(crate) fn canonical_v3_action_name(input: &str) -> Option<&'static str> {
    canonical_v3_name(ACTION_SYNTAX, input)
}

/// Returns the canonical condition parser spelling for an accepted name.
pub fn canonical_condition_name(input: &str) -> Option<&'static str> {
    canonical_name(CONDITION_SYNTAX, input)
}

pub(crate) fn canonical_v3_condition_name(input: &str) -> Option<&'static str> {
    canonical_v3_name(CONDITION_SYNTAX, input)
}

/// Returns the canonical property parser spelling for an accepted name.
pub fn canonical_property_name(input: &str) -> Option<&'static str> {
    canonical_name(PROPERTY_SYNTAX, input)
}

/// Returns the canonical property spelling and, for prefix spellings, the
/// remainder after the matched prefix (possibly empty; named spellings yield "").
pub(crate) fn canonical_property(input: &str) -> Option<(&'static str, &str)> {
    PROPERTY_SYNTAX.iter().find_map(|spelling| {
        if spelling.prefix {
            std::iter::once(&spelling.canonical)
                .chain(spelling.aliases)
                .find_map(|prefix| input.strip_prefix(prefix))
                .map(|suffix| (spelling.canonical, suffix))
        } else {
            (spelling.canonical == input || spelling.aliases.contains(&input))
                .then_some((spelling.canonical, ""))
        }
    })
}

fn canonical_name(spellings: &'static [RuleSyntaxSpelling], input: &str) -> Option<&'static str> {
    spellings
        .iter()
        .find(|spelling| spelling.accepts(input))
        .map(|spelling| spelling.canonical)
}

fn canonical_v3_name(
    spellings: &'static [RuleSyntaxSpelling],
    input: &str,
) -> Option<&'static str> {
    spellings.iter().find_map(|spelling| {
        if spelling.prefix {
            input
                .strip_prefix(spelling.canonical)
                .is_some_and(|suffix| !suffix.is_empty())
                .then_some(spelling.canonical)
        } else {
            (spelling.canonical == input).then_some(spelling.canonical)
        }
    })
}
