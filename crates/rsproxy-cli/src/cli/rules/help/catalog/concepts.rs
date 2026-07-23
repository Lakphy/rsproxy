use super::super::Topic;

pub(super) const TOPICS: &[Topic] = &[
    topic!(
        "concept.rule",
        "concepts",
        "Complete source-line grammar and comment rules.",
        [
            "MATCHER ACTION [ACTION ...] [when CONDITION ...] [@PROPERTY ...]",
            "# comment"
        ],
        ["example.test status(204) when method(GET) @tag:health"],
        [
            "Whitespace separates top-level tokens; whitespace inside calls is preserved.",
            "A # starts a comment outside quotes. Every non-empty rule needs at least one action.",
            "Quotes and (), [], {}, and <file> delimiters must balance within the published nesting limit."
        ],
        ["syntax", "rule"],
        [
            "concept.ordering",
            "matchers",
            "actions",
            "conditions",
            "properties"
        ]
    ),
    topic!(
        "concept.ordering",
        "concepts",
        "Deterministic group, line, priority, first-match, stacking, and skip semantics.",
        [
            "group order -> source line order",
            "@important rules -> ordinary rules",
            "single family: first applicable action wins",
            "stackable family: every applicable action is retained"
        ],
        ["api.example.test status(201)\n*.example.test status(202)"],
        [
            "Specific rules must precede broader rules.",
            "Stackable families are headers, cookies, query, delete, body operations, inject, merge, trailers, tag, and skip.",
            "Use `rsproxy rules lint` to detect provable shadowing, repeated single-action families, contradictory conjunctions, impossible phase guards, and ineffective skip/terminal/route combinations; use `rules test` to inspect a concrete request."
        ],
        ["ordering", "precedence", "first-match", "stacking"],
        ["property.important", "action.skip"]
    ),
    topic!(
        "concept.values",
        "concepts",
        "Inline, named, and file-backed action values.",
        ["plain", "\"quoted text\"", "@VALUE_KEY", "<PATH>"],
        ["example.test mock(@maintenance-page) tag(<labels/route.txt>)"],
        [
            "Keys are 1-128 ASCII letters, digits, dots, underscores, or hyphens.",
            "Quote a leading @ or < to keep it literal.",
            "Each external @key, <path>, or mock file is limited to 8388608 bytes before allocation.",
            "File access is an intentional trusted-rule capability; paths may be storage-relative or as written.",
            "Directory mocks validate request-path components and reject traversal or symlink escape outside the selected directory."
        ],
        ["values", "value-source", "reference", "file-value"],
        ["concept.templates", "action.mock"]
    ),
    topic!(
        "concept.templates",
        "concepts",
        "Request, response, cookie, header, generated, and matcher-capture interpolation.",
        [
            "$0 .. $9",
            "${name}",
            "${id} ${now} ${random} ${randomUUID}",
            "${url} ${host} ${hostname} ${port} ${path} ${pathname} ${query} ${search} ${method}",
            "${clientIp} ${serverIp} ${statusCode}",
            "${reqH.NAME} ${resH.NAME} ${reqCookies.NAME} ${resCookies.NAME}",
            "${VAR.replace(/REGEX/[i], REPLACEMENT)}"
        ],
        ["/users\\/(\\d+)/ req.header(x-user: $1) tag(${host}:${path})"],
        [
            "Generated values are stable within one request metadata snapshot.",
            "Response variables are empty before response resolution.",
            "Unknown named variables render empty; malformed programmatic placeholders remain literal."
        ],
        ["templates", "template", "variables", "captures"],
        ["matcher.regex", "concept.values"]
    ),
    topic!(
        "concept.errors",
        "concepts",
        "Stable, source-located parse errors and conservative lint finding kinds.",
        [
            "parse: syntax | matcher | action | condition | property",
            "lint: shadowed-rule | duplicate-single-family | unsatisfiable-conditions",
            "lint: request-action-requires-response | action-after-skip",
            "lint: conflicting-terminal-actions | response-action-with-local-response | upstream-overridden-by-direct",
            "lint: body-action-with-bodyless-status"
        ],
        ["example.test status(204)"],
        [
            "Parse diagnostics carry code, group, one-based line, and human-facing message; any parse error rejects the ruleset atomically.",
            "Lint JSON uses rsproxy.rules.lint/v1; findings carry kind, source location, rule, message, and families.",
            "Automation must branch on stable code/kind values and must not parse message text."
        ],
        ["errors", "diagnostics", "error-codes", "lint-kinds"],
        [
            "concept.limits",
            "concept.compatibility",
            "concept.ordering"
        ]
    ),
    topic!(
        "concept.compatibility",
        "concepts",
        "Versioned source compatibility, parser spellings, and migration policy.",
        [
            "rule language version: 3; standalone and persisted sources start with @language 3",
            "rsproxy rules help --json",
            "topics[].dsl_spellings[].canonical | aliases"
        ],
        ["example.test status(204)"],
        [
            "Additive unambiguous syntax may retain the language version.",
            "Removing an accepted spelling or changing accepted-source meaning requires a version bump and migration notes.",
            "Help query aliases are not DSL aliases unless listed under dsl_spellings."
        ],
        ["version", "compatibility", "migration", "dsl-spellings"],
        ["concept.errors", "concept.rule"]
    ),
    topic!(
        "concept.limits",
        "concepts",
        "Parser and runtime bounds that make hostile or generated rules predictable.",
        [
            "snapshot source <= 16777216 bytes; groups <= 1024; rules <= 10000",
            "rule line <= 65536 bytes",
            "group name <= 128 bytes; diagnostics <= 256",
            "per snapshot: actions <= 100000; condition nodes <= 100000",
            "per snapshot: body substring/regex condition leaves <= 256",
            "per rule: actions <= 256; condition nodes <= 256; properties <= 64",
            "call arguments <= 256",
            "external/final value <= 8388608 bytes; rendered path <= 4096 bytes; PEM file <= 1048576 bytes",
            "trace tag <= 4096 bytes; distinct rule tags per request <= 256",
            "explain rendered value <= 4096 bytes; total explanation <= 8388608 bytes",
            "upstream proxy chain <= 32 hops; mock file list <= 32 candidates",
            "lint <= 1000000 shadow comparisons and <= 268435456 charged matcher bytes; 10000 findings and 4194304 source/message bytes per report",
            "matcher/condition/delimiter nesting <= 32",
            "delete body path <= 16384 bytes and <= 128 segments",
            "named value key <= 128 bytes",
            "fancy-regex backtracking <= 100000 steps",
            "captures: $0 plus $1..$9"
        ],
        [
            "example.test delete(reqBody.profile.secret) when all(method(POST), header(content-type ~ json))"
        ],
        [
            "Oversized or over-nested source is rejected before publication.",
            "Diagnostics are capped; the final diagnostic states that remaining source was not parsed.",
            "CLI files/stdin and stored rule groups use bounded readers before allocation; persisted groups share the snapshot source budget.",
            "CLI/control value storage and execution-time @key, <path>, mock, certificate, and private-key reads reject one byte beyond their fixed limits.",
            "Linear glob programs are validated, deduplicated, and compiled into the immutable snapshot; advanced regex backtrack exhaustion is a non-match.",
            "Case-insensitive body literals are deduplicated into one snapshot matcher and scan each request body once per resolution.",
            "Template and regex replacement lengths are computed before allocation; URL/header/body outputs use the stricter protocol or configured buffer budget.",
            "Request body UTF-8 decoding and normalized client/server IP values are cached once per resolution and shared by nested conditions.",
            "Lint JSON exposes complete=false when a comparison-count, charged matcher-byte, finding, or report-byte budget prevents a complete audit.",
            "Rendered HTTP header/trailer values, methods, URLs, and mock response fields are protocol-validated before serialization; controls and CRLF injection fail the action.",
            "Request-body buffering is separately bounded by runtime configuration."
        ],
        ["limits", "security", "complexity", "bounds"],
        ["concept.errors", "matcher.regex", "action.delete"]
    ),
    topic!(
        "concept.groups",
        "concepts",
        "Ordered, independently enabled source groups compiled into one immutable snapshot.",
        [
            "rsproxy rules ls",
            "rsproxy rules set GROUP --file FILE",
            "rsproxy rules enable|disable GROUP"
        ],
        ["example.test status(204)"],
        [
            "Enabled group order precedes line order.",
            "A valid replacement is atomic; invalid source leaves the active group unchanged."
        ],
        ["groups", "rule-groups"],
        ["concept.ordering"]
    ),
];
