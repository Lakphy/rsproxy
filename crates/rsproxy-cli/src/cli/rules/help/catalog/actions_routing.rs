use super::super::Topic;

pub(super) const TOPICS: &[Topic] = &[
    topic!(
        "action.host",
        "actions",
        "Round-robin direct origin addresses while preserving the original Host header.",
        ["host(ADDRESS[, ADDRESS...])"],
        ["example.test host(127.0.0.1:18081, 127.0.0.1:18082)"],
        ["Selection is per parsed rule and stable within one resolved request."],
        ["host-action"],
        ["action.upstream", "action.direct"]
    ),
    topic!(
        "action.upstream",
        "actions",
        "Route through one proxy or an ordered mixed proxy chain.",
        ["upstream(PROXY_URL[, PROXY_URL...])"],
        ["example.test upstream(proxy://127.0.0.1:8080, socks5://127.0.0.1:1080)"],
        [
            "Supported schemes include proxy/http, https-proxy, socks, and socks5.",
            "Credentials in observable rule text are redacted."
        ],
        ["proxy", "upstream-action"],
        ["action.direct", "action.host"]
    ),
    topic!(
        "action.direct",
        "actions",
        "Force direct origin routing and override matched upstream routing.",
        ["direct"],
        ["example.test direct"],
        ["Combine `direct skip()` for Whistle-style leave-this-request-alone behavior."],
        ["direct-action"],
        ["action.upstream", "action.skip"]
    ),
    topic!(
        "action.mock",
        "actions",
        "Short-circuit with a body, raw HTTP message, or structured inline response.",
        [
            "mock(VALUE)",
            "mock.raw(VALUE)",
            "mock([status=CODE,] [type=MIME,] [header=NAME: VALUE, ...] [body=VALUE])"
        ],
        [
            "example.test mock(status=503, type=application/json, header=X-Mock: yes, body={\"ok\":false})"
        ],
        [
            "Structured and raw mock statuses must be 200..599; structured status defaults to 200.",
            "Statuses 204 and 304 cannot carry a body; response framing headers are owned by the serializer.",
            "File candidates may use | fallback. Directory mocks append a validated request path and reject traversal or escaping symlinks."
        ],
        ["mock.raw", "mockRaw", "mock_raw", "mock-raw"],
        ["concept.values", "action.status"]
    ),
    topic!(
        "action.map.remote",
        "actions",
        "Transparently replace the request origin without sending a redirect.",
        ["map.remote(HTTP_URL)"],
        ["example.test map.remote(http://127.0.0.1:3000)"],
        [
            "A target without a path retains the original path/query; an explicit path replaces them.",
            "Aliases: mapRemote, map_remote, map-remote."
        ],
        ["mapRemote", "map_remote", "map-remote", "map-remote-action"],
        ["action.redirect", "action.url.rewrite"]
    ),
    topic!(
        "action.status",
        "actions",
        "Short-circuit with an HTTP response status.",
        ["status(CODE)"],
        ["example.test status(410)"],
        [
            "CODE must be 200..599; status 204 and 304 responses have no body.",
            "This is a single-action family; the first applicable status wins."
        ],
        ["status-action"],
        ["action.mock", "action.res.status"]
    ),
    topic!(
        "action.redirect",
        "actions",
        "Short-circuit with a Location header and redirect status.",
        ["redirect(URL[, CODE])"],
        ["example.test redirect(https://new.example.test${path}, 307)"],
        [
            "CODE defaults to 302 and must be one of 301, 302, 303, 307, or 308.",
            "URL supports value sources and templates; rendered values reject whitespace, controls, and non-HTTP(S) absolute schemes."
        ],
        ["redirect-action"],
        ["action.map.remote"]
    ),
];
