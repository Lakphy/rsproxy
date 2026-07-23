use super::super::Topic;

pub(super) const TOPICS: &[Topic] = &[
    topic!(
        "matcher.glob",
        "matchers",
        "Structured scheme, host, effective-port, path, and query matching.",
        [
            "[SCHEME://]HOST[:PORT][/PATH][?QUERY]",
            "* matches within one component",
            "** crosses host labels or path segments"
        ],
        ["https://**.example.test/api/** status(204)"],
        [
            "A plain path is a segment-boundary prefix; a wildcard path is a whole-component glob.",
            "*.example.test matches exactly one subdomain; **.example.test also matches the root.",
            "A backslash escapes the next glob character and selects whole-component glob semantics; a dangling escape is rejected.",
            "Omitted components are unconstrained."
        ],
        ["glob", "domain", "host-matcher", "wildcard"],
        ["matcher.exact", "matcher.port", "concept.templates"]
    ),
    topic!(
        "matcher.exact",
        "matchers",
        "Strict absolute-URL matching with an optional unconstrained query.",
        ["=ABSOLUTE_URL"],
        ["=https://example.test/api status(204)"],
        [
            "Scheme, normalized host, effective port, and path must match.",
            "When the matcher omits a query, any request query is accepted; when present it must match exactly."
        ],
        ["exact", "exact-url"],
        ["matcher.glob"]
    ),
    topic!(
        "matcher.regex",
        "matchers",
        "Compiled full-URL regular expression with numbered and named captures.",
        ["/REGEX/", "/REGEX/i"],
        ["/users\\/(?P<uid>\\d+)/ req.header(x-user: ${uid})"],
        [
            "Rust regex is preferred for linear matching.",
            "Lookaround and backreferences fall back to fancy-regex with a hard backtrack limit.",
            "Only the i flag is supported; captures expose $0, $1..$9, and named values."
        ],
        ["regex", "regexp"],
        ["concept.templates", "concept.limits"]
    ),
    topic!(
        "matcher.port",
        "matchers",
        "Match only the explicit or scheme-default effective port.",
        [":PORT"],
        [":8443 status(204)"],
        [
            "PORT is 1..65535.",
            "http/ws default to 80 and https/wss default to 443."
        ],
        ["port", "port-only"],
        ["matcher.glob"]
    ),
    topic!(
        "matcher.not",
        "matchers",
        "Invert any matcher without retaining captures from the nested matcher.",
        ["!MATCHER"],
        ["!private.example.test status(204)"],
        [
            "Negation may be nested up to the global parser depth limit.",
            "A successful negated match has no inner matcher captures."
        ],
        ["negation", "not-matcher"],
        ["concept.limits"]
    ),
    topic!(
        "condition.method",
        "conditions",
        "Match any listed HTTP method case-insensitively.",
        ["when method(METHOD[, METHOD...])"],
        ["example.test status(204) when method(GET, HEAD)"],
        ["Values inside one call are ORed."],
        ["method"],
        ["concept.ordering"]
    ),
    topic!(
        "condition.host",
        "conditions",
        "Match the normalized request host with host-glob semantics.",
        ["when host(PATTERN)"],
        ["example.test status(204) when host(**.example.test)"],
        ["The pattern follows matcher host wildcard rules."],
        ["host-condition"],
        ["matcher.glob"]
    ),
    topic!(
        "condition.url",
        "conditions",
        "Match the complete raw URL using a glob or regex.",
        ["when url(GLOB)", "when url(/REGEX/[i])"],
        ["example.test status(204) when url(*mode=debug*)"],
        ["A non-regex value without * is an exact raw-URL comparison."],
        ["url-condition"],
        ["matcher.regex"]
    ),
    topic!(
        "condition.header",
        "conditions",
        "Require a request header or a case-insensitive value substring.",
        ["when header(NAME)", "when header(NAME ~ TEXT)"],
        ["example.test status(204) when header(accept ~ json)"],
        [
            "Header names are validated HTTP tokens and compared case-insensitively.",
            "The contains operand must be non-empty."
        ],
        ["request-header-condition", "req-header-condition"],
        ["condition.res.header"]
    ),
    topic!(
        "condition.res.header",
        "conditions",
        "Require a response header or value substring during response resolution.",
        ["when res.header(NAME)", "when res.header(NAME ~ TEXT)"],
        ["example.test res.status(299) when res.header(x-cache ~ hit)"],
        [
            "This condition never matches without explicit response metadata, including when negated."
        ],
        [
            "resHeader",
            "res_header",
            "res-header",
            "response-header-condition",
            "resheader-condition"
        ],
        ["condition.status"]
    ),
    topic!(
        "condition.body",
        "conditions",
        "Search or regex-match the buffered request body.",
        ["when body(~ TEXT)", "when body(/REGEX/[i])"],
        ["example.test status(204) when body(/token=\\d+/)"],
        [
            "Substring matching is case-insensitive over lossy UTF-8.",
            "This condition participates in bounded request-body planning."
        ],
        ["body-condition", "request-body-condition"],
        ["concept.limits"]
    ),
    topic!(
        "condition.client-ip",
        "conditions",
        "Match the normalized downstream client IP against any simple glob.",
        ["when client.ip(PATTERN[, PATTERN...])"],
        ["example.test status(204) when client.ip(203.0.113.*)"],
        ["Socket-address input is normalized to its IP."],
        ["clientIp", "client_ip", "client-ip", "ip"],
        ["condition.server-ip"]
    ),
    topic!(
        "condition.server-ip",
        "conditions",
        "Match the literal/resolved target IP against any simple glob.",
        ["when server.ip(PATTERN[, PATTERN...])"],
        ["example.test status(204) when server.ip(198.51.100.10)"],
        ["A literal IP URL supplies this metadata automatically in CLI simulation."],
        ["serverIp", "server_ip", "server-ip"],
        ["condition.client-ip"]
    ),
    topic!(
        "condition.status",
        "conditions",
        "Match any response status during response resolution.",
        ["when status(CODE[, CODE...])"],
        ["example.test res.header(x-error: yes) when status(500, 502, 503)"],
        [
            "Codes must be 100..599.",
            "This condition never matches without response metadata, including when negated."
        ],
        ["response-status-condition"],
        ["condition.res.header"]
    ),
    topic!(
        "condition.chance",
        "conditions",
        "Deterministically sample requests at thousandth precision.",
        ["when chance(0.0..1.0)"],
        ["example.test tag(canary) when chance(0.1)"],
        [
            "The hash uses URL, method, and source line; the same snapshot input is stable.",
            "Values are rounded to thousandths."
        ],
        ["chance", "sampling"],
        ["concept.ordering"]
    ),
    topic!(
        "condition.env",
        "conditions",
        "Require a process environment variable or exact value.",
        ["when env(NAME)", "when env(NAME=VALUE)"],
        ["example.test status(204) when env(RSPROXY_MODE=debug)"],
        [
            "Names must be non-empty and contain no =, NUL, or whitespace.",
            "Value equality is case-sensitive."
        ],
        ["env", "environment"],
        ["concept.errors"]
    ),
    topic!(
        "condition.any",
        "conditions",
        "Explicit OR over one or more nested conditions.",
        ["when any(CONDITION[, CONDITION...])"],
        ["example.test status(204) when any(method(POST), header(x-beta))"],
        ["At least one nested condition is required."],
        ["any", "or"],
        ["condition.all", "condition.not"]
    ),
    topic!(
        "condition.all",
        "conditions",
        "Explicit AND over one or more nested conditions.",
        ["when all(CONDITION[, CONDITION...])"],
        ["example.test status(204) when all(method(POST), header(x-beta))"],
        [
            "At least one nested condition is required.",
            "Separate when clauses are also ANDed."
        ],
        ["all", "and"],
        ["condition.any", "condition.not"]
    ),
    topic!(
        "condition.not",
        "conditions",
        "Invert a nested condition with response-phase deferral preserved.",
        ["when !CONDITION", "when not(CONDITION)"],
        ["example.test status(204) when !header(authorization)"],
        [
            "Absent response metadata remains non-matching instead of becoming true through negation."
        ],
        ["not", "condition-negation"],
        ["condition.any", "concept.limits"]
    ),
];
