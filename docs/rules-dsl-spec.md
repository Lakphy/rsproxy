# rsproxy Rules DSL Spec

Status: executable language-version 3 contract for the grammar listed below, backed by a
machine-readable corpus and the pinned Whistle option/migration contracts.

The same language reference is built into the binary. `rsproxy rules help`
prints the complete stable topic index; query a precise topic such as
`action.req.header`, use an unambiguous shorthand such as `req.header`, list a
category such as `conditions`, or search every ID, syntax form, example, note,
alias, and relation with `rsproxy rules help --search 'response header'`.
`--json` exposes the complete selected records under the versioned
`rsproxy.rules.help/v1` schema, including `language_version` and each topic's
canonical/compatibility `dsl_spellings`; action topics also declare `resolution`
as single/first-wins or stackable. Parser dispatch and this help field
consume the same `rsproxy-rules` registry. Ambiguous shorthand is rejected with
the exact canonical IDs instead of silently choosing one.

Standalone and persisted v3 sources declare their grammar on the first
non-comment, non-blank line:

```text
@language 3
```

V3 source accepts canonical action and condition call names only. The
programmatic compatibility parser remains available for embedded legacy text;
CLI validation, group storage, and daemon reload use the versioned parser. Run
`rsproxy rules migrate FILE` to preview an upgrade or add `--write` to replace
the file after strict validation. See
[`rules-migration-v2-v3.md`](rules-migration-v2-v3.md).

## Line Format

Each non-empty line is:

```text
matcher action... [when condition]... [@important] [@disabled] [@tag:name]
```

Whitespace separates top-level tokens. Whitespace inside `(...)`, quotes, or `<...>` is preserved.
Comments start with `#` outside quotes. Quotes and `()`, `[]`, `{}`, and
`<file>` delimiters must balance and share the published nesting limit. Empty, consecutive, and trailing call
arguments are rejected rather than silently discarded; only explicitly empty
calls such as `attachment()` and `skip()` are accepted.

## Matchers

Supported now:

| Form | Example | Behavior |
| --- | --- | --- |
<!-- corpus:matcher-domain-exact -->
| Domain | `example.com` | Exact host, any scheme, port, path |
<!-- corpus:matcher-suffix-includes-root -->
| Domain suffix | `**.example.com` | Matches `example.com` and any nested subdomain |
<!-- corpus:matcher-one-label-subdomain -->
| One-label subdomain | `*.example.com` | Matches `a.example.com`, not `a.b.example.com` |
<!-- corpus:matcher-effective-default-port -->
| Host + port | `127.0.0.1:18080` | Matches explicit/default effective port |
<!-- corpus:matcher-path-prefix-boundary -->
| Path prefix | `example.com/api` | Prefix match on path boundary |
<!-- corpus:matcher-double-star-crosses-path-segments -->
| Glob path/query | `example.com/api/**` | `*` stays inside a segment, `**` crosses segments |
<!-- corpus:matcher-scheme-positive -->
| Scheme | `http://example.com/a` | Requires scheme |
<!-- corpus:matcher-exact-without-query-allows-query -->
| Exact URL | `=http://example.com/a` | Exact scheme/host/port/path; query is ignored when omitted |
<!-- corpus:matcher-port-only -->
| Port | `:8080` | Matches by effective port |
<!-- corpus:matcher-negation-matches-outside-inner-domain -->
| Negation | `!example.com/private` | Applies when inner matcher does not match |
<!-- corpus:matcher-regex-named-capture -->
| Regex | `/\/users\/(?P<uid>\d+)/` | Regex over the full URL; supports `i` flag and numbered/named captures |

Regex matchers are compiled when rules are parsed. rsproxy uses the Rust `regex` crate by default for linear-time matching. Patterns rejected by `regex` but accepted by `fancy-regex`, such as backreferences and lookaround, automatically use the fancy engine with a hard backtrack limit. When that limit is exceeded, the matcher is treated as not matched. Glob components are translated to anchored linear regexes, deduplicated, and compiled into the immutable rules snapshot; wildcard captures remain non-greedy and only `$1` through `$9` are retained. Request resolution performs no glob compilation and glob matching never uses recursive or exponential backtracking. `rsproxy rules stats` and `rules bench` expose the snapshot's `compiled_globs` count.

Glob and exact matchers validate scheme, authority, and every component glob
before publication. A dangling escape or a glob that cannot be compiled is a
source-located error rather than a rule that silently never matches.
Backslash escapes the next character consistently in matcher and condition
globs; its presence selects whole-component glob matching even when the pattern
contains no wildcard.
Malformed/zero ports, broken bracketed IPv6 authorities, empty schemes, and
exact values that are not URLs return a `matcher` error instead of degrading to
a broader host-only match.

Rule sets are compiled with exact-domain and suffix-domain buckets, plus a global bucket for port, negated, wildcard, and complex regex rules. Simple regexes with a conservative required literal are indexed by an Aho-Corasick multi-literal prefilter and only enter the candidate set when that literal is found. `rsproxy rules stats` reports the compiled index shape, and `rsproxy rules bench --url URL` reports local p50/p99/max resolver timing.

## Actions

Supported now:

Every action help topic exposes a machine-readable `phases` array. Request-only
families run before upstream forwarding, response-only families run after an
upstream response exists, and `delete`, `hide`, `tag`, and `skip` can participate
in both phases. A request-only action guarded by a condition that necessarily
requires response metadata can never execute and is reported by `rules lint`.

| Action | Example | Behavior |
| --- | --- | --- |
<!-- corpus:action-host-round-robin -->
| `host(addr[, addr...])` | `host(127.0.0.1:18081, 127.0.0.1:18082)` | Connect to the next address in per-rule round-robin order while preserving the original Host header |
| `upstream(proxy://h:p[, proxy://h:p \| https-proxy://h:p \| socks5://[user:pass@]h:p...] \| https-proxy://h:p \| socks5://[user:pass@]h:p)` | `upstream(proxy://127.0.0.1:18001, https-proxy://127.0.0.1:18443)` | Forward HTTP requests and CONNECT passthrough through another proxy; comma-separated `proxy://` / `http://` / `https-proxy://` / `socks://` / `socks5://` entries form a mixed upstream chain, including nested multiple-`https-proxy://` TLS hops |
| `mock(value)` | `mock("hello ${host}\n")` / `mock(<mocks>)` / `mock(<a.json\|fallback.json>)` | Short-circuit with inline, `@key`, or file body; file mocks infer Content-Type, try `|`-separated candidates in order, and directory mocks append a traversal-safe request path (`/` -> `index.html`) |
<!-- corpus:action-mock-inline-status-headers-body -->
| `mock(status=..., type=..., header=..., body=...)` | `mock(status=503, type=application/json, header=X-Mock: yes, body={"ok":false})` | One-stop inline mock: combine a final status (default 200, range 200–599), Content-Type, repeatable `header=Name: value` entries, and a body (inline, `@key`, or `<file>`) in one action; 204/205/304 reject a body |
| `mock.raw(value)` | `mock.raw("HTTP/1.1 207 Multi-Status\r\nX-Raw: yes\r\n\r\nbody")` | Short-circuit with raw status line, headers, and body |
<!-- corpus:action-map-remote-transparent-forward -->
| `map.remote(url)` | `map.remote(http://localhost:3000)` / `/^wss?:\/\/a.test\/(.*)$/ map.remote(wss://localhost:3000/$1)` | Transparent reverse proxy (Whistle `pattern target`, Charles Map Remote): the request is served by the target backend while the client-visible URL is unchanged — no 30x is sent. Targets may use `http`, `https`, `ws`, or `wss`. A target without a path keeps the original path and query; an explicit target path (captures such as `$1` supported) replaces them. WebSocket Upgrade requests are matched with their logical `ws`/`wss` URL, while forwarding normalizes to the corresponding HTTP(S) transport and preserves the Upgrade handshake. `ws`/`wss` targets reject non-Upgrade requests. The upstream `Host` header and TLS follow the target. Static inline scheme prefixes are validated even when the path is templated. Aliases: `mapRemote`, `map_remote`, `map-remote` |
| `status(code)` | `status(410)` | Short-circuit with a final 200–599 status response; 204/205/304 cannot carry response content |
| `redirect(url[, code])` | `redirect(https://a.test, 302)` | Short-circuit with 301, 302, 303, 307, or 308 (default 302) |
<!-- corpus:action-header-regex-replace -->
| `req.header(op)` | `req.header(x-added: v)` / `req.header(-x)` / `req.header(x-release ~ /v(\d+)/release-$1)` | Set, remove, or regex-replace every matching request-header value before upstream forwarding |
| `res.header(op)` | `res.header(x-seen: yes)` / `res.header(-server)` / `res.header(location ~ /old/new)` | Set, remove, or regex-replace every matching response-header value |
| `res.status(code)` | `res.status(299)` | Rewrite upstream response status to 200–599; 204/205/304 remove body and trailers, with 205 normalized to `Content-Length: 0` |
| `req.method(M)` | `req.method(POST)` | Rewrite request method |
| `req.cookie(op)` | `req.cookie(sid=1)` / `req.cookie(-sid)` | Set/remove request Cookie entries |
| `res.cookie(op)` | `res.cookie(token=1)` / `res.cookie(token=1; Path=/api; Max-Age=60; HttpOnly; Secure; SameSite=Lax)` / `res.cookie(-token)` | Add/remove Set-Cookie headers, including common Set-Cookie attributes |
| `req.ua(str)` | `req.ua(rsproxy-agent)` | Set User-Agent |
| `req.referer(str)` | `req.referer(https://ref.test/)` | Set Referer |
| `req.auth(user:pass)` | `req.auth(user:pass)` | Set Basic Authorization |
| `req.forwarded(ip)` | `req.forwarded(${clientIp})` | Set `X-Forwarded-For`; socket-address values are normalized to the IP |
| `req.type(mime)` / `res.type(mime)` | `res.type(text/plain)` | Set Content-Type |
| `req.charset(cs)` / `res.charset(cs)` | `res.charset(utf-8)` | Set charset on Content-Type |
| `res.cors(origin[, options...])` | `res.cors(*)` / `res.cors(${reqH.origin}, methods=GET POST OPTIONS, headers=X-Token Content-Type, credentials=true, expose=X-Trace, max-age=600)` | Set common or detailed CORS response headers |
| `cache(directive...)` | `cache(off)` / `cache(60)` / `cache(public, max-age=60, s-maxage=120, stale-while-revalidate=30, immutable)` | Set response Cache-Control; `off` also sets `Pragma: no-cache` |
| `attachment([filename])` | `attachment(file.txt)` | Set Content-Disposition attachment |
| `url.rewrite(from, to)` | `url.rewrite(/old,/new)` / `url.rewrite(/\/api\/v(\d+)/, /v$1)` | Plain string or regex rewrite on path/query before forwarding |
| `url.query(op...)` | `url.query(debug=1, -token)` | Add/update/remove query parameters before forwarding |
| `delete(prop...)` | `delete(pathname.0, urlParams.token, reqHeaders.x-old, reqBody.profile.secret, reqBody.items[1], resBody.meta.debug, trailer.x-old)` | Typed Whistle-compatible deletion of pathname/segments, URL params, headers, cookies, whole bodies or nested JSON/form/JSONP fields, Content-Type type/charset, and response trailers |
| `req.body.set/prepend/append(value)` | `req.body.append("+tail")` | Rewrite buffered request body and Content-Length |
| `req.body.replace(pattern, repl)` | `req.body.replace(/item-(\d+)/, item=$1)` | Regex replace on UTF-8 request bodies |
| `res.body.set/prepend/append(value)` | `res.body.append(@tail)` | Rewrite buffered response body and Content-Length |
| `res.body.replace(pattern, repl)` | `res.body.replace(/raw/i, rewritten)` | Regex replace on UTF-8 response bodies |
| `inject(html|js|css, value[, mode])` | `inject(html, "<!--tail-->")` / `inject(css, "/*head*/", prepend)` | Content-Type gated response body injection; mode is `append` by default |
| `res.merge(json)` | `res.merge({"ok":true,"nested":{"x":1}})` | Deep-merge JSON object responses; non-object/non-JSON responses are left unchanged |
| `res.trailer(op)` | `res.trailer(x-checksum: abc)` / `res.trailer(-x-old)` | Set/remove HTTP/1.1 response trailers; responses with trailers are sent chunked |
| `delay(req|res, d)` | `delay(res, 50ms)` | Sleep before request forwarding or response forwarding |
| `throttle(req|res, speed)` | `throttle(res, 1KB/s)` | Rate-limit buffered and streaming request/response writes with pacing preserved across frames and bounded by the absolute request deadline |
| `tls(min=version, ciphers=list, client-cert=<path>, client-key=<path>)` | `tls(min=1.2, ciphers=ECDHE-ECDSA-AES128-GCM-SHA256)` / `tls(client-cert=<certs/client.pem>, client-key=<certs/client-key.pem>)` | Configure origin TLS minimum version and allowed cipher suites and/or load a PEM client identity for upstream mTLS; policy applies to origin TLS after direct, SOCKS5, or upstream-proxy CONNECT routing, never to an HTTPS proxy hop itself |
| `skip([family...])` | `skip(res.header)` / `skip()` | Skip subsequent actions by validated family/parent, or all subsequent actions when empty, `all`, or `*` |
| `hide` | `hide` | Suppress trace recording for the matched session; other actions still execute |
| `tag(name)` | `tag(api:${path})` | Add `tag:<rendered>` to trace flags; templates are supported |
| `bypass` | `bypass` | Keep CONNECT tunnels in passthrough mode instead of MITM |
| `direct` | `direct` | Force direct origin routing for the request, overriding matched `upstream(...)` actions |

Header and trailer names must be HTTP tokens. After templates, references, and
files are rendered, the runtime also rejects header values containing CR, LF,
NUL, DEL, or other forbidden controls; rewritten methods must remain HTTP
tokens; and rewritten URLs must not contain ASCII whitespace or controls. The
same validation applies to inline and raw mock responses before serialization,
closing CRLF/header-injection paths even for dynamic values. Redirect locations
reject whitespace, controls, and non-HTTP(S) absolute schemes. Response writers
own `Content-Length`, `Transfer-Encoding`, connection, and trailer framing, so
rule/mock headers cannot create conflicting message boundaries.

### Rule ordering: first match wins

**Within a snapshot, rules resolve in group order, then line order, and the
first matching rule wins each single-action family.** This is the opposite of
Whistle and Charles, where the more specific (or later) rule takes precedence.
When migrating, put specific rules *above* broader wildcard rules:

```text
api.foo.test upstream(socks5://127.0.0.1:2222)   # specific first
*.foo.test   upstream(socks5://127.0.0.1:1111)   # wildcard after
```

With the reversed order the wildcard rule silently swallows every request,
including `api.foo.test`. `rsproxy rules lint` detects provable cases of this
shadowing; it also reports repeated single-action families within one rule and
flat or `all(...)` method/status/environment constraints whose positive and
directly negated sets have no possible value. Constant chance/boolean trees such
as `chance(0)` are covered too. It also reports request-only actions guarded by
conditions that necessarily require response metadata, actions suppressed by an
earlier same-rule `skip`, conflicting local responses, response-only actions
bypassed by a local response, `upstream` overridden by same-rule `direct`, and
body mutations made unobservable by `res.status(204/205/304)`.
Local response precedence is fixed as `status`, then `redirect`, then `mock`;
those responses do not enter the upstream response-action pipeline. The linter
deliberately stays silent for cases requiring regex overlap, dynamic environment
values, ambiguous `any(...)` phase inference, or cross-rule capability
inference. A clean lint run is therefore not a completeness proof. `@important`
moves a rule ahead of all non-important rules when reordering lines is not
practical.

Lint JSON uses schema `rsproxy.rules.lint/v1`. Every finding has a stable `kind`
(`shadowed-rule`, `duplicate-single-family`, `unsatisfiable-conditions`,
`request-action-requires-response`, `action-after-skip`,
`conflicting-terminal-actions`, `response-action-with-local-response`, or
`upstream-overridden-by-direct`, or `body-action-with-bodyless-status`), source `group`, one-based `line`, source
`rule`, human `message`, and
`families`. Shadow findings additionally identify the earlier rule with
`shadowed_by_group`, `shadowed_by_line`, and `shadowed_by_rule`. Automation must
branch on `kind`, not the human message.

Whistle's `pattern $0` ("leave this request alone, ignore later rules") is
expressed as an early `direct skip()` rule: `direct` forces origin routing and
`skip()` suppresses every later action for the matched request.

Single-action families use first-match semantics. Header, cookie, body, query,
`delete`, `inject`, `res.merge`, `res.trailer`, and tag families are stackable.
`skip` is retained in explain/trace and applies to actions resolved after it in rule order.

The public action-family specifications generate `Action::FAMILIES`,
`Action::STACKABLE_FAMILIES`, stackability, and phase lookup from one
declaration. Help `resolution`/`phases` fields and the machine-readable action
corpus must match that declaration. The corpus runner fails if the implementation,
declaration, and resolved families differ. Value-source matrices cover every
structured value slot, and the action-effect suite assigns each family an
executable real-network owner. `scripts/verify.sh actions` runs these contracts
together.

`host` keeps one atomic cursor per parsed rule. Each resolved request selects one
address lazily and reuses it for trace planning, TLS policy, pool-key generation,
and forwarding, so concurrent requests are balanced without allowing repeated
route inspection inside one request to advance the cursor.

`delete` compiles every property to a typed `DeleteOp`; unknown properties and
empty, overlong, or malformed body paths fail with an `action` error. Pathname
indexes in one call are evaluated against the original URL path; negative
indexes count from the end, while `pathname.last` preserves a trailing slash.

Nested request paths support JSON and `application/x-www-form-urlencoded`;
nested response paths support JSON and JavaScript/JSONP wrappers. Dot separates
object keys, a trailing `[n]` selects an array item, and a backslash preserves a
literal separator or special character (`\.`, `\ `, `\|`, `\&`, `\n`, `\r`,
`\t`, `\f`, `\v`). Form deletion matches the raw field name reconstructed from
that path and removes every duplicate occurrence without reordering survivors.
The parser limits a path to 16KiB and 128 segments.

Body-property deletion is deliberately conservative: invalid UTF-8/JSON,
missing paths, incompatible Content-Type, or non-identity Content-Encoding
leaves the body unchanged. Request JSON requires a JSON media type; response
JSON also accepts a missing Content-Type, while JSONP requires a JavaScript or
JSONP media type. Whole-body and nested deletion use the normal bounded body
planner: over-limit streams preserve the original body, retain non-body delete
effects, and carry the existing rewrite-skipped flag.

Header replacement uses `/regex/replacement`: the first unescaped `/` after the
opening delimiter separates the regex from the replacement. Write `\/` for a
literal slash in either part. Replacement text follows Rust `regex` capture
syntax (`$1`, `${name}`); invalid patterns fail during rule parsing with an
`action` error rather than on the proxy hot path.

`tls(min=...)` accepts TLS `1.2` or `1.3` aliases. `ciphers` accepts `:` / `|` / `;` separated IANA names and common OpenSSL aliases for rustls aws-lc's safe TLS 1.3 AES-128/AES-256/ChaCha20 suites and TLS 1.2 ECDHE-ECDSA/ECDHE-RSA AES-128/AES-256/ChaCha20 suites. Unknown suites, TLS versions below 1.2, a TLS 1.3 minimum with only TLS 1.2 suites, and unpaired client certificate/key options fail during `rules check`.

## Value Sources

Every action field represented by a structured value accepts these forms:

| Form | Meaning |
| --- | --- |
| `plain` or `"quoted"` | Inline UTF-8 text; quotes are removed and templates/captures are rendered |
| `@key` | Bytes from `<storage>/values/key`; UTF-8 content is rendered at action execution time |
| `<path>` | File bytes; storage-relative lookup is attempted before the path as written |

Value keys are 1-128 ASCII letters, digits, dots, underscores, or hyphens.
Invalid keys such as `@../escape`, `@bad/key`, and bare `@` fail rule parsing;
runtime validation repeats this check for programmatically constructed actions.
Quote a leading marker to keep it literal, for example `tag("@literal")`.

Text-only fields such as methods, headers, cookies, URLs, routing addresses,
CORS, cache, merge JSON, and attachment names reject non-UTF-8 loaded bytes.
Body, injection, and mock payload fields preserve binary bytes; loaded content
is template/capture-rendered only when it is valid UTF-8. File paths may contain
templates. They are a trusted-rule filesystem capability and are not restricted
to the storage directory. When a mock value selects a directory, the appended
request path must contain only normal components; dot traversal, platform
separator tricks, and a resolved symlink outside that directory are rejected.

Regex replacement operands remain distinct from ordinary templates. Header and
body regex replacements use their inline replacement grammar. A regex
`url.rewrite` target may come from `@key` or `<file>`, and `$1`/`${name}` in that
loaded target are preserved for the URL regex engine instead of being consumed
as matcher templates.

## Whistle Migration Contract

`tests/contracts/whistle_migration.toml` records one supported action mapping
for each public rsproxy family, plus supported syntax mappings and aliases.
Every action mapping names documentation or unit-test evidence in the pinned
Whistle 2.10.5 fixture and contains an rsproxy rule that must parse and resolve
to the declared family.
The runner rejects missing evidence, duplicate mappings, and family-set drift.

The runner also parses the exact protocol and alias declarations in the
fixture's `lib/rules/protocols.js`: every canonical protocol and explicit alias
must be classified as a supported action/syntax form or an explicit
deferred/removed capability. This closes source-registry omissions; it does not
claim behavioral parity for every uncommon option accepted by those names.

`tests/contracts/whistle_options.toml` separately classifies every option
extracted from the fixture's English `enable`, `disable`, and `delete`
documents. The runner rejects omissions and duplicates and parses/resolves
every recipe marked `implemented`. Native defaults and deferred/removed
behavior remain explicit. `process-config` items must name an option that
exists in the CLI help; behavior outside the v1 action table must be identified
as v2 rather than attached to a completed milestone. Documented nested
JSON/form request deletion and JSON/JSONP response deletion are covered by
parser, body-planning, transform, contract, and real-proxy tests.

The same matrix explicitly records currently deferred or removed capabilities,
including `pipe`, `sniCallback`, general `tpl`, write-to-file actions, request
CORS, PAC/style, log, and Weinre. This is an executable migration reference, not
the planned v2 `rules import --from-whistle` converter.

Whistle `script://` and `resScript://` are also not executable rule actions.
Use typed request/response header, body, CORS, merge, and injection actions
where possible. `rsproxy rules help concept.scripting` documents the extension
boundary: any future general program hook must run behind a separately
versioned sandbox with explicit capabilities and resource budgets, rather than
executing arbitrary host code inside the proxy.

## Conditions

Supported now:

| Condition | Example | Behavior |
| --- | --- | --- |
<!-- corpus:condition-method-falls-through -->
| `method(...)` | `when method(GET, POST)` | Case-insensitive method match |
<!-- corpus:condition-host-glob -->
| `host(pattern)` | `when host(**.example.com)` | Uses the same host glob semantics |
<!-- corpus:condition-url-regex -->
| `url(pattern)` | `when url(*mode=debug*)` / `when url(/\/api\/v\d+/)` | Full URL glob or regex match |
<!-- corpus:condition-header-presence -->
| `header(name)` | `when header(authorization)` | Header presence |
<!-- corpus:condition-header-contains -->
| `header(name ~ value)` | `when header(accept ~ json)` | Case-insensitive substring |
<!-- corpus:condition-body-substring -->
| `body(~ value)` | `when body(~ beta-token)` | Case-insensitive request body substring |
<!-- corpus:condition-body-regex -->
| `body(/regex/i)` | `when body(/token=\d+/)` | Request body regex match |
<!-- corpus:condition-response-header-presence -->
| `res.header(name)` | `when res.header(x-origin-state)` | Response header presence, evaluated only after upstream response headers are available |
<!-- corpus:condition-response-header -->
| `res.header(name ~ value)` | `when res.header(x-origin-state ~ hit)` | Case-insensitive response header substring, evaluated during response phase |
<!-- corpus:condition-client-ip-glob -->
| `client.ip(...)` | `when client.ip(127.0.0.1)` / `when client.ip(203.0.*)` | Client IP exact or simple glob match; socket-address values are normalized to IP |
<!-- corpus:condition-server-ip -->
| `server.ip(...)` | `when server.ip(127.0.0.1)` | Request target literal IP exact or simple glob match; socket-address values are normalized to IP |
<!-- corpus:condition-response-status -->
| `status(...)` | `when status(200, 404)` | Evaluated during response phase |
<!-- corpus:condition-chance-one-always-matches -->
| `chance(0.0-1.0)` | `when chance(0.1)` | Deterministic hash sampling |
<!-- corpus:condition-env-missing-falls-through -->
| `env(name)` / `env(name=value)` | `when env(RSPROXY_MODE=dogfood)` | Process environment presence or exact value match |
<!-- corpus:condition-any -->
| `any(...)` | `when any(method(POST, PUT), header(x-mode ~ beta))` | Explicit OR across nested conditions |
<!-- corpus:condition-all -->
| `all(...)` | `when all(method(POST), header(x-mode ~ beta))` | Explicit AND across nested conditions; useful inside `any(...)` |
<!-- corpus:condition-negated-header-presence -->
| Negation | `when !header(authorization)` | Inverts the condition |
<!-- corpus:condition-not-call-form -->
| `not(...)` | `when not(method(GET))` | Call form of negation, equivalent to `!`; composes inside `any(...)`/`all(...)` |

Multiple `when` clauses are ANDed. Multiple values inside one condition are
ORed. Empty method/IP/status lists, invalid HTTP method/header tokens, empty
contains operands, invalid chance ranges, and empty environment names are
`condition` errors. Response-phase conditions (`status` and `res.header`) do not
match during request-only `rules test`/`rules bench`; negating or nesting them
does not turn absence of a response into a match. `rules test --response-status
CODE [--response-header 'Name: value']...` supplies an explicit response
snapshot and resolves them without proxy traffic.

<!-- corpus:composition-group-order -->
Across enabled groups, group order precedes line order. Within that order,
`@important` rules move ahead of non-important rules. Single-action families use
the first condition-satisfying action; stackable families preserve source order.

<!-- corpus:composition-skip-family -->
`skip(family...)` suppresses later actions in the named family without removing
unrelated actions. Family names are canonicalized during parsing and must name
an action family or a parent such as `res.body`; unknown names are rejected.
`skip()`, `skip(all)`, and `skip(*)` suppress all later
actions.

## Error Contract

<!-- corpus:error-header-replace-regex -->
Every parse error exposes `code`, `group`, `line`, and `message`. Stable code
values are `syntax`, `matcher`, `action`, `condition`, and `property`; callers
must not parse the human-readable message.

<!-- corpus:error-syntax -->
Tokenizer and line-shape failures use `syntax`.

<!-- corpus:error-action -->
Unknown or malformed action calls use `action`; equivalent stage-specific codes
apply to matchers, conditions, and properties.

## Template Variables

Supported in template-capable action values:

`${id}`, `${now}`, `${random}`, `${randomUUID}`, `${url}`, `${host}`,
`${hostname}`, `${port}`, `${path}`, `${pathname}`, `${query}`, `${search}`,
`${method}`, `${clientIp}`, `${serverIp}`, `${statusCode}`, `${reqH.name}`,
`${resH.name}`, `${reqCookies.name}`, and `${resCookies.name}`.

<!-- corpus:template-request-context -->
Request variables come from one immutable `RequestMeta` snapshot. `id` is a
32-character lowercase hexadecimal request identifier, `now` is request
metadata creation time in Unix milliseconds, `random` is an unsigned 64-bit
decimal value, and `randomUUID` is a UUID v4 string. These generated values are
stable across every action and the request/response rule phases of one request.
`port` is the effective URL port, including scheme defaults. Header lookup is
case-insensitive; cookie names are case-sensitive and values are read from
`Cookie` entries.

<!-- corpus:template-response-context -->
`statusCode`, `resH.*`, and `resCookies.*` read the immutable upstream response
snapshot used for response-period rule resolution. They are empty before a
response exists. Response cookies are read from the first matching `Set-Cookie`
name/value pair.

`${var.replace(/regex/, replacement)}` applies a regex replacement to any
variable. The regex accepts an optional `i` flag, escaped slashes use `\/`, and
replacement captures use `$1` or `${name}`. Parsed actions validate transform
syntax and regexes before publication; a bounded thread-local cache avoids
recompiling active transform patterns on every render. Escape a literal closing
brace inside a transform as `\}`.

<!-- corpus:error-template-replace-regex -->
Malformed or unterminated templates and invalid replace regexes are `action`
parse errors. Regex matcher captures support `$0` for the complete match,
`$1` through `$9`, and `${name}`; glob wildcards populate `$1` through `$9`.

`rsproxy rules test <url> [-X METHOD] [-H 'Name: value']... [--body TEXT|-d TEXT] [--client-ip IP] [--server-ip IP] [--response-status CODE] [--response-header 'Name: value']...` injects the same request and optional response metadata used by the proxy path. The response options work through both the authenticated control API and offline storage fallback. `rules bench` remains request-only. Quote URLs containing `?` or `&` (zsh expands them as globs): `rsproxy rules test 'https://x.test/api?a=1'`.

When a concrete HTTPS/WSS test resolves `map.remote`, `rules test` reports the
non-fatal `https-mitm-unavailable` warning if MITM is disabled or the selected
storage has no initialized CA. `rules lint` reports the same condition
conservatively when the ruleset contains `map.remote`; the warning is separate
from semantic findings and does not change lint's exit status. Initialize and
trust the CA with `rsproxy ca init && rsproxy ca install`, then restart rsproxy.

## Scope and Limits

This document lists the complete supported language-version 2 grammar; unknown forms fail
validation instead of silently degrading to a broader behavior. The Whistle
migration contract is a classified compatibility surface, not a promise that
rsproxy accepts Whistle syntax or implements every Whistle option.

Version 2 preserves every documented v1 rule form while rejecting malformed
forms that v1 sometimes tolerated accidentally. See
[`rules-migration-v1-v2.md`](rules-migration-v1-v2.md) for the exact detection
and rewrite checklist.

The public Rust model now enforces the same immutability claim: use
`RuleSet::rules()`, `RuleSet::version()`, and `RuleSet::is_empty()` for
inspection. The AST and its compiled candidate/glob indices can no longer be
mutated independently after publication.

One snapshot accepts at most 16,777,216 UTF-8 source bytes, 1,024 groups, and
10,000 non-empty, non-comment source rule lines. Group identifiers are limited to 128 bytes, and one
parse returns at most 256 diagnostics; the last diagnostic explicitly states
when remaining source was not parsed. One physical source line is limited to
65,536 UTF-8 bytes. A snapshot retains at most 100,000 actions, 100,000
condition AST nodes, and 256 body substring/regex condition leaves. A rule
accepts at most 256 actions, 256 total condition AST
nodes, 64 property tokens, and 256 top-level arguments per call. Matcher
negation, condition composition/negation, and delimiter nesting are each
limited to 32 levels. The parser rejects over-limit input with a source-located
`syntax`, `matcher`, `action`, `condition`, or `property` code.

Every execution-time `@key`, `<path>`, or mock-file read accepts at most
8,388,608 bytes. Every PEM certificate-chain or private-key file read accepts
at most 1,048,576 bytes. Readers consume at most one byte beyond the limit to
detect overflow, then return an explicit error instead of allocating the whole
file. A mock candidate falls through only when it is not found; an oversized,
unreadable, or otherwise invalid candidate fails the action immediately.

Rendered filesystem paths are limited to 4,096 bytes. One rendered action value
is limited to 8,388,608 bytes; one trace tag is limited to 4,096 bytes and at
most 256 distinct rule tags are retained per request. Template transforms and
plain/regex replacements compute their exact expanded length before allocating.
URLs and header blocks additionally obey `max_header_size`/`max_header_count`;
body set/append/prepend/replace/inject/merge outputs obey `body_buffer_limit`.
Cross-phase matched-rule provenance and captures use shared immutable storage,
so stacked actions do not duplicate source lines or large capture strings.
Human explanations render at most 4,096 bytes per embedded value and 8,388,608
bytes overall; a stable marker reports value or total-output truncation.
Rendered upstream chains accept at most 32 proxy hops, and one mock file source
tries at most 32 `|`-separated candidates.

Each shadow-lint run performs at most 1,000,000 pairwise comparisons and charges
at most 268,435,456 matcher-source bytes across those comparisons. Shadow and
semantic reports each retain at most 10,000 findings and approximately
4,194,304 source/message bytes. `lint_report()` / `semantic_lint_report()` and
CLI lint JSON expose `complete`; reaching a budget produces `complete: false`
and a failing CLI exit rather than a misleading clean result. The legacy Rust
`lint()` methods return the bounded finding prefix for source compatibility.

Delete body paths are separately limited to 16,384 bytes and 128 segments;
value keys are limited to 128 bytes; regex/glob captures expose at most `$1`
through `$9`. These are acceptance limits, not best-effort runtime timeouts: an
invalid snapshot is never partially published. Query the current contract with
`rsproxy rules help concept.limits` or inspect the `limits` object in help JSON.

CLI rule files, editor results, and stdin are read through a one-extra-byte
bounded reader, so the source limit is enforced before an unbounded `String`
allocation. Persistent group files share one cumulative 16 MiB read budget;
the group manifest is separately limited to 1 MiB and directory/manifest group
counts are rejected before all files are loaded. Direct Rust callers already
own their input strings, but parsing and compiled-snapshot amplification remain
bounded by the same public constants.

`RuleSet::version()` is a process-local monotonically increasing publication
identifier seeded from Unix milliseconds. Concurrent publications are unique,
wall-clock rollback cannot reduce the value, and clones retain the same ID; it
is not a persistent timestamp or a content hash.

Body conditions and mutations require bounded aggregation. When a body exceeds
`body_buffer_limit`, the proxy preserves streaming, skips operations that need
the complete body, and records the corresponding trace flag. Value files and
`<path>` sources are trusted-rule filesystem capabilities and must not be
accepted from untrusted rule authors.

Within one resolution, lossy UTF-8 body decoding and normalized client/server
IP values are computed at most once and reused across nested conditions. ASCII
case-insensitive body literals are semantically deduplicated into one compiled
Aho-Corasick matcher and scan the request body once; header substring checks do
not allocate lowercase copies. The snapshot body-condition cap bounds remaining
full-body regex scans.
