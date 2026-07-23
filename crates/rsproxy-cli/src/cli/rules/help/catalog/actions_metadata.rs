use super::super::Topic;

pub(super) const TOPICS: &[Topic] = &[
    topic!(
        "action.req.header",
        "actions",
        "Set, remove, or regex-replace request header values.",
        [
            "req.header(NAME: VALUE)",
            "req.header(-NAME)",
            "req.header(NAME ~ /REGEX/REPLACEMENT)"
        ],
        ["example.test req.header(x-release ~ /v(\\d+)/release-$1)"],
        [
            "This family stacks in source order.",
            "Replacement uses Rust regex capture syntax and \\/ for a literal slash."
        ],
        ["request-header", "req-header"],
        ["action.res.header", "concept.values"]
    ),
    topic!(
        "action.res.header",
        "actions",
        "Set, remove, or regex-replace response header values.",
        [
            "res.header(NAME: VALUE)",
            "res.header(-NAME)",
            "res.header(NAME ~ /REGEX/REPLACEMENT)"
        ],
        ["example.test res.header(x-seen: yes)"],
        ["This family stacks in source order and applies after upstream response headers exist."],
        ["response-header"],
        ["action.req.header", "action.res.trailer"]
    ),
    topic!(
        "action.res.status",
        "actions",
        "Rewrite the upstream response status.",
        ["res.status(CODE)"],
        ["example.test res.status(299)"],
        [
            "CODE must be 200..599; changing to 204 or 304 removes the response body, trailers, and body framing.",
            "This is distinct from status(), which short-circuits before an upstream response."
        ],
        ["response-status", "replace-status"],
        ["action.status"]
    ),
    topic!(
        "action.req.method",
        "actions",
        "Rewrite the upstream request method.",
        ["req.method(METHOD)"],
        ["example.test req.method(POST)"],
        ["The rendered value must be a valid HTTP method at execution."],
        ["request-method", "method-action"],
        ["condition.method"]
    ),
    topic!(
        "action.req.cookie",
        "actions",
        "Set or remove request Cookie entries.",
        ["req.cookie(NAME=VALUE)", "req.cookie(-NAME)"],
        ["example.test req.cookie(sid=${id})"],
        ["Cookie actions stack in source order."],
        ["request-cookie", "req-cookie"],
        ["action.res.cookie"]
    ),
    topic!(
        "action.res.cookie",
        "actions",
        "Add or remove Set-Cookie fields, including attributes.",
        [
            "res.cookie(NAME=VALUE[; ATTRIBUTE[=VALUE] ...])",
            "res.cookie(-NAME)"
        ],
        ["example.test res.cookie(sid=1; Path=/; Max-Age=60; HttpOnly; Secure; SameSite=Lax)"],
        ["Common attribute spellings are canonicalized; custom attributes are retained."],
        ["response-cookie", "res-cookie", "set-cookie"],
        ["action.req.cookie"]
    ),
    topic!(
        "action.req.ua",
        "actions",
        "Set the User-Agent request header.",
        ["req.ua(VALUE)"],
        ["example.test req.ua(rsproxy-debugger)"],
        ["Equivalent to a typed User-Agent request-header mutation."],
        ["user-agent", "ua"],
        ["action.req.header"]
    ),
    topic!(
        "action.req.referer",
        "actions",
        "Set the Referer request header.",
        ["req.referer(VALUE)"],
        ["example.test req.referer(https://ref.example.test/)"],
        ["The historical HTTP field spelling Referer is used."],
        ["referer", "referrer"],
        ["action.req.header"]
    ),
    topic!(
        "action.req.auth",
        "actions",
        "Set Basic Authorization credentials.",
        ["req.auth(USER:PASSWORD)"],
        ["example.test req.auth(debug:secret)"],
        ["Treat rule sources and value files containing credentials as secrets."],
        ["basic-auth", "authorization"],
        ["concept.values"]
    ),
    topic!(
        "action.req.forwarded",
        "actions",
        "Set X-Forwarded-For from a rendered IP value.",
        ["req.forwarded(IP)"],
        ["example.test req.forwarded(${clientIp})"],
        ["Socket-address values are normalized to the IP."],
        ["forwarded-for", "xff"],
        ["condition.client-ip"]
    ),
    topic!(
        "action.req.type",
        "actions",
        "Set the media type portion of request Content-Type.",
        ["req.type(MIME)"],
        ["example.test req.type(application/json)"],
        ["Existing charset parameters are preserved."],
        ["request-type", "req-content-type"],
        ["action.req.charset", "action.delete"]
    ),
    topic!(
        "action.req.charset",
        "actions",
        "Set the charset parameter of request Content-Type.",
        ["req.charset(CHARSET)"],
        ["example.test req.charset(utf-8)"],
        ["The media type is preserved."],
        ["request-charset"],
        ["action.req.type"]
    ),
    topic!(
        "action.res.cors",
        "actions",
        "Materialize common or detailed CORS response headers.",
        [
            "res.cors(ORIGIN)",
            "res.cors([origin=VALUE,] [methods=VALUE,] [headers=VALUE,] [credentials=BOOL,] [expose=VALUE,] [max-age=VALUE])"
        ],
        ["example.test res.cors(*, methods=GET POST OPTIONS, credentials=true, max-age=600)"],
        [
            "Boolean values accept true/false, yes/no, 1/0, or on/off.",
            "Accepted key aliases are allow-origin, allow-methods, allow-headers, allow-credentials, expose-headers, and max_age."
        ],
        ["cors", "response-cors"],
        ["action.res.header"]
    ),
    topic!(
        "action.res.type",
        "actions",
        "Set the media type portion of response Content-Type.",
        ["res.type(MIME)"],
        ["example.test res.type(application/json)"],
        ["Existing charset parameters are preserved."],
        ["response-type", "res-content-type"],
        ["action.res.charset"]
    ),
    topic!(
        "action.res.charset",
        "actions",
        "Set the charset parameter of response Content-Type.",
        ["res.charset(CHARSET)"],
        ["example.test res.charset(utf-8)"],
        ["The media type is preserved."],
        ["response-charset"],
        ["action.res.type"]
    ),
    topic!(
        "action.res.merge",
        "actions",
        "Deep-merge an object into a JSON object response.",
        ["res.merge(JSON_OBJECT_VALUE)"],
        ["example.test res.merge({\"ok\":true,\"meta\":{\"source\":\"proxy\"}})"],
        [
            "Non-object, non-JSON, encoded, or over-limit bodies are preserved unchanged.",
            "This family stacks."
        ],
        ["json-merge", "response-merge"],
        ["concept.values", "concept.limits"]
    ),
    topic!(
        "action.res.trailer",
        "actions",
        "Set or remove HTTP/1.1 response trailers.",
        ["res.trailer(NAME: VALUE)", "res.trailer(-NAME)"],
        ["example.test res.trailer(x-checksum: ${random})"],
        ["Trailer actions stack; responses carrying trailers use chunked framing."],
        ["response-trailer", "trailer"],
        ["action.res.header", "action.delete"]
    ),
    topic!(
        "action.attachment",
        "actions",
        "Set Content-Disposition to attachment with an optional filename.",
        ["attachment()", "attachment(FILENAME)"],
        ["example.test attachment(report.txt)"],
        ["Filename accepts structured values and templates."],
        ["download", "content-disposition"],
        ["concept.values"]
    ),
    topic!(
        "action.cache",
        "actions",
        "Set a structured Cache-Control policy or explicitly disable caching.",
        [
            "cache(off)",
            "cache(SECONDS)",
            "cache(DIRECTIVE[, DIRECTIVE...])"
        ],
        ["example.test cache(public, max-age=60, stale-while-revalidate=30, immutable)"],
        [
            "A bare integer becomes max-age.",
            "off also writes Pragma: no-cache; underscore aliases canonicalize to standard directive names."
        ],
        ["cache-control", "caching"],
        ["action.res.header"]
    ),
    topic!(
        "action.tls",
        "actions",
        "Constrain origin TLS and optionally provide an mTLS client identity.",
        [
            "tls(min=1.2|1.3)",
            "tls(ciphers=SUITE[:SUITE...])",
            "tls(client-cert=<PATH>, client-key=<PATH>)",
            "TLS 1.3 suites: TLS_AES_128_GCM_SHA256 | TLS_AES_256_GCM_SHA384 | TLS_CHACHA20_POLY1305_SHA256",
            "TLS 1.2 suites: TLS_ECDHE_{ECDSA|RSA}_WITH_{AES_128_GCM_SHA256|AES_256_GCM_SHA384|CHACHA20_POLY1305_SHA256}"
        ],
        ["example.test tls(min=1.2, ciphers=TLS_AES_128_GCM_SHA256)"],
        [
            "Certificate and key must be configured together.",
            "The policy applies to origin TLS after routing, never to an HTTPS-proxy hop.",
            "TLS 1.3 minimum requires at least one TLS 1.3 suite when ciphers are explicit.",
            "min-version, cipher, common TLS version spellings, and common OpenSSL cipher names are accepted aliases. Cipher lists may use :, |, or ;."
        ],
        ["mtls", "origin-tls", "cipher"],
        ["action.upstream", "concept.errors"]
    ),
];
