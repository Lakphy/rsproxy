# rsproxy Dogfooding Report

Date: 2026-07-10
Last updated: 2026-07-11

Build verified with:

```text
cargo test --workspace
cargo build --workspace
```

Runtime setup:

```text
rsproxy run --host 127.0.0.1 --port 18899 --api 127.0.0.1:18900 --storage /tmp/rsproxy-dogfood
```

## Loop 1

Rules:

```text
127.0.0.1:18080/index.txt res.header(x-rsproxy: dogfood-v1)
127.0.0.1:18080/mock mock("mock-v1 ${host}\n")
```

Observed:

- `rules check`, `rules set`, and `rules test` worked.
- `curl -x` through rsproxy forwarded to the local origin and added `X-Rsproxy: dogfood-v1`.
- `mock(...)` short-circuited correctly and rendered `${host}`.
- `trace ls` recorded both sessions.

Optimization from observation:

- CLI trace output was raw JSON by default. Added `/api/sessions.txt` and made `rsproxy trace ls` default to a table, with `--json` preserving machine-readable output.

## Loop 2

Rules:

```text
127.0.0.1:18080/post-only status(410) when method(POST)
127.0.0.1:18080/post-only status(200)
127.0.0.1:18080/delay delay(res, 50ms) res.header(x-delay: yes)
```

Observed:

- GET fell through to line 2 and returned 200.
- POST matched line 1 and returned 410.
- Response delay was visible in trace duration and curl timing.
- `trace ls` table worked after a normal `cargo build`.
- Trace initially reported the skipped line 2 as matched for POST, and status short-circuit responses reported `response_bytes: 0`.

Optimizations from observation:

- `MatchedRule` is now recorded only when an action is actually accepted after first-match filtering.
- Status short-circuit responses now record response byte count and body head.
- Added a regression test for skipped single-family rules not being reported as matched.

## Loop 3

Rule:

```text
fake.local/echo host(127.0.0.1:18081) req.header(x-added: dogfood-v3) res.header(x-rsproxy: host-rewrite)
```

Observed:

- `curl -x http://127.0.0.1:18899 http://fake.local/echo` connected to `127.0.0.1:18081`.
- The origin saw `Host: fake.local`.
- The origin saw `X-Added: dogfood-v3`.
- The client saw `X-Rsproxy: host-rewrite`.
- Trace recorded upstream as `127.0.0.1:18081`.
- Trace initially captured request headers before request rewrite, so `X-Added` was missing.

Optimization from observation:

- Trace request headers are now refreshed after request rewrite, so `req.header(...)` effects are visible in `trace get`.

## Loop 4

Rules:

```text
127.0.0.1:18082/echo url.query(debug=1, -remove) req.body.append("+REQ") res.body.append(@tail) res.header(x-body-rule: yes)
```

CLI setup:

```text
printf 'VALUE-TAIL\n' | rsproxy values set tail --api 127.0.0.1:18900
```

Observed:

- `values set` wrote `tail` and `values ls` listed it.
- `rules test 'http://127.0.0.1:18082/echo?remove=1'` explained `url.query`, `req.body.append`, `res.body.append`, and `res.header`.
- `curl -x` POST rewrote `/echo?remove=1` to `/echo?debug=1`.
- The origin received body `BODY+REQ`.
- The client received response body appended with `VALUE-TAIL`.
- `trace export --json` and `trace export --har` wrote files.

Optimizations from observation:

- `values ls` default output was raw JSON, so `/api/values.txt` was added and the CLI now defaults to one key per line; `--json` keeps JSON.
- Trace did not expose request body capture, so `req_body_head` was added to session detail/export.

## Loop 5

Rule:

```text
127.0.0.1:18082/status res.status(299) res.header(x-status-rewrite: yes) res.body.prepend("rewritten\n")
```

Observed:

- `curl -x` saw `HTTP/1.0 299 OK`.
- The response included `X-Status-Rewrite: yes`.
- The response body was prepended with `rewritten`.
- `trace ls` showed status `299`, and `trace get` included rewritten body and `Content-Length`.

Optimization from observation:

- Added `res.status(code)` support as the response-phase status rewrite action corresponding to the design's `replaceStatus` capability.

## Loop 6

Rule:

```text
127.0.0.1:18083/old upstream(proxy://127.0.0.1:18898) url.rewrite(/old,/new) req.ua(rsproxy-agent) req.referer(http://ref.test/) req.auth(user:pass) req.cookie(sid=abc) req.type(text/plain) req.charset(utf-8) res.cookie(token=xyz) res.cors(*) res.type(text/plain) res.charset(utf-8) cache(off) attachment(file.txt) throttle(res, 1KB/s)
```

Runtime setup:

- Echo origin on `127.0.0.1:18083`.
- Upstream rsproxy on `127.0.0.1:18898`.
- Main rsproxy on `127.0.0.1:18899`.

Observed:

- `rules test` explained `upstream`, `url.rewrite`, request header/cookie/auth/type/charset actions, response cookie/CORS/type/charset/cache/attachment actions, and `throttle`.
- `curl -x http://127.0.0.1:18899 http://127.0.0.1:18083/old` reached the origin as `/new`.
- The origin saw `User-Agent: rsproxy-agent`, `Referer: http://ref.test/`, `Authorization: Basic dXNlcjpwYXNz`, `Cookie: sid=abc`, and `Content-Type: text/plain; charset=utf-8`.
- The client saw `Set-Cookie: token=xyz`, CORS headers, `Cache-Control: no-store`, `Pragma: no-cache`, `Content-Disposition: attachment; filename="file.txt"`, and `Content-Type: text/plain; charset=utf-8`.
- The upstream proxy trace recorded the forwarded request, proving HTTP `upstream(proxy://...)` was used.
- Response throttling made the curl request take about 2 seconds for a 2179-byte response at `1KB/s`.

Optimization from observation:

- Main proxy trace was initially empty when queried immediately after curl because throttled writes slept after the final chunk; curl had already received the body while the proxy thread was still waiting before recording trace. `write_maybe_throttled` now skips the post-write sleep for the final chunk, and immediate `trace ls` shows the session.

## Loop 7

Rule:

```text
/users\/(?P<uid>\d+)\/orders\/(?P<order>\w+)/ req.header(x-uid: ${uid}) req.header(x-order: ${order}) res.header(x-regex-order: ${order})
```

Runtime setup:

- Echo origin on `127.0.0.1:18084`.
- rsproxy started with daemon lifecycle:

```text
rsproxy start --host 127.0.0.1 --port 18899 --api 127.0.0.1:18900 --storage /tmp/rsproxy-dogfood7
```

Observed:

- `start` spawned a background rsproxy, wrote a pidfile, and reported the log path.
- `status` returned the running control-plane status.
- `rules check` accepted the regex matcher.
- `rules test` initially showed raw `${uid}` / `${order}` template placeholders.
- `curl -x` showed the origin receiving `X-Uid` and `X-Order`, and the client receiving `X-Regex-Order`.
- `restart` stopped and relaunched the daemon, and persisted rules continued to work after restart.
- `stop` stopped the daemon and removed the pidfile.

Optimizations from observation:

- `rules test` now renders common action values with captures, so regex explain output shows `req.header(x-uid: 99)` instead of `req.header(x-uid: ${uid})`.
- Internal `kill -0` process probes now silence stderr, so `stop`/`restart` no longer print transient `No such process` messages after a process exits.

## Loop 8

Rule:

```text
127.0.0.1:18085/replay req.header(x-replayed: captured) req.body.append("+captured")
```

Runtime setup:

- POST echo origin on `127.0.0.1:18085`.
- rsproxy daemon on `127.0.0.1:18899` with control API `127.0.0.1:18900`.

Observed:

- The first `start` attempt reported success but the daemon process disappeared immediately after the parent command exited. The log only showed the normal `rsproxy running...` line.
- After fixing daemon detach, `start` kept the daemon alive and `status` worked.
- `curl -x` POST created trace session `1` with rewritten header `X-Replayed: captured` and body `BODY+captured`.
- `rsproxy replay 1` replayed the trace request directly to the origin and returned a response summary containing `x-replayed=captured` and `body=BODY+captured`.
- `trace ls` still showed one proxied session after replay, confirming replay is a control-plane action and does not create an extra proxy trace entry.

Optimizations from observation:

- Daemon start now calls `setsid()` on Unix/macOS before spawning the background `run` process, so the daemon survives after the `start` command exits.
- Added `rsproxy replay <id>` and `/api/replay/{id}` for small HTTP replay from captured request headers and `req_body_head`.

## Loop 9

Runtime setup:

- Finite SSE origin on `127.0.0.1:18086/events`, returning three `text/event-stream` frames.
- rsproxy daemon on `127.0.0.1:18899` with control API `127.0.0.1:18900`.

Observed:

- `curl -x http://127.0.0.1:18899 http://127.0.0.1:18086/events` returned `Content-Type: text/event-stream` and the expected event payload.
- `trace ls` showed kind `sse`.
- `trace get 1` included `frames` with three `s2c` entries:
  `event: greet`, `id: 2`, and `: comment`.
- `trace export --json` preserved the SSE kind and frame list.

Optimization from observation:

- Added `SessionKind::Sse`, frame records, SSE content-type detection, and `\n\n` frame splitting for finite SSE responses. This loop was buffered; Loop 37 replaces that limitation with streaming SSE forwarding and incremental frame capture.

## Loop 10

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18087`.
- rsproxy daemon on `127.0.0.1:18899` with control API `127.0.0.1:18900`.

Observed:

- `rsproxy trace follow --count 2 --poll-ms 100` waited for new sessions and exited after two events.
- Two curl requests through the proxy produced `/a` and `/b` responses.
- Follow output contained two NDJSON session summaries with ids `1` and `2`.
- `trace ls` showed the same two sessions in reverse chronological order.

Optimization from observation:

- Added `TraceStore::list_after`, `/api/sessions.ndjson?after=<id>&limit=<n>`, and a polling `trace follow` CLI with `--count` for scriptable dogfooding.

## Loop 11

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18088` echoing received `x-pay` and `x-dup` request headers.
- rsproxy daemon on `127.0.0.1:18899` with control API `127.0.0.1:18900`.

Observed:

- `rules check` accepted a lookahead matcher and a numbered backreference matcher.
- `rules test 'http://127.0.0.1:18088/pay/42?ok=1'` rendered `req.header(x-pay: 42)` and `res.header(x-engine: fancy-lookahead)`.
- `rules test 'http://127.0.0.1:18088/dup/abc/abc'` rendered `req.header(x-dup: abc)` and `res.header(x-engine: fancy-backref)`.
- `curl -x http://127.0.0.1:18899 'http://127.0.0.1:18088/pay/42?ok=1'` returned `x-pay=42` from the origin body plus `X-Engine: fancy-lookahead`.
- `curl -x http://127.0.0.1:18899 'http://127.0.0.1:18088/dup/abc/abc'` returned `x-dup=abc` from the origin body plus `X-Engine: fancy-backref`.
- `trace get` captured the matched rule lines and rewritten request/response headers for both sessions.

Optimization from observation:

- Added `fancy-regex` fallback for patterns rejected by Rust `regex`, covering lookaround and backreferences.
- Cached compiled regex programs inside `RegexMatcher`, so rules are compiled at parse/load time instead of per request match.
- Kept hard execution budget/timeout for fancy-regex as a remaining safety gap rather than documenting it as complete.

## Loop 12

Runtime setup:

- CA storage in `/tmp/rsproxy-dogfood12`.
- rsproxy daemon on `127.0.0.1:18898` with control API `127.0.0.1:18901`.

Observed:

- `rsproxy ca status --storage /tmp/rsproxy-dogfood12` initially reported `initialized=false`.
- `rsproxy ca init --storage /tmp/rsproxy-dogfood12 --name 'rsproxy dogfood root CA'` generated `rsproxy-root-ca.pem` and `rsproxy-root-ca-key.pem`.
- `rsproxy ca status` reported `initialized=true` and a SHA-256 certificate fingerprint.
- `openssl x509 -noout -subject -issuer -fingerprint -sha256` showed matching subject/issuer and the same SHA-256 fingerprint.
- `openssl x509 -text` showed ECDSA P-256 and `Basic Constraints: CA:TRUE`.
- Re-running `rsproxy ca init` was idempotent when cert and key already existed.
- `rsproxy ca export -o /tmp/rsproxy-dogfood12-export.pem` wrote a byte-for-byte copy of the root certificate.
- `curl http://127.0.0.1:18901/api/ca/root.pem` and `curl http://127.0.0.1:18901/rsproxy.crt` downloaded the same certificate from the daemon control API.

Optimization from observation:

- Replaced the CA placeholder with real `rcgen` root CA generation.
- Added private-key permission hardening on Unix (`0600`).
- Added DER SHA-256 fingerprint output in `ca status`.
- Added `ca export` and daemon certificate download endpoints for browser/mobile trust bootstrap.

## Loop 13

Runtime setup:

- CA storage in `/tmp/rsproxy-dogfood13`.

Observed:

- `rsproxy ca init --storage /tmp/rsproxy-dogfood13 --name 'rsproxy dogfood13 root CA'` generated a new root certificate and key.
- `rsproxy ca issue api.example.test --storage /tmp/rsproxy-dogfood13` generated a cached leaf certificate, key, and chain file under `ca/leaf/`.
- `openssl x509 -noout -subject -issuer -fingerprint -sha256` showed `CN=api.example.test`, issuer `CN=rsproxy dogfood13 root CA, O=rsproxy`, and the same SHA-256 leaf fingerprint printed by rsproxy.
- `openssl x509 -text` showed ECDSA P-256, `Subject Alternative Name: DNS:api.example.test`, and `TLS Web Server Authentication`.
- `openssl verify -CAfile rsproxy-root-ca.pem api.example.test.pem` passed.
- Re-running `rsproxy ca issue api.example.test` returned `cached` with the same fingerprint.
- `rsproxy ca issue 127.0.0.1` generated an IP SAN certificate, and OpenSSL verification passed.

Optimization from observation:

- Added `ca issue <host> [--force]` for dynamic leaf certificate generation.
- Cached leaf cert/key/chain files by sanitized host under `storage/ca/leaf/`.
- Added chain files containing leaf + root certs for future rustls MITM serving.
- Added `leaf_cached=<n>` to `ca status` for cache observability.

## Loop 14

Runtime setup:

- CA storage in `/tmp/rsproxy-dogfood14`.
- HTTPS origin on `127.0.0.1:18443` using a leaf certificate signed by the rsproxy dogfood root CA.
- rsproxy daemon on `127.0.0.1:18897` with control API `127.0.0.1:18902`.

Observed:

- Direct origin check with `curl --cacert rsproxy-root-ca.pem https://127.0.0.1:18443/direct` succeeded.
- Rule `https://127.0.0.1:18443/** req.header(x-mitm-req: yes) res.header(x-mitm-res: yes)` passed `rules check` and `rules test`.
- First MITM curl completed the downstream TLS handshake but failed with a 502 because the Python TLS origin closed without sending `close_notify`; rustls surfaced this during `read_to_end`.
- After optimizing response body reads, `curl --cacert rsproxy-root-ca.pem -x http://127.0.0.1:18897 'https://127.0.0.1:18443/secure?via=mitm'` returned 200.
- The origin response body showed `x-mitm-req=yes`, proving the decrypted request was rewritten before upstream forwarding.
- The client response included `X-Mitm-Res: yes`, proving decrypted response actions ran before re-encryption.
- `trace get 1` recorded a normal HTTP session for the `https://` URL with `flags:["mitm"]`, matched rule metadata, rewritten request headers, rewritten response headers, and response body head.

Optimization from observation:

- Added a minimal CONNECT HTTPS MITM path using rustls server/client streams and cached rcgen leaf certificates.
- Reused the existing HTTP rule, rewrite, forward, and trace pipeline for decrypted HTTPS requests.
- Replaced TLS upstream `read_to_end` with `Content-Length` aware response body reads, and tolerated missing TLS `close_notify` for no-length fallback reads.
- Kept CONNECT passthrough when CA is not initialized or a matched rule includes `bypass`.

## Loop 15

Runtime setup:

- CA storage in `/tmp/rsproxy-dogfood15`.
- rsproxy daemon on `127.0.0.1:18896` with control API `127.0.0.1:18903`.
- Public upstream target `https://example.com/`.
- Local HTTPS origin on `127.0.0.1:18444` signed by the rsproxy dogfood root CA.

Observed:

- Direct `curl -I -L https://example.com` succeeded, confirming public network access.
- First rsproxy MITM request to `https://example.com/` succeeded upstream TLS validation after adding WebPKI roots, and response action `X-Rsproxy-Public-Mitm: yes` was applied.
- The first public response body still contained raw chunk framing (`22f` and `0`) because upstream used `Transfer-Encoding: chunked`; rsproxy removed transfer encoding and set content length without decoding chunks.
- After adding chunked response decoding, the same curl returned clean HTML with `Content-Length: 559` and no chunk markers.
- `trace get 1` recorded `https://example.com:443/`, upstream `example.com:443`, `flags:["mitm"]`, the matched response rule, clean response body head, and the injected response header.
- A second local CA HTTPS origin request through the same proxy returned `local-ca-ok`, proving the root store still includes the rsproxy root for local dogfood CA servers.

Optimization from observation:

- Added Mozilla WebPKI roots to the MITM upstream rustls client root store, while still appending the rsproxy root CA.
- Added HTTP/1.1 chunked response body decoding before response rewrites and trace capture.
- Preserved normalized `Content-Length` output after chunk decoding.

## Loop 16

Runtime setup:

- Plain WebSocket echo server on `127.0.0.1:18091`.
- rsproxy daemon on `127.0.0.1:18895` with control API `127.0.0.1:18904`.
- Rule `127.0.0.1:18091 res.header(x-ws-rule: yes)`.
- Small Python WebSocket client connecting through rsproxy as an HTTP proxy.

Observed:

- The client received `HTTP/1.1 101 Switching Protocols`.
- The client sent masked text frame `hello-rsproxy` through rsproxy and received `echo:hello-rsproxy`.
- The echo server printed `server_received=hello-rsproxy`, proving frame forwarding reached upstream.
- `trace ls` showed kind `websocket`, status `101`, and URL `http://127.0.0.1:18091/ws`.
- `trace get 1` recorded `flags:["websocket"]`, response header `X-Ws-Rule: yes`, `request_bytes=25`, `response_bytes=20`, and frames:
  `c2s hello-rsproxy`, `s2c echo:hello-rsproxy`, and the client close frame.

Optimization from observation:

- Added `SessionKind::WebSocket` and JSON/table rendering for websocket sessions.
- Preserved `Connection: Upgrade` for WebSocket requests instead of rewriting them to `Connection: close`.
- Added minimal HTTP/1.1 `101` WebSocket upgrade handling with sequential c2s/s2c frame forwarding.
- Added WebSocket frame parsing for masked client frames and unmasked server frames, storing decoded payloads in trace.
- Updated WebSocket request byte accounting so trace stats include c2s frame bytes.

## Loop 17

Runtime setup:

- rsproxy daemon on `127.0.0.1:18894` with control API `127.0.0.1:18905`.
- Proxy CA storage in `/tmp/rsproxy-dogfood17`.
- Fault injection HTTPS origin on `127.0.0.1:18446`, signed by a different untrusted CA in `/tmp/rsproxy-dogfood17-origin-ca`.

Observed:

- `curl -x http://127.0.0.1:18894 'http://127.0.0.1:18199/no-listener'` returned 502 with body `upstream error: stage=connect: Connection refused`.
- `trace get 1` recorded error `stage=connect: Connection refused`, but initially had `upstream:null`.
- `curl --cacert rsproxy-root-ca.pem -x http://127.0.0.1:18894 'https://127.0.0.1:18446/tls-fail'` completed the downstream MITM handshake and returned 502 with body `upstream error: stage=tls: invalid peer certificate: UnknownIssuer`.
- `trace get 2` recorded `flags:["mitm"]` and error `stage=tls: invalid peer certificate: UnknownIssuer`.
- After optimizing failed-session attribution, a repeated refused-port request recorded `upstream:"127.0.0.1:18199"` alongside `stage=connect`.

Optimization from observation:

- Added staged upstream error context for `connect`, `tls_config`, `tls`, `request_write`, `response_head`, and `response_body`.
- Drove upstream rustls client handshakes before writing HTTP requests so certificate failures are attributed to `stage=tls` instead of a generic request write failure.
- Recorded planned upstream address on failed sessions, so trace attribution includes the target even without a successful response.

## Loop 18

Runtime setup:

- CA storage in `/tmp/rsproxy-dogfood18`.
- Temporary macOS keychain at `/tmp/rsproxy-dogfood18.keychain-db`; the default login keychain was not modified.

Observed:

- `rsproxy ca status --storage /tmp/rsproxy-dogfood18 --keychain /tmp/rsproxy-dogfood18.keychain-db` reported `installed=false` before install.
- `rsproxy ca install --storage /tmp/rsproxy-dogfood18 --keychain /tmp/rsproxy-dogfood18.keychain-db` initially hung inside `security add-trusted-cert`.
- `man security` confirms that modifying per-user Trust Settings requires an authentication dialog, which is not usable in the headless dogfood session.
- The interrupted `add-trusted-cert` had already written the certificate item into the temporary keychain; `rsproxy ca status` reported `installed=true`, and `security find-certificate -a -Z` showed the same SHA-256 fingerprint.
- `rsproxy ca uninstall --storage /tmp/rsproxy-dogfood18 --keychain /tmp/rsproxy-dogfood18.keychain-db` removed the keychain certificate item and reported `installed=false`.
- A final native `security find-certificate -a -Z` check found no matching SHA-256 fingerprint.

Optimization from observation:

- Implemented `ca install` on macOS via `security add-trusted-cert -r trustRoot -p ssl -k <keychain>`.
- Implemented `ca uninstall` on macOS using SHA-256 keyed `security delete-certificate -t`, plus trust-settings removal only when `security dump-trust-settings` shows the certificate fingerprint.
- Added `ca status --keychain <file>` to check whether the rsproxy root certificate is present in a keychain.
- Added timeout handling around `security` child processes so macOS authentication-dialog waits return a clear CLI error instead of hanging indefinitely.
- Kept dogfood isolated to a temporary keychain; no login or system keychain trust state was intentionally changed.

## Loop 19

Runtime setup:

- macOS `networksetup` available.
- Existing Wi-Fi system proxy was already enabled for `127.0.0.1:7897`, so this loop avoided mutating live network settings.

Observed:

- `rsproxy proxy status --service 'Wi-Fi'` reported both HTTP and HTTPS proxy state, including server, port, authenticated flag, and bypass domains.
- Default `rsproxy proxy status` enumerated all network services from `networksetup -listallnetworkservices` and displayed per-service HTTP/HTTPS proxy status.
- `rsproxy proxy on --service 'Wi-Fi' --host 127.0.0.1 --port 8899 --dry-run` printed the exact `networksetup -setwebproxy` and `-setsecurewebproxy` commands it would execute.
- `rsproxy proxy off --service 'Wi-Fi' --dry-run` printed the exact `networksetup -setwebproxystate off` and `-setsecurewebproxystate off` commands.
- No real on/off mutation was executed because the active system proxy belonged to another local service.

Optimization from observation:

- Replaced the system proxy placeholder with macOS `networksetup` integration.
- Made `proxy status` read-only by default and able to inspect all services or a single `--service`.
- Required `--service NAME` or `--all` for `proxy on/off` to avoid accidental broad changes.
- Added `--dry-run` for no-side-effect command verification.
- Avoided overwriting bypass domains unless `--bypass a,b,c` is explicitly provided.

## Loop 20

Runtime setup:

- Plain WebSocket origin on `127.0.0.1:18092`.
- rsproxy daemon on `127.0.0.1:18893` with control API `127.0.0.1:18906`.
- Rule `127.0.0.1:18092 res.header(x-ws-concurrent: yes)`.

Observed:

- The origin sent a WebSocket text frame `push-first` immediately after the `101 Switching Protocols` response, before the client sent any WebSocket message.
- The client, connected through rsproxy as an HTTP proxy, received `push-first` before sending `client-later`.
- The client then sent masked text frame `client-later` and received `echo:client-later`.
- The first concurrent implementation forwarded bytes correctly but initially recorded trace as HTTP 502 because connection reset during WebSocket close was treated as a hard tunnel error.
- After normalizing close-phase reset/broken-pipe errors, `trace ls` recorded kind `websocket`, status `101`, and 33 response bytes.
- `trace get 1` recorded response header `X-Ws-Concurrent: yes` and frames in observed order: `s2c push-first`, `c2s client-later`, `s2c echo:client-later`, then server close.

Optimization from observation:

- Added an `UpstreamStream` enum so plain TCP upstreams can be cloned for bidirectional WebSocket forwarding while TLS upstreams keep the existing fallback path.
- Added concurrent c2s and s2c WebSocket frame forwarding for plain TCP WebSocket sessions.
- Treated close-phase `UnexpectedEof`, `ConnectionReset`, `ConnectionAborted`, and `BrokenPipe` as normal WebSocket tunnel termination.
- Changed concurrent frame tracing to append into a shared queue at capture time, preserving observed ordering better than post-merge timestamp sorting.

## Loop 21

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18093`.
- rsproxy daemon on `127.0.0.1:18892` with control API `127.0.0.1:18907`.
- Rules:

```text
/(a|b|ab)*(?=c)/i res.header(x-redos: matched)
/\/ok\/(\d+)(?=\?go=1)/ res.header(x-fancy-ok: $1)
```

Observed:

- `rules test 'http://127.0.0.1:18093/ok/42?go=1'` rendered `res.header(x-fancy-ok: 42)`, proving normal lookahead capture still works through the fancy engine.
- `rules test` against a 60-character `abab...` path with no `c` returned `no matched actions` quickly.
- `curl -x http://127.0.0.1:18892 'http://127.0.0.1:18093/ok/42?go=1'` returned `X-Fancy-Ok: 42`.
- `curl -x` against the adversarial `abab...` path returned the origin response without `X-Redos`.
- `trace get 1` recorded matched rule line 2 and response header `X-Fancy-Ok: 42`.
- `trace get 2` recorded no matched rules and no injected redos header.

Optimization from observation:

- Switched fancy fallback compilation to `fancy_regex::RegexBuilder` with a default backtrack limit of `100_000`.
- Treated `RuntimeError::BacktrackLimitExceeded` as a clean non-match.
- Added a focused unit test for the adversarial fancy-regex budget path.

## Loop 22

Runtime setup:

- POST echo origin on `127.0.0.1:18094`.
- rsproxy daemon on `127.0.0.1:18891` with control API `127.0.0.1:18908`.
- Rule:

```text
127.0.0.1:18094/replace req.body.replace(/item-(\d+)/, item=$1) res.body.replace(/status=raw/, status=rewritten) res.header(x-body-replace: yes)
```

Observed:

- Initial `rules test` rendered `req.body.replace(/item-(\d+)/, item=)`, because replacement `$1` was incorrectly treated as a rule matcher capture instead of a body-regex replacement capture.
- After optimizing replacement rendering, `rules test` preserved `item=$1`.
- `curl -x http://127.0.0.1:18891 -X POST --data 'item-42; status=client'` returned `X-Body-Replace: yes`.
- The response body was `origin-body=item=42; status=client; status=rewritten`, proving request body regex replacement happened before upstream forwarding and response body regex replacement happened before client write.
- `trace get 1` recorded rewritten request body head `item=42; status=client` and response body head `origin-body=item=42; status=client; status=rewritten`.

Optimization from observation:

- Added `req.body.replace(pattern, repl)` and `res.body.replace(pattern, repl)` to the rule AST and parser.
- Implemented UTF-8 text body replacement using the linear Rust `regex` engine.
- Preserved `$1`/named replacement syntax for the body regex replacement engine instead of applying rule-capture template rendering to replacement strings.
- Added parser coverage for request/response body replace actions.

## Loop 23

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18095`.
- rsproxy daemon on `127.0.0.1:18890` with control API `127.0.0.1:18909`.
- Regex rewrite rule:

```text
127.0.0.1:18095 url.rewrite(/\/api\/v(\d+)/, /v$1) res.header(x-url-regex: yes)
```

Observed:

- `rules test 'http://127.0.0.1:18095/api/v2/items?x=1'` rendered `url.rewrite(/\/api\/v(\d+)/, /v$1)` and preserved the replacement `$1`.
- `curl -x http://127.0.0.1:18890 'http://127.0.0.1:18095/api/v2/items?x=1'` returned `X-Url-Regex: yes`.
- The origin response body was `origin-path=/v2/items?x=1`, proving URL regex replacement happened before upstream forwarding.
- `trace ls` recorded URL `http://127.0.0.1:18095/v2/items?x=1` and `trace get 1` included flag `url-rewrite` plus the matched rule.
- A compatibility check with old `url.rewrite(/old,/new)` still rewrote `/old/path` to `/new/path`, proving path-like plain rewrite arguments are not misclassified as regex literals.

Optimization from observation:

- Changed `UrlRewrite` from plain string-only `from` to `UrlRewritePattern::Plain|Regex`.
- Treat only complete `/.../flags` literals as regex rewrite patterns, so existing `url.rewrite(/old,/new)` behavior remains unchanged.
- Implemented regex URL replacement against origin-form path/query using the linear Rust `regex` engine.
- Preserved `$1`/named replacement syntax for URL regex replacement instead of applying rule-capture template rendering to replacement strings.

## Loop 24

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18096` serving JSON at `/json` and text at `/text`.
- rsproxy daemon on `127.0.0.1:18891` with control API `127.0.0.1:18910`.
- Rules:

```text
127.0.0.1:18096/json res.merge({"added":2,"nested":{"replace":"after","new":3},"from":"${path}"}) res.header(x-merge: yes)
127.0.0.1:18096/text res.merge({"unused":true}) res.header(x-text-rule: yes)
```

Observed:

- `rules test 'http://127.0.0.1:18096/json?case=merge'` rendered `res.merge({"added":2,"nested":{"replace":"after","new":3},"from":"/json"})`, proving JSON arguments with commas and template variables parse correctly.
- `curl -x http://127.0.0.1:18891 'http://127.0.0.1:18096/json?case=merge'` returned `X-Merge: yes` and body `{"added":2,"from":"/json","keep":1,"nested":{"new":3,"old":true,"replace":"after"},"path":"/json?case=merge"}`.
- `curl -x http://127.0.0.1:18891 'http://127.0.0.1:18096/text'` returned `X-Text-Rule: yes` while preserving body `plain-origin`, proving non-JSON responses are left unchanged.
- `trace get 1` included flag `res-merge`, the matched JSON merge rule, updated `Content-Length: 109`, and the merged response body head.
- `trace get 3` included flag `res-merge`, the matched text rule, and unchanged response body head `plain-origin`.

Optimization from observation:

- Added `res.merge(json)` to the rule AST/parser and made it a stackable response action.
- Extended action argument splitting to ignore commas inside `{}` and `[]`, so inline JSON object/array literals work without extra quoting.
- Implemented recursive JSON object merge in the proxy response phase using `serde_json`; object keys merge deeply, scalar/array patch values replace existing values.
- Left non-UTF-8, invalid JSON, and non-object response bodies unchanged to avoid breaking broad response rules.

## Loop 25

Runtime setup:

- Raw HTTP/1.1 origin on `127.0.0.1:18097` returning a chunked response with upstream trailers `x-origin-trailer: origin` and `x-remove-me: gone`.
- rsproxy daemon on `127.0.0.1:18892` with control API `127.0.0.1:18911`.
- Rule:

```text
127.0.0.1:18097 res.trailer(x-origin-trailer: overridden) res.trailer(-x-remove-me) res.trailer(x-added-trailer: ${path}) res.header(x-trailer-rule: yes)
```

Observed:

- `rules test 'http://127.0.0.1:18097/trail?x=1'` rendered the three trailer actions, including `x-added-trailer: /trail`.
- `curl --raw -D - -x http://127.0.0.1:18892 'http://127.0.0.1:18097/trail?x=1'` returned `Transfer-Encoding: chunked`, `Trailer: x-origin-trailer, X-Added-Trailer`, and response header `X-Trailer-Rule: yes`.
- The raw response ended with chunk terminator trailers `x-origin-trailer: overridden` and `X-Added-Trailer: /trail`; upstream trailer `x-remove-me` was absent.
- `trace get 1` included flags `res-trailer` and `trailers`, response headers with `Transfer-Encoding: chunked`, and `res_trailers` containing the final two trailers.

Optimization from observation:

- Added `res.trailer(k: v | -k)` to the rule AST/parser and made it stackable.
- Changed chunked response decoding to preserve upstream trailers instead of discarding them.
- Added a chunked response writer for responses with trailers, removing `Content-Length`, setting `Transfer-Encoding: chunked`, and emitting final trailers after the body.
- Added `res_trailers` to trace detail JSON so trailer changes are observable independently from response headers.

## Loop 26

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18098` supporting `GET /api` and `OPTIONS /api`.
- rsproxy daemon on `127.0.0.1:18893` with control API `127.0.0.1:18912`.
- Rule:

```text
127.0.0.1:18098 res.cors(${reqH.origin}, methods=GET POST OPTIONS, headers=X-Token Content-Type, credentials=true, expose=X-Upstream X-Trace, max-age=600) res.header(x-cors-rule: detailed)
```

Observed:

- `rules test 'http://127.0.0.1:18098/api'` rendered all detailed CORS parameters; `${reqH.origin}` was empty in offline test because `rules test` does not yet accept custom request headers.
- `curl -x http://127.0.0.1:18893 -H 'Origin: https://app.example' 'http://127.0.0.1:18098/api'` returned `Access-Control-Allow-Origin: https://app.example`, detailed allow methods/headers, credentials, expose headers, max-age, `Vary: Origin`, and `X-Cors-Rule: detailed`.
- `curl -x http://127.0.0.1:18893 -X OPTIONS ...` returned the same detailed CORS headers on the preflight `204` response.
- `trace get 1` and `trace get 2` included flag `res-cors`, the request `Origin`, and the full detailed CORS response header set for GET and OPTIONS.

Optimization from observation:

- Changed `res.cors` from a plain origin string to a structured `CorsOp` while preserving `res.cors(*)`.
- Added detailed options: `methods=`, `headers=`, `credentials=true|false`, `expose=`, and `max-age=`.
- Rendered CORS option values with the same template system, enabling `${reqH.origin}` request-origin reflection.
- Added automatic `Vary: Origin` when the resolved allowed origin is not `*`.

## Loop 27

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18099` returning `Set-Cookie: old=1; Path=/` and `Set-Cookie: keep=origin; Path=/`.
- rsproxy daemon on `127.0.0.1:18894` with control API `127.0.0.1:18913`.
- Rule:

```text
127.0.0.1:18099 res.cookie(-old) res.cookie(token=${path}; Path=/api; Max-Age=60; HttpOnly; Secure; SameSite=Lax) res.header(x-cookie-rule: advanced)
```

Observed:

- `rules test 'http://127.0.0.1:18099/cookie'` rendered `res.cookie(token=/cookie; Path=/api; Max-Age=60; HttpOnly; Secure; SameSite=Lax)`.
- `curl -x http://127.0.0.1:18894 'http://127.0.0.1:18099/cookie'` returned origin cookie `keep=origin; Path=/`, removed origin cookie `old=1`, and added `Set-Cookie: token=/cookie; Path=/api; Max-Age=60; HttpOnly; Secure; SameSite=Lax`.
- `trace get 1` included flag `res-cookie`, the matched rule, and the final response header list with the old cookie removed and advanced cookie present.

Optimization from observation:

- Extended `CookieOp::Set` with parsed Set-Cookie attributes while preserving legacy `res.cookie(token=1)` behavior.
- Added canonical parsing for common attributes: `Path`, `Domain`, `Expires`, `Max-Age`, `HttpOnly`, `Secure`, `SameSite`, `Partitioned`, and `Priority`.
- Kept request cookie rewriting compatible by ignoring Set-Cookie-only attributes in `req.cookie`.
- Added `req-cookie` and `res-cookie` trace flags for cookie rewrite observability.

## Loop 28

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18100` returning conflicting cache headers `Cache-Control: no-store` and `Pragma: no-cache`.
- rsproxy daemon on `127.0.0.1:18895` with control API `127.0.0.1:18914`.
- Rule:

```text
127.0.0.1:18100 cache(public, max-age=60, s-maxage=120, stale-while-revalidate=30, immutable) res.header(x-cache-rule: advanced)
```

Observed:

- `rules test 'http://127.0.0.1:18100/cache'` rendered `cache(public, max-age=60, s-maxage=120, stale-while-revalidate=30, immutable)`.
- `curl -x http://127.0.0.1:18895 'http://127.0.0.1:18100/cache'` returned `Cache-Control: public, max-age=60, s-maxage=120, stale-while-revalidate=30, immutable`.
- The origin `Pragma: no-cache` header was removed when the detailed cache rule rewrote `Cache-Control`.
- `trace get 1` included flag `cache`, the matched rule, and the final advanced cache response header.

Optimization from observation:

- Changed `CacheOp` from simple max-age string to structured directives while preserving `cache(off)` and `cache(60)`.
- Added composable directives including `public`, `private`, `no-cache`, `no-store`, `max-age`, `s-maxage`, `stale-while-revalidate`, `stale-if-error`, `immutable`, and related pass-through directives.
- Removed stale `Pragma` when detailed cache directives are used so origin no-cache headers do not contradict the rewritten policy.
- Added `cache` trace flag for response cache policy observability.

## Loop 29

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18101`.
- rsproxy daemon on `127.0.0.1:18896` with Unix control API `unix:/tmp/rsproxy-dogfood29/run/ctl.sock`.
- Rule loaded through the Unix socket control API:

```text
127.0.0.1:18101 res.header(x-unix-api: yes) cache(30)
```

Observed:

- `rsproxy start --api unix:/tmp/rsproxy-dogfood29/run/ctl.sock` reported the Unix endpoint and became ready.
- `ls -l /tmp/rsproxy-dogfood29/run/ctl.sock` showed `srw-------`, confirming the socket is user-only.
- `rsproxy status --api unix:/tmp/rsproxy-dogfood29/run/ctl.sock` returned normal daemon status.
- `curl --unix-socket /tmp/rsproxy-dogfood29/run/ctl.sock http://rsproxy/api/status` returned the same status JSON, proving non-CLI clients can use the Unix socket HTTP API.
- `rules set`, `rules test`, `trace ls`, and `trace get` all worked through `--api unix:/tmp/rsproxy-dogfood29/run/ctl.sock`.
- `curl -x http://127.0.0.1:18896 'http://127.0.0.1:18101/unix'` returned `X-Unix-Api: yes` and `Cache-Control: max-age=30`, proving rules loaded over Unix control API affected proxy data-plane behavior.
- `curl --unix-socket ... /api/sessions/1` returned the same trace detail as the CLI.

Optimization from observation:

- Added `unix:/path` and `unix:///path` control API endpoint parsing.
- Added UnixListener-based control API serving with parent directory creation, stale socket cleanup, and `0600` socket permissions.
- Changed the CLI API client to use UnixStream automatically for Unix endpoints while preserving TCP `HOST:PORT`.
- Updated start/run output and help text so Unix endpoints are displayed without an incorrect `http://` prefix.

## Loop 30

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18102`.
- rsproxy foreground daemon on `127.0.0.1:18897` with control API `127.0.0.1:18915` and storage `/tmp/rsproxy-dogfood30`.
- Rule:

```text
127.0.0.1:18102 res.header(x-spill: yes) cache(15)
```

Observed:

- `/api/status` returned trace stats with `spill_path=/tmp/rsproxy-dogfood30/trace/sessions.ndjson`, `spilled=0`, and `spill_errors=0`.
- A request made while the origin was down returned `502` and still wrote a spill row with `error="stage=connect: Connection refused"` and the matched rule.
- After restarting the origin, `curl -x http://127.0.0.1:18897 'http://127.0.0.1:18102/spill?via=proxy&ok=1'` returned `X-Spill: yes` and `Cache-Control: max-age=15`.
- `rsproxy trace stats --api 127.0.0.1:18915` returned `sessions=2`, `next_id=3`, `spilled=2`, and `spill_errors=0`.
- `curl http://127.0.0.1:18915/api/sessions/spill.ndjson` returned two NDJSON rows, including the successful `200` response headers and body preview.
- `rsproxy trace clear` reset `spilled=0` and `/api/sessions/spill.ndjson` returned an empty body.

Optimization from observation:

- Added `TraceStore::new_with_spill`, storing append-only NDJSON under `<storage>/trace/sessions.ndjson`.
- Extended trace stats with `spilled`, `spill_path`, `spill_errors`, and `last_spill_error`.
- Added `/api/sessions/spill.ndjson` for direct curl/script access to the on-disk trace stream.
- Made `trace clear` remove the spill file as well as clearing the in-memory ring.

## Loop 31

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18103` returning a JSON body large enough to make trace rows exceed 1KB.
- rsproxy foreground daemon on `127.0.0.1:18898` with control API `127.0.0.1:18916`, storage `/tmp/rsproxy-dogfood31`, `--trace-segment-size 1kb`, `--trace-disk-budget 3kb`, and `--trace-body-limit 512b`.
- Rule:

```text
127.0.0.1:18103 res.header(x-rotate: yes) cache(7)
```

Observed:

- Eight proxied requests returned `200`; `trace ls` showed all eight in the in-memory ring.
- `rsproxy trace stats --api 127.0.0.1:18916` returned `spilled=8`, `spill_segments=2`, `spill_bytes=2604`, `spill_disk_budget_bytes=3072`, and `spill_evicted_segments=6`.
- The trace directory contained only `seg-000000000007.ndjson` and `seg-000000000008.ndjson`, proving oldest segments were deleted by the disk budget.
- `curl http://127.0.0.1:18916/api/sessions/spill.ndjson | wc -l` returned `2`, while `spilled=8`, proving the API exposes the retained on-disk window rather than the full in-memory ring.
- The retained NDJSON rows included `X-Rotate: yes`, `Cache-Control: max-age=7`, matched rule metadata, and truncated response body preview.
- A probe request returned `X-Rotate: yes` and `Cache-Control: max-age=7`; stats then showed `spilled=9`, `spill_segments=2`, `spill_bytes=2614`, and `spill_evicted_segments=7`.
- `rsproxy trace clear` reset `spill_bytes=0`, `spill_segments=0`, `spill_evicted_segments=0`, and the spill endpoint returned an empty body.

Optimization from observation:

- Replaced single-file spill with ordered NDJSON segment files under `<storage>/trace/seg-{n}.ndjson`.
- Added `--trace-segment-size`, `--trace-disk-budget`, and `--trace-body-limit` parsing for resource-control dogfooding.
- Added segment stats: `spill_dir`, `spill_bytes`, `spill_segments`, `spill_segment_bytes`, `spill_disk_budget_bytes`, and `spill_evicted_segments`.
- Changed `/api/sessions/spill.ndjson` to concatenate the currently retained segment files in order.

## Loop 32

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18104`.
- Minimal local SOCKS5 no-auth TCP forwarder on `127.0.0.1:18105`, logging each CONNECT target to `/tmp/rsproxy-dogfood32/socks.log`.
- rsproxy foreground daemon on `127.0.0.1:18899` with control API `127.0.0.1:18917`.
- Rule:

```text
127.0.0.1:18104 upstream(socks5://127.0.0.1:18105) res.header(x-socks-route: yes) cache(11)
```

Observed:

- `rules test 'http://127.0.0.1:18104/socks?via=rsproxy'` rendered `upstream(socks5://127.0.0.1:18105)`, `res.header(x-socks-route: yes)`, and `cache(max-age=11)`.
- `curl -x http://127.0.0.1:18899 'http://127.0.0.1:18104/socks?via=rsproxy'` returned `200`, `X-Socks-Route: yes`, and `Cache-Control: max-age=11`.
- The SOCKS5 forwarder log contained `CONNECT 127.0.0.1:18104`, proving rsproxy opened the origin connection through the SOCKS proxy.
- `trace get 1` recorded `upstream:"socks5://127.0.0.1:18105->127.0.0.1:18104"`, the matched rule, the injected response header, and the response body preview.
- `/api/sessions/spill.ndjson` contained the same socks upstream label and response headers.

Optimization from observation:

- Added upstream route parsing for `upstream(socks://host:port)` and `upstream(socks5://host:port)`.
- Added SOCKS5 no-auth CONNECT negotiation before request forwarding, with failures attributed as `stage=socks5`.
- Kept HTTP proxy behavior unchanged: `proxy://` and `http://` still use absolute-form request forwarding.
- Added a focused unit test for SOCKS route parsing and trace-friendly upstream labels.

## Loop 33

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18106`.
- Local HTTPS proxy on `127.0.0.1:18107`, using a `127.0.0.1` certificate issued by the rsproxy dogfood root CA in `/tmp/rsproxy-dogfood33`.
- rsproxy foreground daemon on `127.0.0.1:18900` with control API `127.0.0.1:18918`, sharing the same storage so its upstream TLS client trusts the dogfood CA.
- Rule:

```text
127.0.0.1:18106 upstream(https-proxy://127.0.0.1:18107) res.header(x-https-proxy-route: yes) cache(13)
```

Observed:

- `rsproxy ca init` and `rsproxy ca issue 127.0.0.1` generated a root CA and HTTPS-proxy leaf certificate.
- Direct proxy smoke test with `curl --proxy https://127.0.0.1:18107 --proxy-cacert ...` succeeded before routing through rsproxy.
- `rules test 'http://127.0.0.1:18106/secure-proxy?via=rsproxy'` rendered `upstream(https-proxy://127.0.0.1:18107)`, `res.header(x-https-proxy-route: yes)`, and `cache(max-age=13)`.
- `curl -x http://127.0.0.1:18900 'http://127.0.0.1:18106/secure-proxy?via=rsproxy'` returned `200`, `X-Https-Proxy-Route: yes`, and `Cache-Control: max-age=13`.
- The HTTPS proxy log contained `GET http://127.0.0.1:18106/secure-proxy?via=rsproxy`, proving rsproxy sent an absolute-form request over TLS to the upstream proxy.
- `trace get 1` and `/api/sessions/spill.ndjson` recorded `upstream:"https-proxy://127.0.0.1:18107"`, the matched rule, the injected header, and the response body preview.

Optimization from observation:

- Added `UpstreamRoute::HttpsProxy` for `upstream(https-proxy://host:port)`.
- Reused the existing rustls/WebPKI + local rsproxy CA root store for TLS-to-proxy verification.
- Kept HTTP-proxy request semantics consistent: `proxy://`, `http://`, and `https-proxy://` all forward HTTP requests in absolute-form, with only `https-proxy://` wrapping the proxy hop in TLS.
- Added a focused unit test for HTTPS proxy route parsing and labeling.

## Loop 34

Runtime setup:

- HTTPS origin on `127.0.0.1:18448`, using a local certificate generated with `rsproxy ca init` and `rsproxy ca issue` under `/tmp/rsproxy-dogfood34-origin-ca`.
- Local HTTP proxy on `127.0.0.1:18109`, supporting only CONNECT and logging CONNECT targets.
- Local SOCKS5 no-auth forwarder on `127.0.0.1:18110`, also logging CONNECT targets.
- rsproxy foreground daemon on `127.0.0.1:18901` with control API `127.0.0.1:18919` and no CA initialized in its storage, forcing CONNECT passthrough.

Rules:

```text
127.0.0.1:18448 upstream(proxy://127.0.0.1:18109) bypass
127.0.0.1:18448 upstream(socks5://127.0.0.1:18110) bypass
```

Observed:

- `rules test 'tunnel://127.0.0.1:18448'` rendered the upstream action and `Bypass` for both HTTP proxy and SOCKS5 variants.
- `curl -kisS -x http://127.0.0.1:18901 'https://127.0.0.1:18448/connect-chain?via=rsproxy'` returned the rsproxy `200 Connection Established` line followed by the HTTPS origin `200 OK` response.
- The HTTP proxy log contained `CONNECT 127.0.0.1:18448`, proving rsproxy established the tunnel through the upstream HTTP proxy.
- `trace get 1` recorded `kind:"tunnel"`, `upstream:"proxy://127.0.0.1:18109->127.0.0.1:18448"`, flags `["tunnel","no-ca","upstream"]`, and non-zero request/response byte counts.
- After switching to `upstream(socks5://127.0.0.1:18110)`, `curl -ksS -x http://127.0.0.1:18901 'https://127.0.0.1:18448/connect-socks?via=rsproxy'` returned the HTTPS origin JSON body.
- The SOCKS5 forwarder log contained `CONNECT 127.0.0.1:18448`, and `trace get 2` recorded `upstream:"socks5://127.0.0.1:18110->127.0.0.1:18448"`.

Optimization from observation:

- Routed CONNECT passthrough through the same `UpstreamRoute` model used by HTTP requests.
- Added HTTP proxy CONNECT negotiation for tunnel passthrough and attributed failures to `stage=proxy_connect`.
- Reused SOCKS5 no-auth CONNECT negotiation for tunnel passthrough and kept `stage=socks5` attribution.
- Added trace flag `upstream` and target-aware upstream labels for tunneled sessions.
- Added a focused unit test for HTTP proxy tunnel target parsing and `proxy://...->target` labeling.

## Loop 35

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18111`.
- Local SOCKS5 forwarder on `127.0.0.1:18112` requiring username/password `alice:secret` and logging successful authentication plus CONNECT targets.
- rsproxy foreground daemon on `127.0.0.1:18902` with control API `127.0.0.1:18920`.
- Rule:

```text
127.0.0.1:18111 upstream(socks5://alice:secret@127.0.0.1:18112) res.header(x-socks-auth: yes) cache(17)
```

Observed:

- `rules test 'http://127.0.0.1:18111/auth-socks?via=rsproxy'` rendered the configured authenticated SOCKS5 upstream plus response header and cache actions.
- `curl -x http://127.0.0.1:18902 'http://127.0.0.1:18111/auth-socks-redacted2?via=rsproxy'` returned `200`, `X-Socks-Auth: yes`, and `Cache-Control: max-age=17`.
- The SOCKS5 log recorded `AUTHOK alice` and `CONNECT 127.0.0.1:18111`, proving rsproxy performed RFC 1929 username/password authentication before CONNECT.
- Initial observation found that trace `rules[].raw` still exposed `alice:secret` even though the upstream label was redacted.
- After optimizing trace rendering, `trace get 1` recorded `upstream:"socks5://auth@127.0.0.1:18112->127.0.0.1:18111"` and `rules[].raw` as `socks5://auth@127.0.0.1:18112`.
- The spill NDJSON output was checked together with trace JSON and no longer contained `alice:secret`.

Optimization from observation:

- Added SOCKS5 RFC 1929 username/password authentication when the upstream URL uses `socks5://user:pass@host:port`.
- Route labels use `auth@host:port` instead of credentials.
- Added shared rule-text redaction for trace JSON and spill NDJSON so matched rule metadata does not expose SOCKS credentials.
- Added focused unit tests for authenticated SOCKS route parsing and redaction.

## Loop 36

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18121`.
- Local HTTPS CONNECT proxy on `127.0.0.1:18122`, using a leaf certificate issued by `rsproxy ca issue 127.0.0.1` under `/tmp/rsproxy-dogfood36`.
- rsproxy foreground daemon on `127.0.0.1:18903` with control API `127.0.0.1:18921` and the same storage CA initialized.
- Rule:

```text
127.0.0.1:18121 bypass upstream(https-proxy://127.0.0.1:18122)
```

Observed:

- `rules test 'tunnel://127.0.0.1:18121' -X CONNECT` rendered both `Bypass` and `upstream(https-proxy://127.0.0.1:18122)`.
- `curl -x http://127.0.0.1:18903 --proxytunnel 'http://127.0.0.1:18121/https-proxy-tunnel?via=rsproxy'` returned the rsproxy `200 Connection Established` line followed by the origin `200 OK` response, including `X-Origin: dogfood36`.
- The HTTPS proxy log recorded `CONNECT 127.0.0.1:18121`.
- The HTTPS proxy byte counts `c2s=108` and `s2c=214` matched `trace get 1` request/response byte counts.
- `trace get 1` recorded `kind:"tunnel"`, `status:200`, flags `["tunnel","upstream"]`, and `upstream:"https-proxy://127.0.0.1:18122->127.0.0.1:18121"`.

Optimization from observation:

- CONNECT passthrough now supports `https-proxy://` by wrapping the proxy hop in TLS, then sending the HTTP CONNECT negotiation inside that TLS stream.
- Added a nonblocking rustls tunnel pump for TLS upstream tunnels because `StreamOwned<ClientConnection, TcpStream>` cannot be cloned like a plain `TcpStream`.
- Kept the plain TCP tunnel path on the existing bidirectional `io::copy` fast path.
- Added a focused unit test for HTTPS proxy tunnel target parsing and `https-proxy://...->target` labeling.

## Loop 37

Runtime setup:

- Chunked SSE origin on `127.0.0.1:18131`, sending three `text/event-stream` frames about 450 ms apart with `Transfer-Encoding: chunked`.
- rsproxy foreground daemon on `127.0.0.1:18904` with control API `127.0.0.1:18922`.
- Rule:

```text
127.0.0.1:18131/events res.header(x-sse-stream: yes)
```

Observed:

- `rules check`, `rules cat`, and `rules test 'http://127.0.0.1:18131/events?via=rsproxy'` showed the response header rule.
- `curl -N --trace-time --trace-ascii - -x http://127.0.0.1:18904 'http://127.0.0.1:18131/events?via=rsproxy'` received the three data chunks at `06:09:04.163`, `06:09:04.626`, and `06:09:05.084`, matching the origin's delayed send cadence instead of waiting for the full response to finish.
- The client saw `X-Sse-Stream: yes`, proving response header rules still apply on the streaming path.
- `trace get 1` recorded `kind:"sse"`, `response_bytes:94`, `flags:["sse"]`, the injected response header, and three `s2c` frames with frame timestamps matching origin send times.
- `/api/sessions/spill.ndjson` preserved the same SSE session and frame records.

Optimization from observation:

- Added an SSE streaming path before full response buffering. It applies safe response header/status actions, strips upstream `Content-Length`/`Transfer-Encoding`/`Trailer`, and forwards decoded SSE payload incrementally.
- Kept body-mutating response rules (`res.body`, `res.merge`) and `res.trailer` on the existing buffered path, where whole-body/trailer semantics are required.
- Added incremental SSE frame capture with body-head collection, chunked decoding, cross-chunk CRLF normalization, and a focused unit test for chunked SSE streaming.

## Loop 38

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18141`.
- First local HTTP proxy `p1` on `127.0.0.1:18142`, accepting only CONNECT and logging tunnel targets.
- Second local HTTP proxy `p2` on `127.0.0.1:18143`, forwarding absolute-form HTTP requests and accepting CONNECT.
- rsproxy foreground daemon on `127.0.0.1:18905` with control API `127.0.0.1:18923`.
- Rule:

```text
127.0.0.1:18141 upstream(proxy://127.0.0.1:18142, proxy://127.0.0.1:18143) res.header(x-chain: p1-p2)
```

Observed:

- `rules check`, `rules cat`, and `rules test 'http://127.0.0.1:18141/chain?via=rsproxy'` accepted and rendered the two-hop upstream chain.
- `curl -x http://127.0.0.1:18905 'http://127.0.0.1:18141/chain?via=rsproxy'` returned the origin body and injected `X-Chain: p1-p2`.
- `p1` logged `CONNECT 127.0.0.1:18143`, proving rsproxy opened a tunnel to the second proxy before forwarding the request.
- `p2` logged `FORWARD GET http://127.0.0.1:18141/chain?via=rsproxy -> 127.0.0.1:18141/chain?via=rsproxy`, proving the final absolute-form HTTP request was sent through the second proxy.
- `trace get 1` recorded `upstream:"proxy://127.0.0.1:18142->proxy://127.0.0.1:18143"` and the response header injected after the chain.
- `curl --proxytunnel -x http://127.0.0.1:18905 'http://127.0.0.1:18141/connect-chain?via=rsproxy'` also returned the origin response through a CONNECT tunnel.
- For the CONNECT case, `p1` logged `CONNECT 127.0.0.1:18143`, `p2` logged `CONNECT 127.0.0.1:18141`, and `trace get 2` recorded `upstream:"proxy://127.0.0.1:18142->proxy://127.0.0.1:18143->127.0.0.1:18141"`.
- `/api/sessions/spill.ndjson` preserved both the HTTP and tunnel sessions.

Optimization from observation:

- `upstream(...)` now accepts multiple comma-separated proxy arguments.
- Added `UpstreamRoute::HttpProxyChain` and a reusable HTTP proxy hop model.
- HTTP requests through a chain CONNECT through each intermediate HTTP proxy, then send the final absolute-form request to the last proxy.
- CONNECT passthrough through a chain CONNECTs through each intermediate proxy and then CONNECTs the final target on the last proxy.
- Added focused unit tests for multi-hop upstream parsing, route labels, and tunnel labels.

## Loop 39

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18151`.
- rsproxy foreground daemon on `127.0.0.1:18906` with control API `127.0.0.1:18924`.
- Rule:

```text
127.0.0.1:18151 res.header(x-crc: yes)
```

Observed:

- `/api/status` and `trace stats` exposed new spill recovery fields: `spill_index_entries` and `spill_corrupt_records`.
- Two `curl -x http://127.0.0.1:18906` requests to `/crc-a` and `/crc-b` returned `X-Crc: yes`.
- The trace directory contained `seg-000000000001.ndjson` and `seg-000000000001.ndjson.idx`.
- The `.idx` sidecar had two entries with offset, length, session id, and CRC values:
  `0 764 1 7e8d350b` and `765 764 2 207a9ed4`.
- Before corruption, `/api/sessions/spill.ndjson` returned both spilled sessions and `trace stats` showed `spill_index_entries:2`, `spill_corrupt_records:0`.
- After manually changing the second on-disk record from `crc-b` to `crc-x` without updating the index, `/api/sessions/spill.ndjson` returned only the first record.
- `trace stats` then showed `spill_corrupt_records:1` and `last_spill_error:"skipped 1 corrupt spill record(s)"`.
- `trace ls` and `trace get 2` still returned the in-memory session for `/crc-b`, proving the recovery behavior only filters corrupted on-disk spill reads.

Optimization from observation:

- Added a per-segment `.idx` sidecar for spill files. Each index row stores byte offset, payload length, session id, and CRC32.
- `/api/sessions/spill.ndjson` now uses a verified reader: indexed segments emit only records whose bounds and CRC match; legacy segments without an index remain readable as raw NDJSON.
- Trace stats now expose `spill_index_entries` and `spill_corrupt_records`.
- Segment budget eviction and `trace clear` remove both `.ndjson` data files and `.idx` sidecars.
- Added focused unit tests for index creation, restarted index scanning, and CRC mismatch recovery.

## Loop 40

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18161`.
- First local HTTP proxy `p1` on `127.0.0.1:18162`, accepting only CONNECT and logging tunnel targets.
- Second local SOCKS5 no-auth proxy `p2` on `127.0.0.1:18163`.
- rsproxy foreground daemon on `127.0.0.1:18907` with control API `127.0.0.1:18925`.
- Rule:

```text
127.0.0.1:18161 upstream(proxy://127.0.0.1:18162, socks5://127.0.0.1:18163) res.header(x-mixed-chain: http-socks)
```

Observed:

- `rules check` accepted the mixed HTTP/SOCKS upstream chain.
- `rules test 'http://127.0.0.1:18161/mixed?via=rsproxy'` rendered `upstream(proxy://127.0.0.1:18162, socks5://127.0.0.1:18163)` and the response header action.
- `curl -x http://127.0.0.1:18907 'http://127.0.0.1:18161/mixed?via=rsproxy'` returned the origin body and injected `X-Mixed-Chain: http-socks`.
- `p1` logged `CONNECT 127.0.0.1:18163`, and the origin logged the final `GET /mixed?via=rsproxy`, proving the HTTP request path connected through the HTTP proxy to the SOCKS5 hop before reaching the origin.
- `curl --proxytunnel -x http://127.0.0.1:18907 'http://127.0.0.1:18161/mixed-connect?via=rsproxy'` returned the rsproxy `200 Connection Established` line followed by the origin `200 OK` response.
- For the CONNECT case, `p1` again logged `CONNECT 127.0.0.1:18163`, and `trace get 2` recorded `upstream:"proxy://127.0.0.1:18162->socks5://127.0.0.1:18163->127.0.0.1:18161"` with flags `["tunnel","no-ca","upstream"]`.
- `trace get 1` recorded `upstream:"proxy://127.0.0.1:18162->socks5://127.0.0.1:18163"` and the injected response header.
- `trace stats` showed two spilled sessions, two spill index entries, and zero corrupt spill records; `/api/sessions/spill.ndjson` preserved both sessions.

Optimization from observation:

- Replaced the HTTP-only proxy chain route with a generic `ProxyChain` made of `ProxyHop::Http` and `ProxyHop::Socks5` hops.
- `upstream(...)` now accepts comma-separated `proxy://` / `http://` / `socks://` / `socks5://` hops for mixed HTTP/SOCKS chains.
- HTTP requests through a chain use absolute-form only when the final hop is an HTTP proxy; chains ending in SOCKS5 perform SOCKS CONNECT to the origin and then send origin-form HTTP.
- CONNECT passthrough through a mixed chain uses each hop's native tunnel operation: HTTP CONNECT for HTTP proxy hops and SOCKS CONNECT for SOCKS5 hops.
- Kept SOCKS username/password support and trace rule redaction when SOCKS5 authenticated hops appear in a chain.
- Added focused unit tests for mixed HTTP-to-SOCKS parsing, SOCKS-to-HTTP parsing with auth redaction, route labels, and tunnel labels.

## Loop 41

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18171`.
- rsproxy dogfood CA storage in `/tmp/rsproxy-dogfood41`; `rsproxy ca init` and `rsproxy ca issue 127.0.0.1` generated a root CA and HTTPS-proxy leaf certificate.
- First HTTPS proxy chain: local HTTPS CONNECT proxy on `127.0.0.1:18172`, then local SOCKS5 no-auth proxy on `127.0.0.1:18173`.
- Second HTTPS proxy chain: local HTTP CONNECT proxy on `127.0.0.1:18174`, then local HTTPS proxy on `127.0.0.1:18175` supporting absolute-form HTTP forwarding and CONNECT.
- rsproxy foreground daemon on `127.0.0.1:18908` with control API `127.0.0.1:18926`, sharing the same storage so the upstream TLS client trusts the dogfood CA.
- Rules exercised:

```text
127.0.0.1:18171 bypass upstream(https-proxy://127.0.0.1:18172, socks5://127.0.0.1:18173) res.header(x-https-chain: yes)
127.0.0.1:18171 bypass upstream(proxy://127.0.0.1:18174, https-proxy://127.0.0.1:18175) res.header(x-https-final: yes)
```

Observed:

- `rules check` accepted both HTTPS-proxy multi-hop chains, and `rules test` rendered `Bypass`, `upstream(...)`, and the response header action.
- For `https-proxy://127.0.0.1:18172, socks5://127.0.0.1:18173`, `curl -x http://127.0.0.1:18908 'http://127.0.0.1:18171/https-chain?via=rsproxy'` returned the origin body and injected `X-Https-Chain: yes`.
- The HTTPS proxy logged `CONNECT 127.0.0.1:18173`, and the SOCKS proxy logged `CONNECT 127.0.0.1:18171`, proving rsproxy established TLS to the first proxy, then CONNECTed to the SOCKS hop before reaching origin.
- With `bypass` enabled, `curl --proxytunnel -x http://127.0.0.1:18908 'http://127.0.0.1:18171/https-chain-connect?via=rsproxy'` returned the rsproxy `200 Connection Established` line followed by the origin `200 OK` response.
- `trace get 3` recorded `upstream:"https-proxy://127.0.0.1:18172->socks5://127.0.0.1:18173"`, and `trace get 4` recorded `upstream:"https-proxy://127.0.0.1:18172->socks5://127.0.0.1:18173->127.0.0.1:18171"`.
- For `proxy://127.0.0.1:18174, https-proxy://127.0.0.1:18175`, `curl -x http://127.0.0.1:18908 'http://127.0.0.1:18171/https-final?via=rsproxy'` returned the origin body and injected `X-Https-Final: yes`.
- The HTTP proxy logged `CONNECT 127.0.0.1:18175`, and the final HTTPS proxy logged `FORWARD GET http://127.0.0.1:18171/https-final?via=rsproxy`, proving the final HTTPS proxy received the absolute-form HTTP request after the TLS hop.
- `curl --proxytunnel -x http://127.0.0.1:18908 'http://127.0.0.1:18171/https-final-connect?via=rsproxy'` returned the origin `200 OK` through the tunnel, and the final HTTPS proxy logged `CONNECT 127.0.0.1:18171`.
- `trace get 5` recorded `upstream:"proxy://127.0.0.1:18174->https-proxy://127.0.0.1:18175"`, and `trace get 6` recorded `upstream:"proxy://127.0.0.1:18174->https-proxy://127.0.0.1:18175->127.0.0.1:18171"`.
- `trace stats` showed two spilled sessions, two spill index entries, and zero corrupt spill records after the final run; `/api/sessions/spill.ndjson` preserved both sessions.

Optimization from observation:

- Added `ProxyHop::Https` so `https-proxy://` can participate in `ProxyChain` alongside HTTP and SOCKS5 hops.
- Multi-hop connection setup now returns `UpstreamStream`, allowing a chain to switch from raw TCP to TLS when it reaches an HTTPS proxy hop.
- HTTP and CONNECT paths now use the same chain stream for final-hop operations: HTTP/HTTPS proxy hops use HTTP CONNECT or absolute-form forwarding; SOCKS5 hops use SOCKS CONNECT.
- Final-hop `https-proxy://` uses absolute-form HTTP request forwarding just like single-hop HTTPS proxy.
- Nested multiple `https-proxy://` hops are rejected with an explicit `stage=tls` error rather than silently attempting unsupported TLS-over-TLS tunnel copying.
- Added focused unit tests for HTTP-to-HTTPS and HTTPS-to-SOCKS chain parsing, labels, and absolute-form decisions.

## Loop 42

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18181`, returning repeated text bodies large enough to show compression.
- rsproxy foreground daemon on `127.0.0.1:18909` with control API `127.0.0.1:18927`, storage `/tmp/rsproxy-dogfood42`, and trace resource flags:

```text
--trace-spill-compression zstd --trace-body-limit 16kb --trace-segment-size 64kb --trace-disk-budget 1mb
```

- Rule:

```text
127.0.0.1:18181 res.header(x-compressed-spill: yes) cache(21)
```

Observed:

- `status` before traffic showed `spill_path:"/tmp/rsproxy-dogfood42/trace/seg-000000000001.ndjson.zst"` and `spill_compression:"zstd"`.
- `rules check` accepted the rule, and `rules test 'http://127.0.0.1:18181/compress?via=rsproxy'` rendered `res.header(x-compressed-spill: yes)` and `cache(max-age=21)`.
- Two `curl -x http://127.0.0.1:18909` requests to `/compress-a` and `/compress-b` returned `X-Compressed-Spill: yes`, `Cache-Control: max-age=21`, and 8600-byte origin bodies.
- `trace stats` showed `spilled:2`, `spill_segments:1`, `spill_index_entries:2`, `spill_corrupt_records:0`, `spill_compression:"zstd"`, and `spill_path` ending in `.ndjson.zst`.
- The trace directory contained `seg-000000000001.ndjson.zst` and `seg-000000000001.ndjson.zst.idx`; the segment file began with zstd magic bytes `28b52ffd`.
- The `.idx` sidecar had two compressed-frame entries:
  `0 557 1 ed050f17` and `557 557 2 31127cd8`.
- `curl http://127.0.0.1:18927/api/sessions/spill.ndjson` returned two normal NDJSON rows after decompression, preserving the URLs, response headers, cache flag, body previews, and matched rule metadata.
- `trace clear` reset `spilled`, `spill_bytes`, `spill_segments`, and `spill_index_entries` to zero and removed both the `.ndjson.zst` segment and `.idx` sidecar.

Optimization from observation:

- Added `TraceSpillCompression` with `none` and `zstd` modes; existing deployments remain uncompressed by default.
- Added CLI flag `--trace-spill-compression none|zstd[:level]`, with `zstd` defaulting to level 1 and accepting levels 1 through 22.
- Compressed spill writes store each NDJSON record as an independent zstd frame containing the JSON line plus newline.
- The `.idx` sidecar now records compressed-frame offset/length plus the original JSON payload CRC32, allowing recovery to skip a corrupt compressed frame without losing the whole segment.
- `/api/sessions/spill.ndjson` still returns plain NDJSON; compressed segment handling is internal to the trace store.
- `trace stats` now exposes `spill_compression`.
- `trace clear`, disk-budget eviction, restarted segment scanning, and CRC recovery handle `.ndjson.zst` segments and `.ndjson.zst.idx` sidecars.
- Added focused tests for zstd round-trip/restart scanning and corrupt compressed-frame recovery.

## Loop 43

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18191`.
- rsproxy foreground daemon on `127.0.0.1:18910` with control API `127.0.0.1:18928`, storage `/tmp/rsproxy-dogfood43`, and `--trace-spill-compression zstd`.
- Rule:

```text
127.0.0.1:18191 res.header(x-tui-loop: yes) cache(33)
```

Observed:

- `rules check` accepted the rule, and `rules test 'http://127.0.0.1:18191/tui-a?via=rsproxy'` rendered `res.header(x-tui-loop: yes)` and `cache(max-age=33)`.
- `rsproxy tui --api 127.0.0.1:18928 --once --limit 5` connected to the control API before traffic and printed a TUI snapshot with `status=running`, proxy/API/storage, `sessions=0`, and `spill_compression=zstd`.
- Two `curl -x http://127.0.0.1:18910` requests to `/tui-a` and `/tui-b` returned `X-Tui-Loop: yes`, `Cache-Control: max-age=33`, and origin bodies.
- `trace ls` showed two HTTP sessions; `trace stats` showed `sessions:2`, `spilled:2`, `spill_compression:"zstd"`, and `spill_index_entries:2`.
- `rsproxy tui --once` then rendered the same two sessions in table form and included selected detail:
  `selected id=2 upstream=127.0.0.1:18191 flags=cache error=`.
- The interactive `rsproxy tui --api 127.0.0.1:18928 --limit 5 --interval-ms 500` mode was launched in a PTY; it rendered a ratatui status panel, recent session table, detail panel, and footer commands, then exited cleanly on `q`.

Optimization from observation:

- Added `rsproxy tui` using ratatui + crossterm as a control-API client.
- Added interactive controls: `q`/Esc quit, `r` refresh, and up/down selection.
- Added `--once` snapshot mode for scriptable dogfooding and non-interactive terminals.
- The TUI displays daemon status, proxy/API/storage, trace counters, spill compression, recent sessions, and selected session detail.
- Added focused unit tests for snapshot rendering and URL truncation.

## Loop 44

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18192`.
- rsproxy foreground daemon on `127.0.0.1:18911` with control API `127.0.0.1:18929`, storage `/tmp/rsproxy-dogfood44`, and `--trace-spill-compression zstd`.
- Rule:

```text
127.0.0.1:18192 res.header(x-tui-advanced: yes) cache(44)
```

Observed:

- `rules check` accepted the rule, and `rules test 'http://127.0.0.1:18192/alpha?via=rsproxy'` rendered `res.header(x-tui-advanced: yes)` and `cache(max-age=44)`.
- Two `curl -x http://127.0.0.1:18911` requests to `/alpha` and `/beta` returned `X-Tui-Advanced: yes`, `Cache-Control: max-age=44`, and origin bodies.
- `rsproxy tui --api 127.0.0.1:18929 --once --limit 10 --filter beta --tab headers` rendered only the `/beta` session and showed request/response headers, including `X-Tui-Advanced: yes` and `Cache-Control: max-age=44`.
- `rsproxy tui --once --filter beta --tab body` showed the `/beta` response body preview, and `--tab rules` showed the matched `res.header` plus `cache(44)` rule.
- `trace ls` showed two HTTP sessions; `trace stats` showed `sessions:2`, `spilled:2`, `spill_compression:"zstd"`, `spill_index_entries:2`, and no corrupt spill records.
- The interactive TUI was launched with initial `--filter beta`; Tab switched detail tabs, `r` replayed the selected session through `/api/replay/{id}`, and the origin log recorded an extra `/beta` request before exiting cleanly on `q`.
- `rsproxy replay 2 --api 127.0.0.1:18929` also replayed the filtered session and returned HTTP status 200.

Optimization from observation:

- Added local session filtering through `--filter` and interactive `/` input.
- Added detail tabs for overview, headers, body previews, and matched rules through `--tab`, Tab, and BackTab.
- Added selected-session replay from the TUI with `r`, while `R` now performs explicit refresh.
- Expanded snapshot output so dogfooding can assert filter state, active tab, selected session detail, and replay status.
- Added focused unit tests for filter matching, detail tab parsing/cycling, and tab-specific snapshot rendering.

## Loop 45

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18193`.
- Two local HTTPS proxies on `127.0.0.1:18194` and `127.0.0.1:18195`, each using a `127.0.0.1` certificate issued by the rsproxy dogfood root CA in `/tmp/rsproxy-dogfood45`.
- rsproxy foreground daemon on `127.0.0.1:18912` with control API `127.0.0.1:18930`, sharing the same storage so its upstream TLS client trusts the dogfood CA, and `--trace-spill-compression zstd`.
- HTTP forwarding rule:

```text
127.0.0.1:18193 upstream(https-proxy://127.0.0.1:18194, https-proxy://127.0.0.1:18195) res.header(x-nested-https-chain: yes) cache(45)
```

- CONNECT passthrough rule:

```text
127.0.0.1:18193 bypass upstream(https-proxy://127.0.0.1:18194, https-proxy://127.0.0.1:18195)
```

Observed:

- `rsproxy ca init` and `rsproxy ca issue 127.0.0.1` generated the CA plus HTTPS-proxy leaf certificate used by both local proxy hops.
- Direct `curl --proxy https://127.0.0.1:18195 --proxy-cacert ...` to the second HTTPS proxy returned the origin body, proving the local HTTPS proxy fixture was valid before routing through rsproxy.
- `rules check` accepted the nested two-HTTPS-proxy rule, and `rules test 'http://127.0.0.1:18193/nested?via=rsproxy'` rendered `upstream(https-proxy://127.0.0.1:18194, https-proxy://127.0.0.1:18195)`, `res.header(x-nested-https-chain: yes)`, and `cache(max-age=45)`.
- `curl -x http://127.0.0.1:18912 'http://127.0.0.1:18193/nested?via=rsproxy'` returned `200`, `X-Nested-Https-Chain: yes`, `Cache-Control: max-age=45`, and the origin body.
- The first HTTPS proxy logged `CONNECT 127.0.0.1:18195`, the second HTTPS proxy logged `FORWARD GET http://127.0.0.1:18193/nested?via=rsproxy`, and the origin logged the final `/nested` request, proving TLS-to-proxy was nested across both hops.
- With the `bypass` rule, `curl --proxytunnel -x http://127.0.0.1:18912 'http://127.0.0.1:18193/tunnel?via=rsproxy'` returned the rsproxy `200 Connection Established` line followed by the origin `200 OK` response.
- For the CONNECT case, the first HTTPS proxy logged `CONNECT 127.0.0.1:18195`, the second HTTPS proxy logged `CONNECT 127.0.0.1:18193`, and the origin logged `/tunnel`, proving nested TLS also works for passthrough tunnels.
- `trace get 1` recorded `upstream:"https-proxy://127.0.0.1:18194->https-proxy://127.0.0.1:18195"` with the injected header/cache rule; `trace get 2` recorded `kind:"tunnel"` and `upstream:"https-proxy://127.0.0.1:18194->https-proxy://127.0.0.1:18195->127.0.0.1:18193"`.
- `trace stats` showed `sessions:2`, `spilled:2`, `spill_compression:"zstd"`, `spill_index_entries:2`, and zero corrupt records.

Optimization from observation:

- Changed `UpstreamStream::Tls` from a TCP-only rustls stream to a boxed recursive `StreamOwned<ClientConnection, UpstreamStream>`.
- `tls_wrap_upstream_stream` now wraps any existing upstream stream, so each `https-proxy://` hop can apply TLS over the tunnel created by the previous hop.
- Added recursive nonblocking and shutdown handling for TLS tunnel copying, preserving CONNECT passthrough behavior when the outer stream itself is another TLS stream.
- Removed the explicit nested-HTTPS-proxy rejection and added a focused unit test for nested HTTPS proxy chain parsing, labels, and absolute-form behavior.

## Loop 46

Runtime setup:

- Plain TCP WebSocket origin on `127.0.0.1:18196`.
- rsproxy foreground daemon on `127.0.0.1:18913` with control API `127.0.0.1:18931`, storage `/tmp/rsproxy-dogfood46`, `--trace-body-limit 4`, and `--trace-spill-compression zstd`.
- Rule:

```text
127.0.0.1:18196 res.header(x-ws-trace: dogfood46)
```

Observed:

- `rules check` accepted the rule, and `rules test 'http://127.0.0.1:18196/ws?via=rules'` rendered `res.header(x-ws-trace: dogfood46)`.
- `curl -x http://127.0.0.1:18913` with WebSocket upgrade headers returned the proxied `101 Switching Protocols` response, including origin header `X-Origin-Ws: dogfood46` and injected `X-Ws-Trace: dogfood46`; curl timed out after receiving initial WebSocket frames because the upgraded connection intentionally stayed open.
- A raw Python WebSocket client connected through rsproxy, received a server `ping` and binary frame, replied with `pong`, then sent fragmented text (`text` + `continuation`), a binary frame, a client `ping`, and close.
- The origin log recorded the expected client frames:
  `pong` for `srv-ping`, `text` with `fin=false`, `continuation` with `fin=true`, binary bytes `00010203040506`, client `ping`, and close.
- `trace get 2` recorded a WebSocket session with `status:101`, injected response header, and frame metadata:
  - server `ping`: `opcode:"ping"`, `payload_len:8`, `preview_len:4`, `data:"srv-"`, `truncated:true`
  - server binary: `opcode:"binary"`, `data_encoding:"hex"`, `data:"00ff1020"`, `truncated:true`
  - client fragmented text: `opcode:"text"`, `fin:false`, then `opcode:"continuation"`, `fin:true`, both using `data_encoding:"utf8"`
  - client binary: `opcode:"binary"`, `data_encoding:"hex"`, `data:"00010203"`, `truncated:true`
  - client `ping` and server `pong`, each preserved as control opcodes.
- `/api/sessions/spill.ndjson` preserved the same frame metadata from the zstd spill segment, and `trace stats` showed `sessions:2`, `spilled:2`, `spill_compression:"zstd"`, `spill_index_entries:2`, and zero corrupt records.

Optimization from observation:

- Extended `FrameRecord` with `opcode`, `fin`, `payload_len`, `preview_len`, `data_encoding`, and `truncated` while keeping the existing `data` field.
- WebSocket frame parsing now records FIN and opcode names for text, binary, continuation, close, ping, and pong frames.
- Added per-direction fragmentation state so continuation frames inherit text vs binary preview encoding from their fragmented message.
- Binary frame previews are emitted as hex and respect `--trace-body-limit`; text/control previews remain UTF-8 when valid.
- Ping/pong policy is now explicit in trace: control frames are forwarded unchanged and recorded with their own opcodes.
- Added focused unit tests for masked frame parsing, fragmentation metadata, binary preview truncation, and ping opcode recording.

## Loop 47

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18197`.
- rsproxy foreground daemon on `127.0.0.1:18914` with control API `127.0.0.1:18932`, storage `/tmp/rsproxy-dogfood47`, and `--trace-spill-compression zstd`.
- Rule file with 54 rules:
  - 50 unrelated exact-domain rules like `noiseN.example.test`.
  - One exact-domain target rule for `api.dogfood47.test`.
  - One suffix-domain rule for `**.dogfood47.test` with `host(127.0.0.1:18197)`.
  - One regex rule `/dogfood47-literal/`.
  - One global `*` rule.

Observed:

- `rules check /tmp/rsproxy-dogfood47/default.rules` accepted all 54 rules.
- `rsproxy rules stats --file ...` and API-backed `rules stats --api 127.0.0.1:18932` both reported:
  `domain_exact_entries=51`, `domain_suffix_entries=1`, `indexed_rules=52`, `global_rules=2`, and `prefilter_literals=1`.
- `rules test 'http://api.dogfood47.test/dogfood47-literal?via=rules'` rendered only the expected indexed/global matches:
  exact line 51 (`res.header` + `cache(47)`), suffix line 52 (`host` + `res.header`), regex line 53, and global line 54.
- `curl -x http://127.0.0.1:18914 'http://api.dogfood47.test/dogfood47-literal?via=rsproxy'` reached the local origin through `host(127.0.0.1:18197)` while preserving `Host: api.dogfood47.test`.
- The curl response contained `X-Exact-Index: yes`, `Cache-Control: max-age=47`, `X-Suffix-Index: yes`, `X-Prefilter-Regex: yes`, and `X-Global-Index: yes`.
- `trace get 1` recorded only rules 51-54 as matched, upstream `127.0.0.1:18197`, cache flag, and the injected response headers.
- `trace stats` showed one zstd-spilled session, one spill index entry, and zero corrupt records.

Optimization from observation:

- Added a private `RuleIndex` to `RuleSet` with exact-domain buckets, suffix-domain buckets, global rule indices, and conservative regex literal prefilter metadata.
- `resolve()` now computes a candidate rule set from the request host, merges exact/suffix/global candidates, deduplicates them, then sorts by `@important` and line number before running the original matcher and condition checks.
- This keeps current priority and capture semantics intact while excluding unrelated exact/suffix domain rules before expensive matcher evaluation.
- Regex prefiltering is conservative: only simple regexes with a safe required literal get a prefilter literal; complex patterns still stay in the global verification path.
- Added `rsproxy rules stats` for scriptable observation of index shape.
- Added focused tests for index stats, candidate reduction without ordering changes, and regex prefilter safety.

## Loop 48

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18198`.
- rsproxy foreground daemon on `127.0.0.1:18915` with control API `127.0.0.1:18933`, storage `/tmp/rsproxy-dogfood48`, and `--trace-spill-compression zstd`.
- Rule file with exactly 10,000 rules:
  - 7,996 unrelated exact-domain rules like `noiseN.bench.test`.
  - One exact-domain target rule for `api.dogfood48.test`.
  - One suffix-domain rule for `**.dogfood48.test` with `host(127.0.0.1:18198)`.
  - 2,000 unrelated simple regex literal rules.
  - One target regex `/dogfood48-literal/`.
  - One global `*` rule.

Observed:

- `rules check /tmp/rsproxy-dogfood48/default.rules` accepted all 10,000 rules.
- `rules stats --file ...` and API-backed `rules stats --api 127.0.0.1:18933` both reported:
  `domain_exact_entries=7997`, `domain_suffix_entries=1`, `indexed_rules=7998`, `global_rules=1`, `prefilter_literals=2001`, and `prefilter_rules=2001`.
- Debug `rules bench` for 2,000 iterations reported `p50_ns=17250`, `p99_ns=18959`; API-backed debug bench reported `p50_ns=17333`, `p99_ns=19125`.
- Release `rules bench` for 10,000 iterations reported `p50_ns=2709`, `p99_ns=3458`, and `max_ns=63750`, satisfying the design target of p99 < 10us for this 10k-rule dogfood set.
- `rules test 'http://api.dogfood48.test/dogfood48-literal?via=rules'` rendered only expected lines:
  exact line 7997, suffix line 7998, Aho-Corasick prefiltered regex line 9999, and global line 10000.
- `curl -x http://127.0.0.1:18915 'http://api.dogfood48.test/dogfood48-literal?via=rsproxy'` reached the local origin through `host(127.0.0.1:18198)` while preserving `Host: api.dogfood48.test`.
- The curl response contained `X-Exact-Aho: yes`, `Cache-Control: max-age=48`, `X-Suffix-Aho: yes`, `X-Aho-Prefilter: yes`, and `X-Global-Aho: yes`.
- `trace get 1` recorded upstream `127.0.0.1:18198`, cache flag, and matched rules 7997, 7998, 9999, and 10000 only.
- `trace stats` showed one zstd-spilled session, one spill index entry, and zero corrupt records.

Optimization from observation:

- Replaced the per-rule regex `contains` prefilter with a real Aho-Corasick automaton built from all conservative required regex literals.
- Regex rules with safe required literals are no longer kept in the always-global bucket; they are added to the candidate set only when the Aho-Corasick scan reports their literal.
- Multiple regex rules can share one literal bucket, and matched prefilter candidates are deduplicated before line-order sorting.
- Added `rsproxy rules bench` with configurable `--url`, `--iterations`, and `--warmup`, printing p50/p99/max timing plus index stats.
- Added focused unit coverage for shared-literal Aho-Corasick buckets.

## Loop 49

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18199`.
- rsproxy foreground daemon on `127.0.0.1:18916` with control API `127.0.0.1:18934`, storage `/tmp/rsproxy-dogfood49`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- One default rule: `127.0.0.1:18199 res.header(x-system-proxy-branch: loop49) cache(49)`.

Observed:

- `rsproxy proxy on --platform windows --host 127.0.0.1 --port 18916 --bypass localhost,127.0.0.1 --dry-run` printed `reg add` commands for `ProxyEnable=1`, `ProxyServer=http=127.0.0.1:18916;https=127.0.0.1:18916`, and `ProxyOverride=localhost;127.0.0.1`, followed by `proxy_on platform=windows host=127.0.0.1 port=18916`.
- `rsproxy proxy off --platform windows ... --dry-run` printed `ProxyEnable=0` plus `reg delete` commands for `ProxyServer` and `ProxyOverride`.
- `rsproxy proxy status --platform windows --dry-run` printed `reg query` commands for `ProxyEnable`, `ProxyServer`, and `ProxyOverride`.
- `rsproxy proxy on --platform linux --host 127.0.0.1 --port 18916 --bypass localhost,127.0.0.1 --dry-run` printed GNOME `gsettings` commands for manual HTTP/HTTPS proxy host/port, `ignore-hosts`, and shell `http_proxy`/`https_proxy`/`all_proxy` exports.
- `rsproxy proxy off --platform linux ... --dry-run` printed `gsettings set org.gnome.system.proxy mode none` and the corresponding proxy environment unsets.
- `rsproxy proxy status --platform linux --dry-run` printed `gsettings get` probes for mode, HTTP/HTTPS host/port, and ignore-hosts.
- Non-dry-run Windows/Linux commands refused to mutate the host and returned clear errors requiring `--dry-run`.
- `rules stats --api 127.0.0.1:18934` reported one exact-domain indexed rule and zero prefilter/global rules.
- `rules test 'http://127.0.0.1:18199/system-proxy?via=rsproxy'` rendered the expected `res.header` and `cache(max-age=49)` actions from line 1.
- Two `curl -x http://127.0.0.1:18916 'http://127.0.0.1:18199/system-proxy?via=rsproxy'` requests reached the origin through rsproxy and returned `X-System-Proxy-Branch: loop49` plus `Cache-Control: max-age=49`.
- `trace get 2` recorded upstream `127.0.0.1:18199`, the matched line 1 rule, cache flag, and the injected response header.
- `trace stats` showed two zstd-spilled sessions, one spill segment, two spill index entries, and zero corrupt records.

Implementation from observation:

- Added explicit `--platform macos|windows|linux` dispatch for `rsproxy proxy status|on|off`.
- Kept macOS on the existing `networksetup` implementation and limited Windows/Linux to `--dry-run` command-plan generation until native mutation and rollback tests are added.
- Added Windows registry dry-run rendering for status/on/off and Linux GNOME `gsettings` plus environment dry-run rendering for status/on/off.
- Added unit coverage for explicit platform alias parsing, Windows registry dry-run plans, and Linux gsettings/env dry-run plans.
- Updated CLI help to expose `--platform` and `--bypass`.

## Loop 50

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18200`.
- rsproxy foreground daemon on `127.0.0.1:18917` with control API `127.0.0.1:18935`, storage `/tmp/rsproxy-dogfood50`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- One default rule:
  `127.0.0.1:18200 res.cors(${reqH.origin}, methods=GET POST OPTIONS, headers=X-Token Content-Type, credentials=true, expose=X-Trace, max-age=600) res.header(x-rules-test-headers: ${reqH.origin}) cache(50)`.

Observed:

- `rules check` accepted the `${reqH.origin}` CORS/header rule.
- API-backed `rules test 'http://127.0.0.1:18200/cors?via=rules-test' -X OPTIONS -H 'Origin: https://app.loop50.test' -H 'Access-Control-Request-Headers: X-Token' --api 127.0.0.1:18935` rendered `res.cors(https://app.loop50.test, ...)`, `res.header(x-rules-test-headers: https://app.loop50.test)`, and `cache(max-age=50)`.
- Local fallback `rules test ... -H 'Origin: https://offline.loop50.test' --api 127.0.0.1:1 --storage /tmp/rsproxy-dogfood50` rendered the offline origin value, proving the CLI and daemon paths use the same request metadata.
- `rules bench --url ... -X OPTIONS -H 'Origin: https://bench.loop50.test' --iterations 1000 --warmup 10` completed with three matched actions per resolve path (`matched_actions=3030`) and p99 around 12us in debug.
- Two `curl -x http://127.0.0.1:18917 -H 'Origin: https://curl.loop50.test' ...` requests reached the origin through rsproxy and returned dynamic `Access-Control-Allow-Origin: https://curl.loop50.test`, `Access-Control-Allow-Methods`, `Access-Control-Allow-Headers`, `Access-Control-Allow-Credentials`, `Access-Control-Expose-Headers`, `Access-Control-Max-Age`, `Vary: Origin`, `X-Rules-Test-Headers: https://curl.loop50.test`, and `Cache-Control: max-age=50`.
- `trace get 1` recorded the request `Origin` and `Access-Control-Request-Headers`, the matched line 1 rule, `res-cors` and `cache` flags, and all injected response headers.
- `trace stats` showed two zstd-spilled sessions, one spill segment, two spill index entries, and zero corrupt records.
- Negative CLI dogfood with `-H 'Origin https://bad.loop50.test'` rejected the malformed header instead of misparsing the URL scheme colon.

Optimization from observation:

- Added repeated `-H` / `--header` parsing and `-X` / `--method` normalization for `rsproxy rules test`.
- Propagated headers through `/api/rules/test` using repeated `header=` query parameters, so online API-backed explain and offline storage fallback render `${reqH.*}` consistently.
- Reused the same request header parsing for `rules bench`, allowing benchmark runs for header/method-dependent rules.
- Added HTTP-token validation for header names to reject malformed `-H` values like `Origin https://...`.
- Added unit coverage for CLI request option parsing, API query repeated-header decoding, and malformed header rejection.

## Loop 51

Runtime setup:

- Header-echo HTTP origin on `127.0.0.1:18201`; it returns the received `X-Forwarded-For` value in `X-Origin-Forwarded` and the response body.
- rsproxy foreground daemon on `127.0.0.1:18918` with control API `127.0.0.1:18936`, storage `/tmp/rsproxy-dogfood51`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- One default rule:
  `127.0.0.1:18201 req.forwarded(${clientIp}) res.header(x-forwarded-rule: yes) cache(51)`.

Observed:

- `rules check` accepted the new `req.forwarded(${clientIp})` rule.
- Initial API-backed `rules test` rendered `req.forwarded()` because the explain path had no simulated client IP input, while the real proxy path does have peer address metadata.
- Initial curl dogfood proved the action was applied, but showed the raw peer address including port in `X-Origin-Forwarded: 127.0.0.1:<port>`.
- After optimization, API-backed `rules test ... --client-ip 203.0.113.51` rendered `req.forwarded(203.0.113.51)`.
- Offline fallback `rules test ... --client-ip 198.51.100.51 --api 127.0.0.1:1 --storage /tmp/rsproxy-dogfood51` rendered `req.forwarded(198.51.100.51)`.
- `curl -x http://127.0.0.1:18918 'http://127.0.0.1:18201/forwarded?via=curl2'` returned `X-Origin-Forwarded: 127.0.0.1`, `X-Forwarded-Rule: yes`, `Cache-Control: max-age=51`, and body `forwarded=127.0.0.1`.
- `trace get 1` recorded request header `X-Forwarded-For: 127.0.0.1`, matched line 1, cache flag, the origin echo header, and the injected response header.
- `rules bench --client-ip 192.0.2.51 --iterations 1000 --warmup 10` completed with three matched actions per resolve path.
- `trace stats` showed zstd spill enabled with zero corrupt records.

Optimization from observation:

- Implemented `req.forwarded(ip)` as a request action that sets `X-Forwarded-For`, with template rendering.
- Kept `req.forwarded` as a single-action family, so normal first-match semantics apply.
- Normalized socket-address values such as `127.0.0.1:61901` or `[2001:db8::1]:443` to just the IP before writing `X-Forwarded-For`.
- Added `--client-ip` to `rsproxy rules test` and `rules bench`, and propagated `clientIp` through `/api/rules/test`, so `${clientIp}` can be exercised without a live proxy connection.
- Added unit coverage for rule parsing/explain, first-match behavior, CLI/API `clientIp` propagation, and request header application.

## Loop 52

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18202`.
- rsproxy foreground daemon on `127.0.0.1:18919` with control API `127.0.0.1:18937`, storage `/tmp/rsproxy-dogfood52`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18202 res.header(x-ip-matched: yes) cache(52) when clientIp(127.0.0.1)`
  - `127.0.0.1:18202 res.header(x-ip-fallback: yes) cache(5)`

Observed:

- `rules check` accepted `when clientIp(127.0.0.1)`.
- `rules test ... --client-ip 127.0.0.1` rendered line 1 `res.header(x-ip-matched: yes)`, line 1 `cache(max-age=52)`, and line 2 `res.header(x-ip-fallback: yes)`.
- `rules test ... --client-ip 198.51.100.10` rendered only the fallback line 2 `res.header` and `cache(max-age=5)`.
- The matched explain output also confirmed existing stackable action semantics: `res.header` actions from later matching rules still stack, while single-action `cache` keeps first-match behavior.
- `rules bench --client-ip 127.0.0.1 --iterations 1000 --warmup 10` completed with `rules=2`, `indexed_rules=2`, and `matched_actions=3030`.
- `curl -x http://127.0.0.1:18919 'http://127.0.0.1:18202/ip?via=curl'` returned `X-Ip-Matched: yes`, `X-Ip-Fallback: yes`, and `Cache-Control: max-age=52`, proving the real peer IP matched line 1 while headers remained stackable.
- `trace get 1` recorded client `127.0.0.1:<port>`, matched rules 1 and 2, response headers `X-Ip-Matched`, `X-Ip-Fallback`, and `Cache-Control: max-age=52`.
- `trace stats` showed one zstd-spilled session, one spill index entry, and zero corrupt records.

Optimization from observation:

- Added `when clientIp(...)` plus `when ip(...)` alias for request-phase client IP conditions.
- Client IP conditions normalize socket-address values before matching, so real proxy peer strings like `127.0.0.1:<port>` match `clientIp(127.0.0.1)`.
- Client IP conditions accept multiple values and simple glob patterns such as `ip(203.0.*)`.
- Added focused unit coverage for exact match, glob match, alias parsing, socket-address normalization, and fallback behavior.
- Documented that header actions remain stackable even when a previous single-action family matched, so exclusive branch examples should use different header names or mutually exclusive conditions.

## Loop 53

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18203`.
- rsproxy foreground daemon on `127.0.0.1:18920` with control API `127.0.0.1:18938`, storage `/tmp/rsproxy-dogfood53`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18203 res.header(x-url-glob: yes) cache(53) when url(*mode=match*)`
  - `127.0.0.1:18203 res.header(x-url-regex: yes) cache(54) when url(/\/rx\/\d+\?ok=1/)`
  - `127.0.0.1:18203 res.header(x-url-fallback: yes) cache(6)`

Observed:

- `rules check` accepted both URL condition forms.
- `rules test 'http://127.0.0.1:18203/path?mode=match&x=1'` rendered line 1 `res.header(x-url-glob: yes)`, line 1 `cache(max-age=53)`, and line 3 fallback header.
- `rules test 'http://127.0.0.1:18203/rx/42?ok=1'` rendered line 2 `res.header(x-url-regex: yes)`, line 2 `cache(max-age=54)`, and line 3 fallback header.
- `rules test 'http://127.0.0.1:18203/path?mode=miss'` rendered only line 3 `res.header(x-url-fallback: yes)` and `cache(max-age=6)`.
- `rules bench --url 'http://127.0.0.1:18203/rx/42?ok=1' --iterations 1000 --warmup 10` completed with `rules=3`, `indexed_rules=3`, and `matched_actions=3030`.
- Curl through rsproxy confirmed proxy behavior:
  - `mode=match` returned `X-Url-Glob: yes`, fallback header, and `Cache-Control: max-age=53`.
  - `/rx/42?ok=1` returned `X-Url-Regex: yes`, fallback header, and `Cache-Control: max-age=54`.
  - `mode=miss` returned only fallback header and `Cache-Control: max-age=6`.
- `trace get 2` recorded URL `http://127.0.0.1:18203/rx/42?ok=1`, matched rules 2 and 3, response headers `X-Url-Regex`, `X-Url-Fallback`, and `Cache-Control: max-age=54`.
- `trace stats` showed three zstd-spilled sessions, three spill index entries, and zero corrupt records.

Optimization from observation:

- Added request-phase `when url(pattern)` conditions.
- `url(*glob*)` matches against the complete URL string and is suitable for query/path fragments.
- `url(/regex/i)` reuses the existing regex engine selection, including Rust `regex` by default and `fancy-regex` fallback where needed.
- Added focused unit coverage for URL glob, URL regex, and fallback behavior.

## Loop 54

Runtime setup:

- Header-controlled HTTP origin on `127.0.0.1:18204`; `/hit` returns `X-Origin-State: route-hit`, while other paths return `X-Origin-State: route-miss`.
- rsproxy foreground daemon on `127.0.0.1:18921` with control API `127.0.0.1:18939`, storage `/tmp/rsproxy-dogfood54`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18204 res.header(x-res-header-condition: matched) cache(54) when res.header(x-origin-state ~ hit)`
  - `127.0.0.1:18204 res.header(x-res-header-fallback: yes) cache(7)`

Observed:

- `rules check` accepted `when res.header(x-origin-state ~ hit)`.
- Before optimization, `rules test` and trace request-phase pre-resolution treated response-header conditions as temporarily matched. This made `/miss` trace include line 1 even though line 1 was not applied to the response.
- After optimization, request-phase `rules test 'http://127.0.0.1:18204/hit'` rendered only fallback line 2, accurately reflecting that response-header conditions need a real upstream response to evaluate.
- `curl -x http://127.0.0.1:18921 'http://127.0.0.1:18204/hit'` returned `X-Origin-State: route-hit`, `X-Res-Header-Condition: matched`, `X-Res-Header-Fallback: yes`, and `Cache-Control: max-age=54`.
- `curl -x http://127.0.0.1:18921 'http://127.0.0.1:18204/miss'` returned `X-Origin-State: route-miss`, only the fallback header, and `Cache-Control: max-age=7`.
- `trace get 1` for `/hit` recorded rules 1 then 2 in response-phase order and response headers `X-Res-Header-Condition`, `X-Res-Header-Fallback`, and `Cache-Control: max-age=54`.
- `trace get 2` for `/miss` recorded only rule 2, proving response-header condition misses are no longer falsely recorded.
- `trace stats` showed two zstd-spilled sessions, two spill index entries, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18204/hit' --iterations 1000 --warmup 10` completed with `rules=2`, `indexed_rules=2`, and `matched_actions=2020`, reflecting request-phase fallback-only explain/bench behavior.

Optimization from observation:

- Added response-phase `when res.header(name)` and `when res.header(name ~ value)` conditions.
- Response-header conditions no longer match during request-only resolution; they are evaluated only when `ResponseMeta` is available.
- Forwarding now returns response-phase matched rules to `handle_http_stream`, and trace merges request-phase-only rules with actual response-phase rules without duplicates.
- Trace rule ordering now keeps request-only rules first, then response-phase rules in actual response resolve order.
- Added focused unit coverage for response-header presence, substring matching, and fallback behavior.

## Loop 55

Runtime setup:

- Status-controlled HTTP origin on `127.0.0.1:18205`; `/missing` returns `404` with `status=missing`, while other paths return `200` with `status=ok`.
- rsproxy foreground daemon on `127.0.0.1:18922` with control API `127.0.0.1:18940`, storage `/tmp/rsproxy-dogfood55`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18205 res.header(x-status-condition: notfound) cache(55) when status(404)`
  - `127.0.0.1:18205 res.header(x-status-any: yes) when status(200, 404)`
  - `127.0.0.1:18205 res.header(x-status-fallback: yes) cache(8)`

Observed:

- `rules check` accepted all three rules.
- Request-only `rules test 'http://127.0.0.1:18205/missing'` rendered only fallback line 3, proving `when status(...)` is no longer treated as matched before an upstream response exists.
- `curl -x http://127.0.0.1:18922 'http://127.0.0.1:18205/missing'` returned upstream `404`, `X-Status-Condition: notfound`, `X-Status-Any: yes`, fallback header, and `Cache-Control: max-age=55`.
- `curl -x http://127.0.0.1:18922 'http://127.0.0.1:18205/ok'` returned upstream `200`, `X-Status-Any: yes`, fallback header, and `Cache-Control: max-age=8`, without the 404-only header.
- `trace get 1` for `/missing` recorded rules 1, 2, then 3 and response headers matching the 404 branch.
- `trace get 2` for `/ok` recorded only rules 2 and 3, proving the 404-only status condition is no longer falsely recorded.
- `trace stats` showed two zstd-spilled sessions, two spill index entries, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18205/missing' --iterations 1000 --warmup 10` completed with `rules=3`, `indexed_rules=3`, and `matched_actions=2020`, reflecting request-phase fallback-only explain/bench behavior.

Optimization from observation:

- Changed `when status(...)` to return false during request-only resolution, matching the documented two-phase response condition model.
- Reused the response-phase matched-rule merge path added in Loop 54, so status-conditioned rules now appear in trace only after actual response evaluation.
- Added focused unit coverage for request-only fallback behavior, 404 response matches, and 200 response fallback behavior.

## Loop 56

Runtime setup:

- Echo HTTP origin on `127.0.0.1:18206`, returning request method, path, and `X-Mode` header value in the response body.
- rsproxy foreground daemon on `127.0.0.1:18923` with control API `127.0.0.1:18941`, storage `/tmp/rsproxy-dogfood56`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18206 res.header(x-any-matched: yes) cache(56) when any(method(POST, PUT), header(x-mode ~ beta))`
  - `127.0.0.1:18206 res.header(x-any-fallback: yes) cache(9)`

Observed:

- `rules check` accepted nested `any(method(POST, PUT), header(x-mode ~ beta))`, covering both nested call parsing and multi-value method arguments.
- Request-only `rules test` with plain GET rendered only fallback line 2.
- `rules test ... -H 'X-Mode: beta-preview'` rendered line 1 `res.header`, line 1 `cache(max-age=56)`, and fallback header line 2.
- `rules test ... -X POST` rendered the same line 1 + fallback-header behavior without requiring the header branch.
- Curl through rsproxy confirmed proxy behavior:
  - Plain GET returned only `X-Any-Fallback: yes` and `Cache-Control: max-age=9`.
  - GET with `X-Mode: beta-preview` returned `X-Any-Matched: yes`, fallback header, and `Cache-Control: max-age=56`.
  - POST without `X-Mode` returned `X-Any-Matched: yes`, fallback header, and `Cache-Control: max-age=56`.
- `trace get 1` recorded the header branch with rules 1 then 2 and the expected request header.
- `trace get 2` recorded only fallback rule 2 for the miss branch.
- `trace get 3` recorded the method branch with rules 1 then 2 and method `POST`.
- `trace stats` showed three zstd-spilled sessions, three spill index entries, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18206/path?case=header' -H 'X-Mode: beta-preview' --iterations 1000 --warmup 10` completed with `rules=2`, `indexed_rules=2`, and `matched_actions=3030`.

Optimization from observation:

- Added explicit OR `when any(cond1, cond2, ...)` conditions.
- Fixed argument splitting to track nested parentheses, so nested calls such as `any(method(POST, PUT), header(x-mode ~ beta))` parse correctly instead of splitting on the comma inside `method(...)`.
- Added focused unit coverage for OR hit via header, OR hit via method, and miss fallback behavior.

## Loop 57

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18207`, returning `env-loop path=...`.
- rsproxy foreground daemon on `127.0.0.1:18924` with control API `127.0.0.1:18942`, storage `/tmp/rsproxy-dogfood57`, `RSPROXY_LOOP57=enabled`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18207 res.header(x-env-disabled: unexpected) cache(99) when env(RSPROXY_LOOP57=disabled)`
  - `127.0.0.1:18207 res.header(x-env-present: yes) when env(RSPROXY_LOOP57)`
  - `127.0.0.1:18207 res.header(x-env-matched: enabled) cache(57) when env(RSPROXY_LOOP57=enabled)`
  - `127.0.0.1:18207 res.header(x-env-fallback: yes) cache(10)`

Observed:

- `rules check` accepted both `env(name)` and `env(name=value)` conditions.
- API-backed `rules test 'http://127.0.0.1:18207/env?case=enabled'` rendered line 2 `res.header`, line 3 `res.header`, line 3 `cache(max-age=57)`, and line 4 fallback header. The disabled env-value branch did not match.
- Curl through rsproxy returned `X-Env-Present: yes`, `X-Env-Matched: enabled`, fallback header, and `Cache-Control: max-age=57`.
- `trace get 1` recorded only rules 2, 3, and 4, proving the non-matching env-value branch was not reported.
- `trace stats` showed one zstd-spilled session, one spill index entry, and zero corrupt records.
- `rules bench` without `RSPROXY_LOOP57=enabled` used the CLI process environment and produced fallback-only `matched_actions=2020`.
- Re-running `RSPROXY_LOOP57=enabled rsproxy rules bench ... --iterations 1000 --warmup 10` aligned the bench environment with the daemon and produced `rules=4`, `indexed_rules=4`, and `matched_actions=4040`.

Optimization from observation:

- Added `when env(name)` for process environment presence checks.
- Added `when env(name=value)` for exact process environment value checks.
- Documented that local `rules bench` evaluates env conditions in the CLI process environment, so dogfood and scripts should run it with the same env overlay as the daemon when benchmarking env-dependent rules.
- Added focused unit coverage for env presence, exact value match, and fallback behavior.

## Loop 58

Runtime setup:

- POST echo HTTP origin on `127.0.0.1:18208`, returning request body length and body content.
- rsproxy foreground daemon on `127.0.0.1:18925` with control API `127.0.0.1:18943`, storage `/tmp/rsproxy-dogfood58`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18208 res.header(x-body-substring: yes) cache(58) when body(~ beta-token)`
  - `127.0.0.1:18208 res.header(x-body-regex: yes) cache(59) when body(/token=\d+/)`
  - `127.0.0.1:18208 res.header(x-body-fallback: yes) cache(11)`

Observed:

- `rules check` accepted both body substring and regex conditions.
- API-backed `rules test ... -X POST --body 'alpha beta-token'` rendered line 1 `res.header`, line 1 `cache(max-age=58)`, and fallback header line 3.
- API-backed `rules test ... -X POST --body 'token=42'` rendered line 2 `res.header`, line 2 `cache(max-age=59)`, and fallback header line 3.
- API-backed `rules test ... -X POST --body 'plain'` rendered only fallback line 3 with `cache(max-age=11)`.
- Curl through rsproxy confirmed proxy behavior:
  - POST body `alpha beta-token` returned `X-Body-Substring: yes`, fallback header, and `Cache-Control: max-age=58`.
  - POST body `token=42` returned `X-Body-Regex: yes`, fallback header, and `Cache-Control: max-age=59`.
  - POST body `plain` returned only fallback header and `Cache-Control: max-age=11`.
- Trace recorded request body heads for all three requests and matched only the expected body-conditioned rules; parallel curl execution made trace IDs order by completion rather than command order.
- `trace stats` showed three zstd-spilled sessions, three spill index entries, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18208/post' -X POST --body 'token=42' --iterations 1000 --warmup 10` completed with `rules=3`, `indexed_rules=3`, and `matched_actions=3030`.

Optimization from observation:

- Added request metadata body storage so `when body(...)` can evaluate against the original incoming request body before request-body rewrite actions.
- Added `when body(~ value)` for case-insensitive request body substring matching.
- Added `when body(/regex/i)` for request body regex matching using the existing compiled regex engine and fancy-regex fallback.
- Added `--body`/`-d` support to `rules test` and `rules bench`, and propagated `body=` through the control API explain endpoint.
- Added focused unit coverage for body substring hit, regex hit, and fallback behavior.

## Loop 59

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18209`, echoing the received `X-Server-Ip` request header in the response body.
- rsproxy foreground daemon on `127.0.0.1:18926` with control API `127.0.0.1:18944`, storage `/tmp/rsproxy-dogfood59`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18209 req.header(x-server-ip: ${serverIp}) res.header(x-server-ip-match: yes) cache(59) when serverIp(127.0.0.1)`
  - `127.0.0.1:18209 res.header(x-server-ip-fallback: yes) cache(12)`

Observed:

- `rules check` accepted `when serverIp(127.0.0.1)`.
- API-backed `rules test 'http://127.0.0.1:18209/server-ip?case=hit'` inferred the URL host literal IP and rendered line 1 `req.header(x-server-ip: 127.0.0.1)`, line 1 `res.header`, line 1 `cache(max-age=59)`, and fallback header line 2.
- API-backed `rules test ... --server-ip 198.51.100.9` overrode the inferred target IP and rendered only fallback line 2 with `cache(max-age=12)`.
- Curl through rsproxy returned `X-Server-Ip-Match: yes`, fallback header, and `Cache-Control: max-age=59`; the origin response body showed `server-ip-header=127.0.0.1`, proving `${serverIp}` rendered into the request header.
- `trace get 1` recorded rules 1 then 2, request header `X-Server-Ip: 127.0.0.1`, response headers for the match/fallback, and the expected response body head.
- `trace stats` showed one zstd-spilled session, one spill index entry, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18209/server-ip?case=bench' --iterations 1000 --warmup 10` completed with `rules=2`, `indexed_rules=2`, and `matched_actions=4040`.

Optimization from observation:

- Added `when serverIp(...)` request-phase conditions using the target URL host when it is a literal IP.
- Added `--server-ip` to `rules test` and `rules bench`, plus `serverIp=` propagation through `/api/rules/test`, so offline explain can override the inferred value.
- Added `${serverIp}` template rendering.
- Kept domain targets as `None` for `serverIp` rather than pretending DNS resolution has happened during request-phase rule resolution.
- Added focused unit coverage for exact server IP matching, glob matching, fallback, and template rendering.

## Loop 60

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18210`; `/html` returns `text/html`, `/css` returns `text/css`, and `/text` returns `text/plain`.
- rsproxy foreground daemon on `127.0.0.1:18927` with control API `127.0.0.1:18945`, storage `/tmp/rsproxy-dogfood60`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18210/html inject(html, "<!--pre:${path}-->", prepend) inject(html, "<!--tail-->") res.header(x-inject-html: yes)`
  - `127.0.0.1:18210/css inject(css, "/*tail*/") res.header(x-inject-css: yes)`
  - `127.0.0.1:18210 res.header(x-inject-fallback: yes) cache(60)`

Observed:

- `rules check` accepted `inject(html, value, prepend)`, default append mode, and `inject(css, value)`.
- API-backed `rules test 'http://127.0.0.1:18210/html'` rendered both stackable inject actions with `${path}` expanded, plus the HTML response header and fallback cache/header.
- API-backed `rules test 'http://127.0.0.1:18210/css'` rendered the CSS inject action, CSS response header, and fallback cache/header.
- Curl through rsproxy confirmed proxy behavior:
  - `/html` returned `<!--pre:/html--><html><body>origin</body></html><!--tail-->`, `X-Inject-Html: yes`, fallback header, `Cache-Control: max-age=60`, and updated `Content-Length: 59`.
  - `/css` returned `body{color:black}/*tail*/`, `X-Inject-Css: yes`, fallback header, and updated `Content-Length: 25`.
  - `/text` returned unchanged `plain-body` with only fallback header/cache, proving Content-Type gating prevents HTML/CSS injection into `text/plain`.
- `trace get 1/2/3` recorded the injected response body heads for HTML/CSS, unchanged text body, expected matched rules, and zero errors.
- `trace stats` showed three zstd-spilled sessions, three spill index entries, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18210/html' --iterations 1000 --warmup 10` completed with `rules=3`, `indexed_rules=3`, and `matched_actions=5050`.

Optimization from observation:

- Added stackable `inject(html|js|css, value[, append|prepend|replace])` response body actions.
- Inject actions are gated by the current response `Content-Type` after earlier response header/type actions in rule order.
- Inject actions force full-body handling for SSE responses rather than streaming passthrough, matching existing response body mutation behavior.
- Added focused unit coverage for parsing/explain, Content-Type gating, prepend mode, append no-op on nonmatching type, and replace mode.

## Loop 61

Runtime setup:

- No origin was started on `127.0.0.1:18211`; this intentionally verified that mock actions short-circuit before upstream connection.
- rsproxy foreground daemon on `127.0.0.1:18928` with control API `127.0.0.1:18946`, storage `/tmp/rsproxy-dogfood61`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18211/raw mock.raw('HTTP/1.1 207 Multi-Status\r\nContent-Type: application/json\r\nX-Raw-Mock: ${path}\r\n\r\n{"raw":"${path}","query":"${query}"}')`
  - `127.0.0.1:18211 mock("plain ${path}\n")`

Observed:

- `rules check` accepted `mock.raw(...)`.
- API-backed `rules test 'http://127.0.0.1:18211/raw?case=dogfood'` rendered the raw response with `${path}` and `${query}` expanded.
- API-backed `rules test 'http://127.0.0.1:18211/plain?case=fallback'` rendered the regular `mock(...)` fallback.
- `lsof` confirmed nothing was listening on `127.0.0.1:18211`.
- Curl through rsproxy for `/raw` returned `HTTP/1.1 207 Multi-Status`, `Content-Type: application/json`, `X-Raw-Mock: /raw`, generated `Content-Length: 37`, and body `{"raw":"/raw","query":"case=dogfood"}`.
- Curl through rsproxy for `/plain` returned the existing regular mock behavior: `200 OK`, `Content-Type: text/plain; charset=utf-8`, and body `plain /plain`.
- `trace get 1` recorded status `207`, `upstream:null`, `flags:["mock"]`, raw response headers/body, and only rule 1.
- `trace get 2` recorded regular mock status `200`, `upstream:null`, text/plain response headers/body, and only rule 2.
- `trace stats` showed two zstd-spilled sessions, two spill index entries, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18211/raw?case=bench' --iterations 1000 --warmup 10` completed with `rules=2`, `indexed_rules=2`, and `matched_actions=1010`.

Optimization from observation:

- Added `mock.raw(value)` as a `mock`-family first-match action.
- Raw mock values parse `HTTP/version status reason`, headers, blank line, and body after template/value resolution.
- Trace now records mock response headers for both raw and regular mock responses, including generated `Content-Length`.
- Preserved existing regular `mock(<file>)` missing-file behavior by treating unreadable regular mock values as not matched rather than raising a runtime error.
- Added focused unit coverage for `mock.raw` parse/explain/family behavior and raw response parsing.

## Loop 62

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18212`.
- rsproxy foreground daemon on `127.0.0.1:18929` with control API `127.0.0.1:18947`, storage `/tmp/rsproxy-dogfood62`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18212/skip-header skip(res.header) res.header(x-skipped: no) cache(62) when url(*skip-header*)`
  - `127.0.0.1:18212/all skip() when url(*all*)`
  - `127.0.0.1:18212 res.header(x-later: yes) cache(9)`

Observed:

- `rules check` accepted `skip(res.header)` and `skip()`.
- API-backed `rules test 'http://127.0.0.1:18212/skip-header'` rendered only `skip(res.header)` and `cache(max-age=62)`, proving the same-line `res.header` and later fallback `res.header` were skipped while cache remained executable.
- API-backed `rules test 'http://127.0.0.1:18212/all'` rendered only `skip()`, proving all later actions were suppressed.
- API-backed `rules test 'http://127.0.0.1:18212/plain'` rendered the fallback `res.header(x-later: yes)` and `cache(max-age=9)`.
- Curl through rsproxy confirmed proxy behavior:
  - `/skip-header` returned the origin body with `Cache-Control: max-age=62` and without `X-Skipped` or `X-Later`.
  - `/all` returned the origin response without injected headers or cache control.
  - `/plain` returned `X-Later: yes` and `Cache-Control: max-age=9`.
- `trace get 1/2/3` recorded the expected matched raw rules, response headers, and flags: `/skip-header` and `/plain` had `cache`, while `/all` had no flags.
- `trace stats` showed three zstd-spilled sessions, three spill index entries, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18212/skip-header' --iterations 1000 --warmup 10` completed with `rules=3`, `indexed_rules=3`, and `matched_actions=2020`.

Optimization from observation:

- Added executable `skip(family...)` and `skip()` control semantics to the action resolver.
- Retained the `skip` action itself in explain/trace so users can see which control rule caused later actions to disappear.
- `skip()` plus `skip(all)` and `skip(*)` suppress all subsequent actions.
- Family names are normalized case-insensitively, with `_` and `-` treated as `.`, and prefix matching lets a skipped family such as `res.body` cover subfamilies.
- Added focused unit coverage for named-family suppression, all-action suppression, and explain output.

## Loop 63

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18213`; `/response-tag` returns `201`, `/response-hide` returns `204`, and other paths return `200`.
- rsproxy foreground daemon on `127.0.0.1:18931` with control API `127.0.0.1:18949`, storage `/tmp/rsproxy-dogfood63`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18213/visible tag(api:${path}) res.header(x-tagged: yes) cache(13)`
  - `127.0.0.1:18213/hidden hide res.header(x-hidden: yes) when url(*hidden*)`
  - `127.0.0.1:18213/response-tag tag(done:${path}) res.header(x-response-tag: yes) when status(201)`
  - `127.0.0.1:18213/response-hide hide res.header(x-response-hidden: yes) when status(204)`

Observed:

- `rules check` accepted `tag(...)`, `hide`, and response-phase `when status(...)` control rules.
- API-backed `rules test 'http://127.0.0.1:18213/visible'` rendered `tag(api:/visible)`, `res.header(x-tagged: yes)`, and `cache(max-age=13)`.
- API-backed `rules test 'http://127.0.0.1:18213/hidden'` rendered `hide` and `res.header(x-hidden: yes)`.
- API-backed request-only `rules test` for `/response-tag` and `/response-hide` rendered `no matched actions`, as expected before response status is known.
- Curl through rsproxy confirmed proxy behavior:
  - `/visible` returned `X-Tagged: yes`, `Cache-Control: max-age=13`, and the origin body.
  - `/hidden` returned `X-Hidden: yes` and the origin body, proving `hide` suppresses trace only and does not skip response mutation.
  - `/response-tag` returned `201 Created` with `X-Response-Tag: yes`.
  - `/response-hide` returned `204 No Content` with `X-Response-Hidden: yes`.
- `trace ls` showed only two visible sessions: `/visible` and `/response-tag`.
- `trace stats` showed two zstd-spilled sessions, `next_id=3`, two spill index entries, and zero corrupt records, proving hidden request-phase and response-phase sessions did not consume trace IDs or spill rows.
- `trace get 1` recorded `flags:["tag:api:/visible","cache"]`.
- `trace get 2` recorded `flags:["tag:done:/response-tag"]` and the response-phase matched rule.
- `rules bench --url 'http://127.0.0.1:18213/visible' --iterations 1000 --warmup 10` completed with `rules=4`, `indexed_rules=4`, and `matched_actions=3030`.
- `rules bench --url 'http://127.0.0.1:18213/hidden' --iterations 1000 --warmup 10` completed with `matched_actions=2020`.

Optimization from observation:

- Added executable `hide` trace suppression for request-phase and response-phase rules.
- Added executable `tag(name)` trace flags as `tag:<rendered>`, with template rendering and duplicate suppression.
- Carried response-phase resolved actions back to session finalization so `hide` and `tag` work with `when status(...)` and `when res.header(...)`.
- Kept response mutation behavior independent from trace visibility: hidden sessions still execute other matched actions and return the rewritten response.
- Added focused unit coverage for `hide`/`tag` explain output, tag rendering, duplicate tag suppression, and hide visibility detection.

## Loop 64

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18214`, returning `X-Origin-Hit: yes` and `origin path=...`.
- HTTP upstream proxy fixture on `127.0.0.1:18215`, returning `X-Upstream-Proxy: yes` and `upstream proxy target=...` without forwarding to origin.
- rsproxy foreground daemon on `127.0.0.1:18932` with control API `127.0.0.1:18950`, storage `/tmp/rsproxy-dogfood64`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18214/direct direct res.header(x-direct-rule: yes)`
  - `127.0.0.1:18214 upstream(proxy://127.0.0.1:18215) res.header(x-upstream-rule: yes) cache(14)`

Observed:

- `rules check` accepted `direct` with the global `upstream(...)` fallback.
- API-backed `rules test 'http://127.0.0.1:18214/direct'` rendered `direct`, `res.header(x-direct-rule: yes)`, the global `upstream(proxy://127.0.0.1:18215)`, `res.header(x-upstream-rule: yes)`, and `cache(max-age=14)`.
- API-backed `rules test 'http://127.0.0.1:18214/via-upstream'` rendered only the global upstream/header/cache actions.
- Curl through rsproxy confirmed proxy behavior:
  - `/direct` returned `X-Origin-Hit: yes`, body `origin path=/direct`, `X-Direct-Rule: yes`, `X-Upstream-Rule: yes`, and `Cache-Control: max-age=14`, with no `X-Upstream-Proxy`.
  - `/via-upstream` returned `X-Upstream-Proxy: yes`, body `upstream proxy target=http://127.0.0.1:18214/via-upstream`, `X-Upstream-Rule: yes`, and `Cache-Control: max-age=14`, with no `X-Direct-Rule`.
- `trace get 1` recorded `/direct` with upstream `127.0.0.1:18214` and both matched rules, proving route selection honored `direct` even while the global upstream action remained visible.
- `trace get 2` recorded `/via-upstream` with upstream `proxy://127.0.0.1:18215` and only the global upstream rule.
- `trace stats` showed two zstd-spilled sessions, two spill index entries, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18214/direct' --iterations 1000 --warmup 10` completed with `rules=2`, `indexed_rules=2`, and `matched_actions=5050`.
- `rules bench --url 'http://127.0.0.1:18214/via-upstream' --iterations 1000 --warmup 10` completed with `matched_actions=3030`.

Optimization from observation:

- Added executable `direct` routing semantics to the HTTP upstream route planner.
- `direct` now overrides any matched `upstream(...)` action while preserving `host(...)` target rewriting before the route is selected.
- This supports the common shape of a broad upstream proxy rule plus narrower direct exceptions without requiring `skip(upstream)`.
- Added focused unit coverage for direct overriding earlier and same-line upstream actions.

## Loop 65

Runtime setup:

- Plain HTTP POST origin on `127.0.0.1:18216`, reading the request body and returning `X-Origin-Body-Len`, `request-len=...`, and a 138-byte text body.
- rsproxy foreground daemon on `127.0.0.1:18933` with control API `127.0.0.1:18951`, storage `/tmp/rsproxy-dogfood65`, `--trace-filter headers-only`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rule:
  - `127.0.0.1:18216 res.header(x-filter-mode: headers-only) cache(65)`

Observed:

- `rules check` accepted the rule and API-backed `rules test` rendered `res.header(x-filter-mode: headers-only)` plus `cache(max-age=65)`.
- Curl through rsproxy with POST body `PAYLOAD-1234567890` returned the complete origin response body, `X-Origin-Body-Len: 18`, `X-Filter-Mode: headers-only`, and `Cache-Control: max-age=65`, proving the trace filter does not affect forwarding or response mutation.
- `trace get 1` recorded `request_bytes:18` and `response_bytes:138`, while `req_body_head` and `res_body_head` were empty strings.
- `trace get 1` still recorded full request/response headers, matched rule, status, upstream, and cache flag.
- `trace stats` showed one zstd-spilled session, one spill index entry, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18216/headers-only' -X POST --body 'PAYLOAD-1234567890' --iterations 1000 --warmup 10` completed with `rules=1`, `indexed_rules=1`, and `matched_actions=2020`.

Optimization from observation:

- Added `--trace-filter headers-only` as the design-facing CLI for header-only capture.
- Kept `--no-trace-body` as a compatibility alias and reused the existing `trace_body_limit=0` capture path.
- Added `--trace-filter full` as an explicit no-op value so scripts can opt into the full-capture default.
- Unknown trace filters now fail fast instead of being silently ignored.
- Added focused unit coverage for headers-only, full/no-op, and unsupported filter values.

## Loop 66

Runtime setup:

- Plain HTTP origin on `127.0.0.1:18217`; `/image` returns a 96-byte `image/png` body, and `/text` returns a 103-byte `text/plain` body.
- rsproxy foreground daemon on `127.0.0.1:18934` with control API `127.0.0.1:18952`, storage `/tmp/rsproxy-dogfood66`, `--trace-body-limit 32`, `--trace-filter media`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rule:
  - `127.0.0.1:18217 res.header(x-media-filter: yes) cache(66)`

Observed:

- API-backed `rules test` for both `/image` and `/text` rendered `res.header(x-media-filter: yes)` plus `cache(max-age=66)`.
- Curl through rsproxy for `/image` returned `Content-Type: image/png`, `Content-Length: 96`, `X-Media-Filter: yes`, and wrote a 96-byte output file, proving media filtering does not affect forwarding.
- Curl through rsproxy for `/text` returned the complete text body, `X-Media-Filter: yes`, and `Cache-Control: max-age=66`.
- `trace get 1` for `/image` recorded `response_bytes:96`, full response headers, and an empty `res_body_head`.
- `trace get 2` for `/text` recorded `response_bytes:103` and `res_body_head:"text-preview-captured-TTTTTTTTTT"`, proving non-media responses still respect `--trace-body-limit 32`.
- `trace stats` showed two zstd-spilled sessions, two spill index entries, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18217/image' --iterations 1000 --warmup 10` completed with `rules=1`, `indexed_rules=1`, and `matched_actions=2020`.

Optimization from observation:

- Added media body preview exclusion for trace capture based on final request/response `Content-Type`.
- Media filtering currently covers `image/*`, `audio/*`, `video/*`, `font/*`, `application/font-*`, `application/x-font-*`, and `application/vnd.ms-fontobject`.
- Media filtering is enabled by default, can be explicitly selected with `--trace-filter media`, and can be disabled with `--trace-filter full`.
- Kept byte counts, headers, matched rules, flags, and spill records intact when body preview is excluded.
- Added focused unit coverage for media MIME detection, text passthrough capture, and `full` disabling media exclusion.

## Loop 67

Runtime setup:

- No origin was started on `127.0.0.1:18218`; this intentionally verified that file mocks short-circuit before upstream connection.
- Mock files under `/tmp/rsproxy-dogfood67/mocks`:
  - `fallback.json` with body `{"ok":true,"source":"fallback"}`
  - `page.html` with body `<html><body>mock page</body></html>`
- rsproxy foreground daemon on `127.0.0.1:18935` with control API `127.0.0.1:18953`, storage `/tmp/rsproxy-dogfood67`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rules:
  - `127.0.0.1:18218/json mock(<mocks/missing.json|mocks/fallback.json>)`
  - `127.0.0.1:18218/html mock(<mocks/page.html>)`

Observed:

- `rules check` accepted file mock candidates separated by `|`.
- API-backed `rules test 'http://127.0.0.1:18218/json'` rendered `mock(<mocks/missing.json|mocks/fallback.json>)`.
- API-backed `rules test 'http://127.0.0.1:18218/html'` rendered `mock(<mocks/page.html>)`.
- Curl through rsproxy for `/json` returned `Content-Type: application/json`, `Content-Length: 32`, and body `{"ok":true,"source":"fallback"}`, proving the missing first candidate was skipped and the fallback file was used.
- Curl through rsproxy for `/html` returned `Content-Type: text/html; charset=utf-8`, `Content-Length: 36`, and the HTML body.
- `trace get 1` and `trace get 2` recorded `upstream:null`, `flags:["mock"]`, inferred response headers, response body previews, and zero errors.
- `trace stats` showed two zstd-spilled sessions, two spill index entries, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18218/json' --iterations 1000 --warmup 10` completed with `rules=2`, `indexed_rules=2`, and `matched_actions=1010`.

Optimization from observation:

- Added regular file mock Content-Type inference from the final matched file extension.
- Added `|`-separated file candidate fallback for `mock(<...>)`, tried left-to-right after template rendering.
- Kept inline and `@key` regular mock responses as `text/plain; charset=utf-8`.
- Kept `mock.raw(...)` unchanged because it already carries its own status line and headers.
- Added focused unit coverage for missing-file candidate fallback and JSON Content-Type inference.

## Loop 68

Runtime setup:

- No origin was started on `127.0.0.1:18219`; this intentionally verified that directory mocks short-circuit before upstream connection.
- Mock files under `/tmp/rsproxy-dogfood68/mocks`:
  - `api/item.json` with body `{"dir":true,"name":"item"}`
  - `docs/index.html` with body `<html><body>docs index</body></html>`
- rsproxy foreground daemon on `127.0.0.1:18936` with control API `127.0.0.1:18954`, storage `/tmp/rsproxy-dogfood68`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rule: `127.0.0.1:18219 mock(<mocks>)`.

Observed:

- `lsof` confirmed no origin listener on `127.0.0.1:18219`.
- `rules check` accepted the directory mock rule.
- API-backed `rules test 'http://127.0.0.1:18219/api/item.json'` and `rules test 'http://127.0.0.1:18219/docs/'` rendered `mock(<mocks>)`.
- Curl through rsproxy for `/api/item.json` returned `Content-Type: application/json`, `Content-Length: 27`, and body `{"dir":true,"name":"item"}`.
- Curl through rsproxy for `/docs/` returned `Content-Type: text/html; charset=utf-8`, `Content-Length: 37`, and the HTML body.
- `trace get 1` and `trace get 2` recorded `upstream:null`, `flags:["mock"]`, inferred response headers, response body previews, and zero errors.
- `trace stats` showed two zstd-spilled sessions, two spill index entries, and zero corrupt records.
- `rules bench --url 'http://127.0.0.1:18219/api/item.json' --iterations 1000 --warmup 10` completed with `rules=1`, `indexed_rules=1`, and `matched_actions=1010`.

Optimization from observation:

- Added directory candidate support for `mock(<dir>)`: when a file mock candidate is a directory, rsproxy appends the sanitized request URL path.
- Mapped `/` and paths ending in `/` to `index.html` for directory mocks.
- Inferred `Content-Type` from the final resolved path, so directory mocks use the matched leaf file extension.
- Kept storage-relative lookup first, then raw path fallback, matching regular file candidate behavior.
- Added focused unit coverage for request-path directory mock resolution and inferred JSON `Content-Type`.

## Loop 69

Runtime setup:

- Temporary CA/storage: `/tmp/rsproxy-dogfood69`.
- Generated root CA with `rsproxy ca init --storage /tmp/rsproxy-dogfood69 --name rsproxy-dogfood69`.
- Issued a `127.0.0.1` leaf certificate with `rsproxy ca issue 127.0.0.1 --storage /tmp/rsproxy-dogfood69`.
- HTTPS origin on `127.0.0.1:18220` used the generated leaf chain and returned `X-Origin-Tls: dogfood69`.
- rsproxy foreground daemon on `127.0.0.1:18937` with control API `127.0.0.1:18955`, storage `/tmp/rsproxy-dogfood69`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rule: `127.0.0.1:18220 res.header(x-rsproxy-loop: 69) tag(tls-trace)`.

Observed:

- `rules check` accepted the HTTPS MITM response rule and `rules test 'https://127.0.0.1:18220/tls?loop=69'` rendered `res.header(x-rsproxy-loop: 69)` plus `tag(tls-trace)`.
- Curl through rsproxy with `--cacert /tmp/rsproxy-dogfood69/ca/rsproxy-root-ca.pem` returned the origin body and both `X-Origin-Tls: dogfood69` and `X-Rsproxy-Loop: 69`, proving CONNECT MITM, upstream TLS validation, and response mutation worked together.
- Initial trace observation showed `client_mitm_tls.handshake_ms` larger than `duration_ms`, because the inner HTTP session was created after the client TLS handshake.
- After optimizing the timing origin, `trace get 1` recorded `duration_ms:33`, `flags:["tag:tls-trace","mitm"]`, and two TLS records:
  - `client_mitm_tls` for `127.0.0.1`, `handshake_ms:26`, `peer_certificates:0`, `protocol:"TLSv1_3"`.
  - `upstream_tls` for `127.0.0.1`, `handshake_ms:2`, `peer_certificates:2`, `protocol:"TLSv1_3"`.
- `trace export` and `/api/sessions/spill.ndjson` both preserved the `tls` array from the zstd spill segment.
- `trace stats` showed one zstd-spilled session, one spill index entry, and zero corrupt records.
- `rules bench --url 'https://127.0.0.1:18220/tls?loop=69b' --iterations 1000 --warmup 10` completed with `rules=1`, `indexed_rules=1`, `matched_actions=2020`, and p99 under 8µs.

Optimization from observation:

- Added `TlsRecord` to trace sessions and emitted it from CLI detail/export plus segmented spill NDJSON.
- Recorded client-side MITM TLS handshakes with `phase:"client_mitm_tls"` after explicitly completing the rustls server handshake before reading the inner HTTP request.
- Recorded upstream TLS handshakes with `phase:"upstream_tls"` from every TLS-wrapped upstream hop, including direct HTTPS and HTTPS proxy-chain hops.
- Included peer certificate chain count, negotiated TLS protocol, and ALPN when rustls exposes them.
- Shifted MITM inner HTTP session `started_ms` to the original CONNECT session start, so `duration_ms` includes client TLS setup instead of only post-handshake HTTP forwarding.

## Loop 70

Runtime setup:

- Temporary CA/storage: `/tmp/rsproxy-dogfood70`.
- Generated root CA with `rsproxy ca init --storage /tmp/rsproxy-dogfood70 --name rsproxy-dogfood70`.
- Issued a `127.0.0.1` leaf certificate with `rsproxy ca issue 127.0.0.1 --storage /tmp/rsproxy-dogfood70`.
- HTTPS origin on `127.0.0.1:18221` used the generated leaf chain and explicitly advertised ALPN `http/1.1`; it echoed the selected ALPN in `X-Origin-Alpn` and the body.
- rsproxy foreground daemon on `127.0.0.1:18938` with control API `127.0.0.1:18956`, storage `/tmp/rsproxy-dogfood70`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rule: `127.0.0.1:18221 res.header(x-rsproxy-alpn: dogfood70) tag(alpn:${path})`.

Observed:

- `rules check` accepted the rule and `rules test 'https://127.0.0.1:18221/alpn?loop=70'` rendered `res.header(x-rsproxy-alpn: dogfood70)` and `tag(alpn:/alpn)`.
- Curl through rsproxy with `--http1.1` and the dogfood CA returned `X-Origin-Alpn: http/1.1`, injected `X-Rsproxy-Alpn: dogfood70`, and body `alpn-origin path=/alpn?loop=70 alpn=http/1.1`.
- `trace get 1` recorded `flags:["tag:alpn:/alpn","mitm"]` and both TLS records with negotiated ALPN:
  - `client_mitm_tls`: `protocol:"TLSv1_3"`, `alpn:"http/1.1"`.
  - `upstream_tls`: `protocol:"TLSv1_3"`, `alpn:"http/1.1"`.
- `trace export` and `/api/sessions/spill.ndjson` preserved `alpn:"http/1.1"` from the zstd spill segment.
- `trace stats` showed one zstd-spilled session, one spill index entry, and zero corrupt records.
- `rules bench --url 'https://127.0.0.1:18221/alpn?loop=70' --iterations 1000 --warmup 10` completed with `rules=1`, `indexed_rules=1`, `matched_actions=2020`, and p99 under 9µs.

Optimization from observation:

- Added a shared `http/1.1` ALPN helper and applied it to both MITM `ServerConfig` and upstream `ClientConfig`.
- Kept ALPN conservative: rsproxy advertises only `http/1.1` until the HTTP/2 bridge is implemented, so clients cannot negotiate h2 into an HTTP/1.1-only handler.
- Added focused unit coverage proving both TLS configs carry `http/1.1` ALPN.

## Loop 71

Runtime setup:

- Temporary CA/storage: `/tmp/rsproxy-dogfood71`.
- Generated root CA with `rsproxy ca init --storage /tmp/rsproxy-dogfood71 --name rsproxy-dogfood71`.
- Issued a `127.0.0.1` leaf certificate with `rsproxy ca issue 127.0.0.1 --storage /tmp/rsproxy-dogfood71`.
- WSS origin on `127.0.0.1:18222` used the generated leaf chain, advertised ALPN `http/1.1`, returned `X-Origin-Wss: dogfood71`, sent WebSocket text frame `push-first-wss` immediately after the `101 Switching Protocols` response, then echoed client text frames.
- rsproxy foreground daemon on `127.0.0.1:18939` with control API `127.0.0.1:18957`, storage `/tmp/rsproxy-dogfood71`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rule: `127.0.0.1:18222 res.header(x-rsproxy-wss: dogfood71) tag(wss:${path})`.

Observed:

- `rules check` accepted the rule and `rules test 'https://127.0.0.1:18222/ws?loop=71'` rendered `res.header(x-rsproxy-wss: dogfood71)` and `tag(wss:/ws)`.
- Curl through rsproxy with `--cacert /tmp/rsproxy-dogfood71/ca/rsproxy-root-ca.pem` returned the proxied `101 Switching Protocols` response, `X-Origin-Wss: dogfood71`, injected `X-Rsproxy-Wss: dogfood71`, and the raw `push-first-wss` frame bytes before timing out as expected for an upgraded connection.
- The first strict Python TLS client failed with `CertificateUnknown`; observation showed OpenSSL 3.6 rejects the generated root CA without key usage suitable for CA signing.
- After adding root CA `KeyCertSign`, `CrlSign`, and `DigitalSignature`, a strict Python TLS WebSocket client using the dogfood CA completed TLS verification with `TLSv1.3` / `http/1.1`.
- The verified Python client received `push-first-wss` before sending any WebSocket message, then sent `hello-verified-wss` and received `echo:hello-verified-wss`.
- `trace get 1` recorded `kind:"websocket"`, `status:101`, `flags:["tag:wss:/ws","mitm","websocket"]`, injected response headers, TLS records with `alpn:"http/1.1"`, and frame order:
  - `s2c` text `push-first-wss`
  - `c2s` text `hello-verified-wss`
  - `s2c` text `echo:hello-verified-wss`
  - `c2s` close
- `trace export` and `/api/sessions/spill.ndjson` preserved the same WSS frame metadata and TLS records from the zstd spill segment.
- `trace stats` showed one zstd-spilled session, one spill index entry, and zero corrupt records.
- `rules bench --url 'https://127.0.0.1:18222/ws?verified=1' --iterations 1000 --warmup 10` completed with `rules=1`, `indexed_rules=1`, `matched_actions=2020`, and p99 at 8µs.

Optimization from observation:

- Replaced the no-clone WebSocket fallback with a single-thread nonblocking bidirectional pump, so TLS/MITM WebSocket sessions no longer require the client to send the first frame.
- Added an incremental WebSocket frame decoder that waits for complete split frames before parsing and recording metadata.
- Preserved the existing threaded clone-based fast path for plain TCP WebSocket sessions.
- Added root CA key usages (`KeyCertSign`, `CrlSign`, `DigitalSignature`) so stricter OpenSSL clients can validate rsproxy-generated CA chains.
- Added focused unit coverage for split-frame WebSocket decoding and root CA PEM generation.

## Loop 72

Runtime setup:

- Temporary CA/storage: `/tmp/rsproxy-dogfood72`.
- Generated root CA with `rsproxy ca init --storage /tmp/rsproxy-dogfood72 --name rsproxy-dogfood72`.
- Issued a `127.0.0.1` leaf certificate with `rsproxy ca issue 127.0.0.1 --storage /tmp/rsproxy-dogfood72`.
- HTTPS origin on `127.0.0.1:18223` used the generated leaf chain, advertised ALPN `http/1.1`, and returned `X-Origin-Cache: dogfood72`.
- rsproxy foreground daemon on `127.0.0.1:18940` with control API `127.0.0.1:18958`, storage `/tmp/rsproxy-dogfood72`, `--mitm-cert-cache-capacity 2`, `--trace-segment-size 8kb`, `--trace-disk-budget 1mb`, and `--trace-spill-compression zstd:1`.
- Final default rule: `127.0.0.1:18223 res.header(x-rsproxy-cache: dogfood72) tag(cache:${path})`.

Observed:

- `rules check` accepted the rule and `rules test 'https://127.0.0.1:18223/cache?run=1'` rendered `res.header(x-rsproxy-cache: dogfood72)` and `tag(cache:/cache)`.
- Two consecutive curl requests through rsproxy with the dogfood CA returned the origin body plus injected `X-Rsproxy-Cache: dogfood72`.
- `trace get 1` recorded `flags:["mitm-cert-cache-miss","tag:cache:/cache","mitm"]`, `duration_ms:46`, and `client_mitm_tls.handshake_ms:28`.
- `trace get 2` recorded `flags:["mitm-cert-cache-hit","tag:cache:/cache","mitm"]`, `duration_ms:4`, and `client_mitm_tls.handshake_ms:2`, proving the in-memory `ServerConfig` cache was used on the second CONNECT.
- `trace export` and `/api/sessions/spill.ndjson` preserved both cache hit/miss flags, response bodies, and TLS ALPN records from the zstd spill segment.
- `trace stats` showed two zstd-spilled sessions, two spill index entries, and zero corrupt records.
- `rsproxy ca status --storage /tmp/rsproxy-dogfood72` reported `leaf_cached=1`, confirming disk leaf reuse remains intact alongside the new memory cache.
- `rules bench --url 'https://127.0.0.1:18223/cache?run=2' --iterations 1000 --warmup 10` completed with `rules=1`, `indexed_rules=1`, `matched_actions=2020`, and p99 under 8µs.

Optimization from observation:

- Added a bounded MITM `ServerConfig` LRU cache shared across proxy sessions; default capacity is 1024.
- Added `--mitm-cert-cache-capacity N`; `0` disables the memory cache while retaining disk leaf certificate reuse.
- Added trace flags `mitm-cert-cache-hit` and `mitm-cert-cache-miss` so cache behavior is visible in detail/export/spill.
- Kept cache flags scoped to inner HTTP/WSS sessions without leaking the outer CONNECT `tunnel` flag.
- Added focused unit coverage for LRU eviction, zero-capacity behavior, and CLI option parsing.

## Loop 73

Runtime setup:

- Temporary CA/storage: `/tmp/rsproxy-dogfood73`.
- Generated root CA with `rsproxy ca init --storage /tmp/rsproxy-dogfood73 --name rsproxy-dogfood73`.
- Issued a `127.0.0.1` origin leaf certificate with `rsproxy ca issue 127.0.0.1 --storage /tmp/rsproxy-dogfood73`.
- Generated a separate client certificate/key with `clientAuth` EKU, signed by the dogfood root CA.
- HTTPS origin on `127.0.0.1:18224` required client certificates and returned `X-Origin-Mtls: yes` only when a verified client cert was presented.
- rsproxy foreground daemon on `127.0.0.1:18941` with control API `127.0.0.1:18942`, storage `/tmp/rsproxy-dogfood73`, `--trace-body-limit 4096`, and `--mitm-cert-cache-capacity 16`.
- Final default rule: `127.0.0.1:18224 tls(client-cert=<certs/client.pem>, client-key=<certs/client-key.pem>) res.header(x-rsproxy-mtls: dogfood73) tag(mtls:${path})`.

Observed:

- `rules check` accepted the rule and `rules test 'https://127.0.0.1:18224/mtls?via=test'` rendered `tls(client-cert=certs/client.pem, client-key=certs/client-key.pem)`, `res.header(x-rsproxy-mtls: dogfood73)`, and `tag(mtls:/mtls)`.
- Direct curl to the origin without a client cert failed during TLS read, proving the origin required mTLS.
- Direct curl with `--cert /tmp/rsproxy-dogfood73/certs/client.pem --key /tmp/rsproxy-dogfood73/certs/client-key.pem` returned `X-Origin-Mtls: yes` and `client_cert=True`.
- Curl through rsproxy with only the dogfood CA succeeded, returned `X-Origin-Mtls: yes`, injected `X-Rsproxy-Mtls: dogfood73`, and body `mtls-origin path=/mtls?via=rsproxy client_cert=True`, proving rsproxy supplied the upstream client cert.
- `trace get 1` recorded `flags:["mitm-cert-cache-miss","tag:mtls:/mtls","mitm","upstream-mtls"]`, the matched `tls(...)` rule, and both TLS records:
  - `client_mitm_tls`: `protocol:"TLSv1_3"`, `alpn:"http/1.1"`.
  - `upstream_tls`: `protocol:"TLSv1_3"`, `peer_certificates:2`.
- `trace export` preserved the `upstream-mtls` flag, injected response header, matched rule, response body preview, and TLS records.
- `rules bench --url 'https://127.0.0.1:18224/mtls?via=bench' --iterations 1000` completed with `rules=1`, `indexed_rules=1`, `matched_actions=3300`, `p50_ns=7917`, and `p99_ns=8834`.

Optimization from observation:

- Added `tls(client-cert=<path>, client-key=<path>)` to the rules DSL as a single-action `tls` family with template rendering and storage-relative path resolution.
- Wired matched TLS identity into rustls `ClientConfig::with_client_auth_cert` for direct HTTPS origin connections and single-hop SOCKS5 origin TLS.
- Kept client certificates away from HTTPS proxy TLS handshakes and proxy-chain hops until origin-over-proxy mTLS is modeled explicitly.
- Added `upstream-mtls` trace flag so detail/export/spill consumers can identify sessions where a client certificate was configured for the upstream origin.
- Added focused unit coverage for DSL parsing/explain/rejection, mTLS route-boundary flagging, and storage-relative TLS file resolution.

## Loop 74

Runtime setup:

- Temporary CA/storage: `/tmp/rsproxy-dogfood74`.
- Generated root CA with `rsproxy ca init --storage /tmp/rsproxy-dogfood74 --name rsproxy-dogfood74`.
- Issued a `127.0.0.1` origin leaf certificate with `rsproxy ca issue 127.0.0.1 --storage /tmp/rsproxy-dogfood74`.
- Generated a separate `clientAuth` client certificate/key signed by the dogfood root CA.
- HTTPS origin on `127.0.0.1:18225` required client certificates and returned `X-Origin-Proxy-Mtls: yes` only when a verified client cert was presented.
- HTTP upstream proxy on `127.0.0.1:18226` accepted only `CONNECT` requests and returned `400 Bad Request` for non-CONNECT absolute-form traffic; it logged each first request line to `/tmp/rsproxy-dogfood74/upstream-proxy.log`.
- rsproxy foreground daemon on `127.0.0.1:18943` with control API `127.0.0.1:18944`, storage `/tmp/rsproxy-dogfood74`, `--trace-body-limit 4096`, and `--mitm-cert-cache-capacity 16`.
- Final default rule: `127.0.0.1:18225 upstream(proxy://127.0.0.1:18226) tls(client-cert=<certs/client.pem>, client-key=<certs/client-key.pem>) res.header(x-rsproxy-proxy-mtls: dogfood74) tag(proxy-mtls:${path})`.

Observed:

- `rules check` accepted the rule and `rules test 'https://127.0.0.1:18225/proxy-mtls?via=test'` rendered `upstream(proxy://127.0.0.1:18226)`, `tls(client-cert=certs/client.pem, client-key=certs/client-key.pem)`, `res.header(x-rsproxy-proxy-mtls: dogfood74)`, and `tag(proxy-mtls:/proxy-mtls)`.
- Direct curl through the upstream HTTP proxy without a client cert failed after `HTTP/1.1 200 Connection Established`, proving the origin still required mTLS behind the proxy.
- Direct curl through the upstream HTTP proxy with the client cert succeeded and returned `X-Origin-Proxy-Mtls: yes`; the upstream proxy log contained only `CONNECT 127.0.0.1:18225 HTTP/1.1`.
- Curl through rsproxy with only the dogfood CA succeeded, returned `X-Origin-Proxy-Mtls: yes`, injected `X-Rsproxy-Proxy-Mtls: dogfood74`, and body `proxy-mtls-origin path=/proxy-mtls?via=rsproxy client_cert=True`, proving rsproxy created the upstream CONNECT tunnel and then supplied the origin client cert inside it.
- The upstream proxy log after the rsproxy request added another `CONNECT 127.0.0.1:18225 HTTP/1.1`, proving rsproxy no longer used absolute-form `GET https://...` for HTTPS origin traffic through HTTP upstream proxy.
- `trace get 1` recorded `upstream:"proxy://127.0.0.1:18226"`, `flags:["mitm-cert-cache-miss","tag:proxy-mtls:/proxy-mtls","mitm","upstream-mtls"]`, injected response headers, and both TLS records:
  - `client_mitm_tls`: `protocol:"TLSv1_3"`, `alpn:"http/1.1"`.
  - `upstream_tls`: `host:"127.0.0.1"`, `protocol:"TLSv1_3"`, `peer_certificates:2`.
- `trace export` preserved the upstream proxy label, `upstream-mtls` flag, matched rule, injected response header, response body preview, and TLS records.
- `rules bench --url 'https://127.0.0.1:18225/proxy-mtls?via=bench' --iterations 1000` completed with `rules=1`, `indexed_rules=1`, `matched_actions=4400`, `p50_ns=9291`, and `p99_ns=10334`.

Optimization from observation:

- Changed HTTPS origin forwarding through `upstream(proxy://...)`, `upstream(https-proxy://...)`, and proxy chains to establish a CONNECT tunnel to the origin first, then perform origin TLS inside that tunnel.
- Kept HTTP origin forwarding through HTTP(S) upstream proxies on absolute-form request targets.
- Split route helpers for proxy TLS host versus origin TLS host so client certificates are applied to the origin TLS handshake, not to HTTPS proxy TLS.
- Reused the CONNECT tunnel code path with TLS trace collection so HTTPS-proxy hops still record proxy TLS handshakes before origin TLS.
- Added focused unit coverage for HTTPS-origin-via-HTTP-proxy routing, request-target selection, and mTLS flagging.

## Loop 75

Runtime setup:

- Temporary storage: `/tmp/rsproxy-dogfood75`.
- HTTP origin on `127.0.0.1:18227` returned a normal response for `/ok` and extra `X-Origin-Extra-*` headers for `/many-response-headers`.
- rsproxy foreground daemon on `127.0.0.1:18945` with control API `127.0.0.1:18946`, storage `/tmp/rsproxy-dogfood75`, `--max-header-count 5`, and `--trace-body-limit 4096`.
- Final default rule: `127.0.0.1:18227 res.header(x-rsproxy-header-limit: dogfood75) tag(header-limit:${path})`.

Observed:

- `rules check` accepted the rule and `rules test 'http://127.0.0.1:18227/ok?via=test'` rendered `res.header(x-rsproxy-header-limit: dogfood75)` and `tag(header-limit:/ok)`.
- Normal curl through rsproxy returned `HTTP/1.1 200 OK`, origin body `header-limit-origin path=/ok?via=rsproxy`, and injected `X-Rsproxy-Header-Limit: dogfood75`.
- Curl through rsproxy with three extra request headers exceeded the configured count and returned `HTTP/1.1 431 Request Header Fields Too Large` with body `header count limit exceeded (limit 5)`.
- Curl through rsproxy to `/many-response-headers` exceeded the upstream response header count and returned `HTTP/1.1 502 Bad Gateway` with body `upstream error: stage=response_head: header count limit exceeded (limit 5)`.
- `trace ls` showed only the normal proxied request and the upstream response-head failure; the request-head 431 was rejected before rule resolution and did not create a session.
- `trace get 1` recorded status `200`, `flags:["tag:header-limit:/ok"]`, response body preview, and injected response header.
- `trace get 2` recorded status `502`, `flags:["tag:header-limit:/many-response-headers"]`, empty response headers/body, and `error:"stage=response_head: header count limit exceeded (limit 5)"`.
- `trace export` preserved both the successful injected response header and the failed response-head error.
- `rules bench --url 'http://127.0.0.1:18227/ok?via=bench' --iterations 1000` completed with `rules=1`, `indexed_rules=1`, `matched_actions=2200`, `p50_ns=6875`, and `p99_ns=7542`.

Optimization from observation:

- Added `max_header_count` to runtime config with default 256 and CLI option `--max-header-count N`.
- Enforced header count in the shared HTTP/1 parser so request heads, upstream response heads, control API requests, and replay response reads use the same limit semantics.
- Kept request-head count failures as direct 431 responses before rule resolution; upstream response-head count failures use the existing staged upstream error path and remain visible in trace detail/export.
- Added focused unit coverage for request and response header count limits plus CLI option parsing.

## Loop 76

Runtime setup:

- Temporary storage: `/tmp/rsproxy-dogfood76`.
- HTTP origin on `127.0.0.1:18228` returned the request path plus `X-Origin-Auth: dogfood76`.
- rsproxy foreground daemon on `127.0.0.1:18947` with control API `127.0.0.1:18948`, storage `/tmp/rsproxy-dogfood76`, `--proxy-auth user:pass`, and `--trace-body-limit 4096`.
- Default rule: `127.0.0.1:18228 res.header(x-rsproxy-proxy-auth: dogfood76) tag(proxy-auth:${path})`.

Observed:

- `rules check`, `rules cat`, and API-backed `rules test 'http://127.0.0.1:18228/authorized?via=rules-test'` accepted and rendered the response-header and trace-tag actions.
- Ordinary HTTP curl without credentials and with `user:wrong` both returned `HTTP/1.1 407 Proxy Authentication Required`, `Proxy-Authenticate: Basic realm="rsproxy"`, and the explicit authentication-required body.
- Ordinary HTTP curl with `--proxy-user user:pass` reached the origin and returned `HTTP/1.1 200 OK`, the origin body, and injected `X-Rsproxy-Proxy-Auth: dogfood76`.
- A raw proxy header using lowercase `basic` and multiple spaces also authenticated successfully, proving scheme case-insensitivity and whitespace-tolerant parsing.
- Forced CONNECT curl without credentials failed the tunnel with response 407; the same request with `--proxy-user user:pass` received `200 Connection Established` and reached the HTTP origin through the passthrough tunnel.
- `trace ls` contained exactly the three authorized requests (two HTTP sessions and one tunnel session); rejected HTTP and CONNECT attempts did not create sessions.
- Initial `trace get` exposed `Proxy-Authorization: Basic dXNlcjpwYXNz` in both HTTP and CONNECT request headers even though forwarding removed the header. This made the proxy password recoverable from memory, spill, and export data.
- After optimization and a clean daemon restart, all three authorized trace details omitted `Proxy-Authorization`; a case-insensitive scan of the JSON export and spill directory found neither the header name nor its Base64 credential value.
- `rsproxy run --proxy-auth invalid-format` failed before binding with `--proxy-auth must use user:pass format`, and `rsproxy --help` now advertises the option.
- `rules bench --url 'http://127.0.0.1:18228/authorized?via=bench' --iterations 1000 --warmup 100` completed with `rules=1`, `indexed_rules=1`, `matched_actions=2200`, `p50_ns=6792`, and `p99_ns=8000`.

Optimization from observation:

- Parse Basic authorization as exactly two whitespace-delimited fields, compare the scheme case-insensitively, and reject missing, wrong, non-Basic, or extra-field credentials.
- Validate `--proxy-auth` as a non-empty `user:pass` pair during runtime configuration and expose it in CLI help.
- Strip `Proxy-Authorization` immediately after successful admission, before CONNECT/HTTP dispatch, rule resolution, trace capture, spill, export, or upstream forwarding. The same stripping applies when authentication is disabled but a client sends the proxy-only header unexpectedly.
- Added focused unit coverage for accepted/malformed authorization fields, pre-dispatch credential stripping, and CLI configuration validation.

## Loop 77

Runtime setup:

- Temporary storage and CA: `/tmp/rsproxy-dogfood77`.
- OpenSSL origin on `127.0.0.1:18229` used an rsproxy-issued ECDSA leaf certificate and accepted only TLS 1.2 with `ECDHE-ECDSA-AES128-GCM-SHA256`.
- rsproxy foreground daemon on `127.0.0.1:18949` with control API `127.0.0.1:18950`, the same storage, and `--trace-body-limit 8192`.
- Final default rule: `127.0.0.1:18229 tls(min=1.2, ciphers=ECDHE-ECDSA-AES128-GCM-SHA256) res.header(x-rsproxy-tls-policy: valid) tag(tls-policy:${path})`.

Observed:

- `rules check` and API-backed `rules test` accepted `min=1.2` plus the OpenSSL cipher alias and rendered the canonical IANA name `TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256`.
- `rules check` rejected TLS 1.1, an unsupported 3DES suite, and `min=1.3` paired only with a TLS 1.2 suite before runtime.
- Curl through rsproxy with the dogfood CA returned `200`, the origin's `New, TLSv1.2, Cipher is ECDHE-ECDSA-AES128-GCM-SHA256` evidence, and injected `X-Rsproxy-Tls-Policy: valid`.
- Successful `trace get 1` separated the two connections: client MITM TLS negotiated TLS 1.3/ChaCha20 with `http/1.1`, while `upstream_tls` negotiated TLS 1.2 and exactly `TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256`. Flags included `upstream-tls-policy`, `upstream-tls-min:1.2`, and `upstream-tls-ciphers:1`.
- Changing the rule to `min=1.3, ciphers=TLS_AES_128_GCM_SHA256` against the TLS-1.2-only origin returned 502 with `stage=tls: received fatal alert: ProtocolVersion`.
- Keeping `min=1.2` but allowing only `ECDHE-ECDSA-AES256-GCM-SHA384` returned 502 with `stage=tls: received fatal alert: HandshakeFailure`, proving the cipher allowlist was active.
- Initial negative traces contained only the successful client MITM TLS record; the failed origin TLS attempt was lost because `forward` returned its accumulated TLS records only on success.
- After optimization, a clean restart and rerun produced an `upstream_tls` record for each failure with handshake duration, null protocol/cipher/ALPN, and the corresponding structured error.
- `trace export` and `/api/sessions/spill.ndjson` preserved all three `upstream_tls` records, the successful negotiated cipher, and both failure classes.
- `rules bench --url 'https://127.0.0.1:18229/final?via=bench' --iterations 1000 --warmup 100` completed with `rules=1`, `indexed_rules=1`, `matched_actions=3300`, `p50_ns=7834`, and `p99_ns=16667` in the debug build.

Optimization from observation:

- Extended `TlsOp` so minimum version, ordered cipher allowlist, and optional paired mTLS certificate/key can be used independently or together; accepted aliases normalize to canonical IANA output.
- Build an origin-specific rustls aws-lc provider filtered in rule order and select TLS 1.3-only versions for `min=1.3`; do not apply origin policy or identity to HTTPS upstream-proxy handshakes.
- Added `upstream-tls-policy` trace flags and negotiated `cipher_suite` to successful TLS records.
- Changed forwarding to append TLS records directly into the session as each handshake completes, preserving partial state on errors; failed TLS records now include phase, host, elapsed time, and `error` in memory, spill, and export.
- Added parser validation and unit coverage for aliases, invalid/incompatible policy, route boundaries, provider filtering, failed records, and trace serialization.

## Loop 78

Runtime setup:

- Temporary storage and CA: `/tmp/rsproxy-dogfood78`.
- TLS origin on `127.0.0.1:18230` advertised only `http/1.1`, echoed its received HTTP version/method/path/body and `X-H2-Bridge`, and returned a chunked body with `X-Origin-Trailer: origin78`.
- rsproxy foreground daemon on `127.0.0.1:18951` with control API `127.0.0.1:18952`, the same storage, and `--trace-body-limit 8192`.
- Default rule: `127.0.0.1:18230 req.header(x-h2-bridge: dogfood78) res.header(x-rsproxy-h2: client-h2) res.trailer(x-rule-trailer: dogfood78) tag(h2:${path})`.

Observed:

- `rules check` and API-backed `rules test` accepted and rendered request-header, response-header, response-trailer, and trace-tag actions.
- Curl `--http2` through the HTTP proxy completed CONNECT and TLS, reported `ALPN: server accepted h2`, opened stream 1, and sent a POST body over HTTP/2.
- The successful response was `HTTP/2 200` with `X-Rsproxy-H2: client-h2`; the origin body proved rsproxy sent `HTTP/1.1`, preserved POST/path/body, and injected `X-H2-Bridge: dogfood78`.
- Curl received both the origin trailer and rule-added trailer as HTTP/2 trailing headers.
- Parallel curl without `--parallel-immediate` reused one CONNECT/TLS connection and opened streams 1, 3, and 5; trace sessions shared the same client socket and start timestamp while each retained its own URL, duration, rules, body, and upstream TLS record.
- Sending 251 custom headers exceeded the effective pseudo-header-inclusive count and returned `HTTP/2 431` with `header count limit exceeded (limit 256)`; the rejected stream did not create a trace session.
- `--http1.1` still negotiated the existing h1 path and returned the same body/trailers, proving ALPN fallback was preserved.
- Initial dogfood accidentally used a plaintext origin for an HTTPS URL. rsproxy correctly attempted origin TLS and returned a structured `upstream_tls` EOF failure; switching the origin to TLS produced the intended h2→h1-over-TLS proof.
- Initial successful trace exposed internal h1 `Transfer-Encoding: chunked`, while curl actually saw h2 `Content-Length` and trailers. After normalization, trace/export/spill omitted connection-specific headers and recorded final client-visible `Content-Length` plus both trailers.
- The first nonblocking adapter used a 1ms retry timer. It worked functionally but would wake every idle connection continuously; Unix was changed to readiness-driven Tokio `AsyncFd`, with the timer retained only as the non-Unix fallback.
- The first AsyncFd run panicked because the shared runtime enabled time but not IO. Enabling Tokio IO fixed the regression; final POST and multiplex reruns passed, and an idle negotiated h2 connection showed `0.0% CPU` for rsproxy in `top`.
- Final trace/export/spill contained four optimized h2 sessions with `h2-client`, client TLS `alpn:"h2"`, final response headers, and both trailers; no `Transfer-Encoding` remained. `rules bench` completed 1,000 iterations with four matched actions per request, `p50_ns=8167`, and `p99_ns=9167` in the debug build.

Optimization from observation:

- Added Hyper 1.x, Hyper-util, Tokio, and HTTP body dependencies plus a dedicated client-side h2 module backed by a shared multi-thread runtime.
- Advertise `h2` before `http/1.1` only on the MITM server side; upstream TLS remains h1-only until the independent upstream bridge is implemented.
- Convert pseudo headers/body to `RawRequest`, explicitly downgrade the upstream request line to HTTP/1.1, and run the existing rule/forward/trace pipeline in Tokio's blocking pool.
- Convert captured h1 responses back to h2 data/trailers, remove connection-specific headers, preserve repeated headers and trailers, and align trace headers with the final h2 response.
- Enforce h2 header size/count settings and reject CONNECT/request-trailer forms that are outside the current bridge boundary.
- Replaced Unix timer polling with `AsyncFd` readiness, kept a cross-platform fallback, and added focused tests for pseudo-header mapping, sensitive/header stripping, response trailers, ALPN configuration, upstream version translation, trace reuse, and h2 response normalization.

## Loop 79

Runtime setup:

- Temporary storage and CA: `/tmp/rsproxy-dogfood79`; rsproxy issued the TLS leaf used by both origins.
- `nghttpd` TLS origin on `127.0.0.1:18240` advertised h2, echoed POST uploads, and returned `grpc-status: 0` plus `grpc-message: dogfood79` trailers.
- Python TLS fallback origin on `127.0.0.1:18241` advertised only `http/1.1` and returned its received HTTP version/path.
- Main rsproxy daemon on `127.0.0.1:18961`, API `127.0.0.1:18962`; a second no-CA rsproxy on `127.0.0.1:18963` acted as a passthrough HTTP CONNECT hop.
- Direct-origin rule: `127.0.0.1:18240 req.header(x-rsproxy-upstream-h2: dogfood79) res.header(content-type: application/grpc) res.header(x-rsproxy-h2: upstream-h2) res.trailer(x-rule-trailer: dogfood79) tag(h2-upstream:${path})`; the proxy-hop variant prepended `upstream(proxy://127.0.0.1:18963)`.

Observed:

- CLI `status`, `rules cat`, API-backed `rules test`, `trace ls/get`, and `rules bench` all operated against the running daemon; `rules test` rendered all five request/response/trailer/tag actions.
- Curl forced to `--http1.1` received an HTTP/1.1 chunked response while trace TLS recorded client `alpn:http/1.1` and origin `alpn:h2`, proving h1→h2. The echoed body, injected request/response headers, `grpc-status`, `grpc-message`, and rule trailer were preserved.
- A second h1 request was a pool hit and completed in 3ms versus the first connection/handshake request at 36ms; trace flags distinguished `h2-upstream-pool-miss` and `h2-upstream-pool-hit`.
- Curl `--http2` negotiated h2 with rsproxy and received h2 from the h2 origin path with all trailers, proving h2→h2 while reusing the existing upstream connection.
- Eight parallel curl transfers all returned HTTP/2 200. Verbose `nghttpd` output showed one origin connection (`id=2`) carrying streams 7, 9, 11, 13, 15, 17, 19, and 21 concurrently, after earlier streams 1/3/5 on the same connection.
- A nine-byte binary unary gRPC frame (`00000000040a026869`: uncompressed, four-byte protobuf payload containing `hi`) returned byte-for-byte unchanged as `application/grpc`; the final h2 trailers were `grpc-status: 0`, `grpc-message: dogfood79`, and `x-rule-trailer: dogfood79`.
- Routing the HTTPS origin through `upstream(proxy://127.0.0.1:18963)` still negotiated h2 inside CONNECT. Trace identified the proxy route, origin `upstream_tls alpn:h2`, h2 pool miss, body equality, and all gRPC trailers.
- Curl h2 to the h1-only TLS origin returned HTTP/2 to the client while the origin body reported `HTTP/1.1`; trace separated client `alpn:h2` from origin `alpn:http/1.1`, proving fallback.
- A separate daemon with `--max-header-count 5` accepted a minimal outbound request but rejected the h2 origin response because its five fields plus `:status` exceeded the limit. Curl received an explicit 502 body `upstream_h2 response: header count limit exceeded (limit 5)`; trace retained both TLS records, `alpn:h2`, the staged error, and h2 pool-miss flags.
- Before idle eviction, changing the rule from direct origin to CONNECT hop left both old-route and new-route sockets established. Process sampling showed the debug main daemon at 0.0% CPU and about 11.5MiB RSS.
- After the optimized daemon restart, `lsof` initially showed both route sockets and showed only the proxy/API listeners after the 60-second idle interval. The next request reconnected with `h2-upstream-pool-miss` and origin TLS `alpn:h2`; an immediate second request recorded `h2-upstream-pool-hit`.
- `rules bench` completed 1,000 iterations with five matched actions per request: `p50_ns=8833`, `p99_ns=14750`, `max_ns=115042` in the debug build.
- Final workspace verification passed 74 CLI tests, 38 rules tests, 6 trace tests, all doc tests, and `cargo check --workspace --all-targets`.

Optimization from observation:

- Origin TLS now advertises `h2,http/1.1`; TLS to an HTTPS proxy hop remains h1-only. WebSocket upgrade, SSE accept, and request-throttle paths deliberately select origin h1 until equivalent streaming h2 implementations exist.
- Added a Hyper HTTP/2 client on the shared Tokio runtime, with pseudo-header conversion, connection-token stripping, `TE: trailers` filtering, request/response header limits, body/trailer collection, and reuse of the existing response-rule/trace pipeline.
- Added an upstream h2 pool keyed by state, origin, full route, TLS policy/identity, and header limits. It supports concurrent cross-client streams, marks hit/miss in trace, reconnects only when a cached sender fails before request dispatch, and never blindly retries a possibly-sent non-idempotent request.
- Added a shared readiness-driven `AsyncIo` adapter for client and origin rustls streams, replacing duplicated Unix adapters while retaining the non-Unix timer fallback.
- Capped the pool at 256 route keys and added generation-guarded 60-second idle eviction after dogfood exposed stale sockets across rule route changes; focused integration coverage forces expiry and verifies connection shutdown.
- Normalized h2 error-path trace flags so failed new connections and failed pooled exchanges remain queryable as `h2-upstream` with pool miss/hit provenance.
- Added tests for origin/proxy-hop ALPN separation, h2 header conversion/limits, pooled binary body and gRPC trailer preservation, response-period rule reuse, and idle eviction.

## Loop 80

Runtime setup:

- Independent TLS origin CA/storage: `/tmp/rsproxy-dogfood80-origin`; its root was not installed into the macOS keychain or either proxy storage.
- `nghttpd` h2 origins on `127.0.0.1:18250` and `127.0.0.1:18251` used leaves signed by that independent CA, echoed POST bodies, and returned `X-Origin-Root` trailers.
- Negative-control rsproxy on `127.0.0.1:18971`, API `127.0.0.1:18972`, storage `/tmp/rsproxy-dogfood80-negative`, with ordinary macOS native trust discovery.
- Positive rsproxy on `127.0.0.1:18973`, API `127.0.0.1:18974`, storage `/tmp/rsproxy-dogfood80-positive`, with `SSL_CERT_FILE` pointing only to the independent origin root.
- Shared rule: `127.0.0.1:18250 req.header(x-native-root: dogfood80) res.header(x-rsproxy-native-roots: yes) res.trailer(x-rule-trailer: native80) tag(native-root:${path})`.

Observed:

- Direct curl with the origin CA verified the fixture over h2 and received the upload plus origin trailer.
- On startup, the normal macOS loader reported 118 WebPKI roots and 159 native certificates with no rejects/errors. The isolated loader reported 118 WebPKI roots plus exactly one `SSL_CERT_FILE` certificate.
- CLI `status` exposed `upstream_roots` counts, while API-backed `rules test` rendered all request/response/trailer/tag actions.
- The negative-control curl completed client MITM h2 but returned HTTP/2 502 with `stage=tls: invalid peer certificate: UnknownIssuer`; trace preserved the failed `upstream_tls` record.
- The positive curl made the identical request and returned HTTP/2 200, byte-identical body, `X-Rsproxy-Native-Roots: yes`, `X-Origin-Root: native80`, and `X-Rule-Trailer: native80`. Trace recorded origin TLS 1.3, `alpn:h2`, two peer certificates, rule/tag provenance, and no error.
- A second positive request targeted port 18251, forcing a distinct origin connection/TLS configuration. No second root-loading log appeared, proving the expensive platform enumeration was cached rather than repeated per handshake.
- Starting a daemon with a missing `SSL_CERT_FILE` produced one explicit warning, kept the 118 WebPKI fallback roots, and exposed `native_errors:1` through status instead of failing silently.
- Initial root merge retained all 118 WebPKI plus 159 native anchors. After deduplication, macOS reported `native_duplicates:86` and `total:191`; the isolated CA path reported zero duplicates and `total:119`.
- Final trace exports at `/tmp/rsproxy-dogfood80-negative/trace-final.json` and `/tmp/rsproxy-dogfood80-positive/trace-final.json` preserve the failed/successful comparison.
- Final workspace verification passed 75 CLI tests, 38 rules tests, 6 trace tests, all doc tests, and `cargo check --workspace --all-targets`.

Optimization from observation:

- Added `rustls-native-certs` for macOS keychain, Windows certificate store, and Linux/Unix CA-bundle discovery, including standard `SSL_CERT_FILE` / `SSL_CERT_DIR` overrides.
- Added a per-daemon `OnceLock` cache for merged WebPKI/native roots; the current storage CA remains a dynamic per-configuration addition so initializing rsproxy CA while running does not stale the base cache.
- Invalid native entries are counted and ignored best-effort; loader errors are printed once and counted in status while WebPKI remains available.
- Deduplicate normalized trust anchors by subject/SPKI/name-constraints identity before caching, eliminating 86 duplicates in the observed macOS store.
- Added `upstream_roots` startup/status diagnostics for WebPKI, native loaded/rejected/duplicate, total, and error counts, plus focused merge/reject/dedup coverage.

## Loop 81

Runtime setup:

- Temporary storage and CA: `/tmp/rsproxy-dogfood81`; rsproxy issued the TLS leaf shared by both test origins.
- Node TLS origins on `127.0.0.1:18260` and `127.0.0.1:18261`: the first advertised h2, while the second advertised only `http/1.1`. Both echoed method/path/body/request trailers and returned an origin response trailer.
- Rsproxy on `127.0.0.1:18981`, API `127.0.0.1:18982`, with per-origin rules adding a request header, response header, response trailer, and path-derived tag.
- A raw Node client opened CONNECT tunnels and sent either HTTP/1.1 chunked requests with terminal trailers or HTTP/2 DATA plus trailing HEADERS. Curl `--http2` supplied ordinary no-trailer control traffic.

Observed:

- h1→h1, h1→h2, h2→h1, and h2→h2 requests all preserved the exact body and `X-Client-Trailer` value. Each origin also observed the rule-injected request header, and each client received both the origin and rule-added response trailers.
- Origin logs independently confirmed the received protocol and request trailer in all four directions. Curl controls reached both origins without synthetic request trailers and preserved the existing response-trailer behavior.
- Trace detail marked trailer-bearing requests with `req-trailers`, retained `req_trailers` separately from headers, and still distinguished client/upstream ALPN plus h2 pool hit/miss. JSON export, HAR `request._trailers` / `response._trailers`, NDJSON spill, and the TUI Headers tab preserved the same values.
- The final rebuilt-binary h2→h2 trace recorded client and origin TLS 1.3 with `alpn:h2`, the exact request trailer, both response trailers, matched-rule provenance, and no error. Final exports are `/tmp/rsproxy-dogfood81/trace-latest.json` and `/tmp/rsproxy-dogfood81/trace-latest.har`.
- A raw request carrying both `Content-Length` and `Transfer-Encoding` returned a complete `400 Bad Request` with `request must not contain both Content-Length and Transfer-Encoding`. A chunked request using forbidden `Content-Length` as a trailer returned a complete 400 naming that field; neither request reached an origin or created a normal session.
- The first malformed-request run exposed a partially delivered error response because the proxy closed with unread adversarial bytes after several small writes. Coalescing small responses made the complete status, headers, and body observable before the expected malformed-connection reset.
- Final workspace verification passed 82 CLI tests, 38 rules tests, 6 trace tests, all doc tests, and `cargo check --workspace --all-targets`.

Optimization from observation:

- Extended `RawRequest` and trace sessions with request trailers. HTTP/1.1 parsing now decodes chunk extensions, payload chunks, and terminal trailers under the configured header size/count limits.
- Hardened request framing by rejecting conflicting `Content-Length` values, simultaneous Content-Length/Transfer-Encoding, unsupported transfer codings, invalid trailer syntax, and security/framing-sensitive trailer fields.
- HTTP/1.1 upstream translation re-emits trailer-bearing requests as chunked with a normalized `Trailer` declaration; decoded chunked requests without trailers become an exact `Content-Length` request.
- HTTP/2 client intake collects trailing HEADERS, while the pooled Hyper HTTP/2 client emits request trailers after DATA. The shared request/response rule pipeline now bridges trailers across every h1/h2 client-origin combination.
- Added request-trailer visibility to detail JSON, spill, HAR, and the TUI, plus focused parser, bridge, pooled-h2, trace, export, and snapshot tests.
- Small HTTP/1.1 responses are assembled for one write so rejection details survive immediate close; bodies above 64 KiB retain split header/body writes to avoid a second large allocation.

## Loop 82

Runtime setup:

- Temporary storage and CA: `/tmp/rsproxy-dogfood82.kueAIb`; rsproxy issued the `127.0.0.1` TLS leaf used by the h1-only HTTPS origin.
- Node HTTP/1.1 origins on `127.0.0.1:18300` (plain) and `127.0.0.1:18301` (TLS with ALPN `http/1.1`). Each response exposed a connection id, request sequence on that connection, total accepted sockets, whether the proxy advertised trailer support, and an origin trailer.
- Rsproxy on `127.0.0.1:18991`, API `127.0.0.1:18992`; per-origin rules injected a request header, response header, response trailer, and trace tag.
- Separate curl invocations forced distinct downstream client connections, making any repeated origin connection id evidence of cross-client upstream pooling rather than client socket reuse.

Observed:

- CLI `status`, `rules set`, `rules test`, `trace ls/export`, TUI snapshot, and `rules bench` all operated against the running daemon. The 1,000-iteration debug rules bench recorded `p50_ns=11250`, `p99_ns=12125`, and `max_ns=64292` for four matched actions.
- The first and second plain curl requests reached origin connection 2 with request sequence 1 then 2. Trace marked the first `h1-upstream-pool-miss` and the second `h1-upstream-pool-hit`; measured duration fell from 4ms to 1ms.
- The first and second TLS curl requests likewise used origin connection 2, sequence 1 then 2. The first trace contained origin TLS 1.3 with `alpn:http/1.1` and took 38ms; the 3ms second trace was a pool hit and correctly contained no repeated origin TLS handshake record.
- Both paths preserved the rule-injected request/response headers, origin and rule-added response trailers, and full response bodies. The origins reported `acceptsTrailers:true`, proving rsproxy advertised h1 trailer capability independently of curl's downstream headers.
- Restarting both origins invalidated idle sockets. The next plain request safely opened new connection 1 and traced a miss rather than failing or replaying the request. A subsequent `/force-close` request reused that socket and traced a hit, but the following request opened connection 2 and traced a miss, proving an origin `Connection: close` response was not returned to the pool.
- Initial curl output exposed the origin's `Keep-Alive: timeout=120` in a client response that rsproxy itself closed. After optimization, final curl, trace JSON, HAR, spill, and TUI headers contained no `Keep-Alive`; end-to-end headers and trailers remained intact.
- Final exports are `/tmp/rsproxy-dogfood82.kueAIb/trace-final.json` and `/tmp/rsproxy-dogfood82.kueAIb/trace-final.har`.
- Final workspace verification passed 85 CLI tests, 38 rules tests, 6 trace tests, all doc tests, formatting checks, and `cargo check --workspace --all-targets`.

Optimization from observation:

- Added a shared Hyper HTTP/1.1 client pool for ordinary buffered requests. Idle senders are keyed by daemon state/storage, origin, complete route, TLS policy/identity, and header limits; the pool supports multiple idle connections per key, a 256-entry global/per-key idle cap, and a shared 90-second expiry sweeper.
- Pool checkout removes a sender for exclusive use until the response body and trailers are complete. Stale pre-dispatch senders fall back to a fresh connection; errors after dispatch are returned without blindly retrying a possibly-sent non-idempotent request.
- WebSocket, SSE, request-throttle, and HTTP/1.0 traffic remain on their existing specialized h1 paths. TLS origins continue independent ALPN selection, so negotiated h2 enters the existing multiplexed pool while h1 enters the new pool.
- Hyper h1 request conversion normalizes connection-managed fields, forces chunked framing when request trailers exist, and advertises `Connection: TE` plus `TE: trailers` because the proxy can bridge origin trailers even when the downstream client did not send that hop-by-hop capability.
- Added `h1-upstream` and pool hit/miss trace flags, structured error provenance, route-isolated pool keys, body/header/request-and-response-trailer reuse tests, and real origin connection counters.
- Added shared response hop-header stripping that removes `Connection`-declared extension fields, `Keep-Alive`, proxy connection fields, TE/framing fields, and upgrade metadata before final h1/h2 framing is rebuilt.

## Loop 83

Runtime setup:

- Temporary storage and CA: `/tmp/rsproxy-dogfood83.3xTU9Q`; rsproxy issued the `127.0.0.1` TLS leaf used by the h1-only HTTPS origin.
- Node HTTP/1.1 origins on `127.0.0.1:18310` (plain) and `127.0.0.1:18311` (TLS, ALPN `http/1.1`). Responses exposed origin connection id, sequence on that socket, total connections, remote port, rule header, and trailers; a finite `/sse` endpoint exercised close-delimited streaming.
- Rsproxy on `127.0.0.1:19001`, API `127.0.0.1:19002`, with request/response header, response trailer, and tag rules for both origins.
- Curl multi-URL invocations tested sequential reuse on one downstream connection; a raw Node socket wrote two complete requests before reading either response to prove ordered pipeline handling.

Observed:

- One plain curl invocation requested two URLs. Curl printed `Re-using existing connection with proxy`; both trace sessions had the same client address, and the origin reported connection 6 with sequence 1 then 2 and the same remote port. The second request also hit the h1 upstream pool.
- One HTTPS curl invocation established a single CONNECT/TLS tunnel, then printed the same reuse message for its second URL. Final optimized evidence used client `127.0.0.1:55354`, origin connection 3, sequence 1 then 2; trace duration fell from 36ms to 1ms and curl's second `time_connect` was zero.
- The raw pipeline sent two HTTP/1.1 requests in one write. Rsproxy returned two ordered 200 responses, first `Connection: keep-alive` and second `Connection: close`, while preserving both origin/rule trailer sets. Trace marked only the second session `h1-client-connection-reused` and both sessions shared one client address.
- HTTP/1.0 without an explicit persistence token returned an HTTP/1.0 response with Content-Length and close. A pipelined HTTP/1.0 pair using `Proxy-Connection: Keep-Alive` on the first request returned keep-alive then close on the same downstream socket. Origin and rule trailers were suppressed rather than emitting unsupported chunked framing.
- SSE with a response-trailer rule used the buffered, safely framed path and remained persistent. After temporarily loading a header-only rule, the true streaming SSE path removed length/chunk framing, returned `Connection: close`, delivered the event, and exited the request loop.
- Initial TLS curl output exposed a successful CONNECT response containing contradictory `Connection: close`; removing that header preserved tunnel establishment and multi-request reuse.
- Initial reused MITM trace repeated `mitm-cert-cache-miss` and the first request's 27ms client handshake on the 1ms second session. Final trace instead marked `mitm-tunnel-reused`, omitted the cache event, and retained TLS protocol/cipher/ALPN context with `handshake_ms:0`.
- CLI `status`, rule hot switching/restoration, `trace ls/export`, spill, and TUI snapshot all remained operational. TUI showed `h1-client-connection-reused`, `mitm-tunnel-reused`, upstream pool hit, trailers, and `h1-client-keepalive` on the second TLS request.
- Evidence exports are `/tmp/rsproxy-dogfood83.3xTU9Q/trace-final.json`, `/tmp/rsproxy-dogfood83.3xTU9Q/trace-optimized.json`, and final `/tmp/rsproxy-dogfood83.3xTU9Q/trace-verified.json` / `trace-verified.har`.
- Final workspace verification passed 89 CLI tests, 38 rules tests, 6 trace tests, all doc tests, formatting checks, and `cargo check --workspace --all-targets`.

Optimization from observation:

- Converted both plain proxy and MITM HTTP/1.x handlers from one-request ownership to ordered per-connection loops. Each request still receives an independent rule snapshot, upstream dispatch, trace session, and response framing decision.
- Added HTTP/1.1 default persistence plus case-insensitive Connection/Proxy-Connection close handling; HTTP/1.0 remains close by default and only persists when explicitly requested.
- Response writers now select one Connection header and an HTTP/1.0/1.1 status line instead of always appending close. Buffered responses rebuild Content-Length or chunk terminators, so trailers and body boundaries remain safe across subsequent requests.
- Streaming SSE and WebSocket/CONNECT socket takeover return a close disposition to the HTTP loop. Successful CONNECT responses omit the misleading close header because the connection becomes a tunnel.
- Added a 90-second downstream keep-alive read timeout so idle or stalled clients cannot retain one blocking worker indefinitely.
- Added `h1-client-keepalive`, `h1-client-close`, `h1-client-connection-reused`, and `mitm-tunnel-reused` trace flags. Reused MITM TLS context records zero incremental handshake time.
- Added real TCP pipeline tests, persistence negotiation tests, single selected Connection-header tests, and HTTP/1.0 trailer downgrade tests.

## Loop 84

Runtime setup:

- Temporary storage and evidence directory: `/tmp/rsproxy-dogfood84.PeizZH`.
- A Node HTTP/1.1 origin on `127.0.0.1:18320` delayed each response by a query parameter and reported connection id, per-connection request sequence, current active requests, and maximum observed concurrency.
- Rsproxy ran on `127.0.0.1:19011`, API `127.0.0.1:19012`, with `--h1-pool-max-active-per-key 1`. The success phase used an 800ms wait timeout; the timeout phase restarted the daemon with 120ms.
- Rule: `127.0.0.1:18320 res.header(x-rsproxy-pool: dogfood84) tag(pool-wait:${path})`.

Observed:

- CLI `status` exposed `h1_pool.max_active_per_key:1` and the configured wait timeout. `rules check`, API-backed `rules set`, and `rules test` all succeeded before traffic.
- Two real curl requests started 50ms apart. The first held the origin for 600ms; the second waited and then completed with 200 in 568ms. Its trace duration was 562ms with `pool_wait_ms:537` and `h1-upstream-pool-hit`.
- Both success requests used origin connection 1 with sequence 3 then 4, and the origin never observed more than one active request. This proves the waiter reused the returned sender instead of opening a second connection.
- HAR exported the waited request as `timings.blocked:537` and `wait:25`; the TUI overview rendered `total=562ms pool_wait=537ms`. Rule response headers and trace tags remained intact.
- With a 120ms wait timeout, a second request behind a 500ms holder returned `HTTP/1.1 504 Gateway Timeout` in 128ms. Its trace recorded status 504, `pool_wait_ms:120`, `h1-upstream-pool-wait-timeout`, and the staged error `upstream_h1 pool_wait: timeout after 120ms (active limit 1)`; HAR reported `blocked:120`.
- After the holder completed, a third request returned 200 in 15ms. The holder and follow-up used origin connection 2 with sequence 1 then 2, while maximum active requests remained one, proving timeout cleanup did not leak the permit or poison the idle sender.
- Evidence exports are `/tmp/rsproxy-dogfood84.PeizZH/success.har` and `/tmp/rsproxy-dogfood84.PeizZH/timeout.har`.
- Final workspace verification passed 91 CLI tests, 38 rules tests, 6 trace tests, all doc tests, formatting checks, and `cargo check --workspace --all-targets`.

Optimization from observation:

- Added a per-pool-key active lease limit for ordinary buffered h1 traffic. The lease spans idle checkout or fresh connect through complete response body/trailer collection, so one permit corresponds to one in-flight h1 connection.
- Added `--h1-pool-max-active-per-key` and `--h1-pool-wait-timeout-ms`, with defaults of 256 and 15 seconds, plus control-status visibility and startup validation.
- Added condition-variable admission. Release wakes all waiters because a shared condition variable serves multiple keys; each waiter rechecks its own key under the mutex, avoiding a cross-key lost opportunity.
- Added structured `pool_wait_ms` to in-memory sessions, summary/detail JSON, NDJSON spill, HAR blocked timing, and TUI overview.
- Pool-wait expiration now returns 504 with a dedicated trace flag. Pre-dispatch stale senders may still connect fresh under the same permit, while errors after dispatch remain non-retriable.
- Added focused lease release/timeout tests, leak checks, pool-wait error classification coverage, and CLI configuration validation.

## Loop 85

Runtime setup:

- Temporary storage and evidence directory: `/tmp/rsproxy-dogfood85.6g8B4m`; its rsproxy CA issued the TLS certificate used by the test origin.
- A Node secure HTTP/2 origin on `127.0.0.1:18330` reported origin session id, stream id, session request sequence, active streams, maximum observed concurrency, path, delay, and protocol.
- Rsproxy ran on `127.0.0.1:19021`, API `127.0.0.1:19022`. Admission-success used one active stream and an 800ms timeout; starvation used one stream and 120ms; cold-pool single-flight used two streams and 800ms.
- Rule: `127.0.0.1:18330 res.header(x-rsproxy-h2-pool: dogfood85) tag(h2-pool-wait:${path})`.

Observed:

- CLI `status` exposed both h1 and h2 pool settings, including `h2_pool.max_active_streams_per_key` and `wait_timeout_ms`. CA init/issue, `rules check/set/test`, trace commands, HAR export, TUI snapshot, and rules bench all operated against the running daemon.
- Two independent HTTP/2 curl requests started 50ms apart behind a one-stream limit. A 600ms holder returned in 628ms; the 20ms waiter returned in 592ms. Both used origin session 2, streams 1 then 3, sequence 1 then 2, and the origin never observed more than one active stream.
- The waiter trace recorded duration 587ms, `pool_wait_ms:564`, `h2-upstream-pool-hit`, client and origin h2, and the path-derived rule tag. HAR reported `blocked:564` and `wait:23`; the TUI rendered `total=587ms pool_wait=564ms`. Both curl responses retained the origin protocol header and rule-added header.
- With a 120ms limit, the second curl returned HTTP/2 504 in about 130ms while a 500ms stream held the only permit. Final trace recorded duration 122ms, `pool_wait_ms:120`, `h2-upstream-pool-wait-timeout`, and `upstream_h2 pool_wait: timeout after 120ms (active stream limit 1)`; HAR reported `blocked:120`.
- Timeout did not leak a permit or damage the pooled connection. The holder and immediate follow-up remained on origin session 6, sequence 3 then 4 and streams 5 then 7, with maximum active streams still one.
- A connection-level probe proved `trace clear` leaves the h2 pool intact: requests before and after clear used origin session 6, sequence 1 then 2 and streams 1 then 3, while the TCP socket remained established. Earlier prewarm sessions had simply crossed the existing 60-second idle TTL during manual inspection.
- Cold-pool concurrency with a two-stream limit sent two 400ms requests 10ms apart. Both used the only origin session 7 on streams 1 and 3; origin active concurrency reached two, both curls completed in about 465ms, and the second trace attributed 6ms to pool wait. This proves connector single-flight without serializing streams after publication.
- A final origin advertised `SETTINGS_MAX_CONCURRENT_STREAMS=1` while rsproxy's local limit remained two. The second request waited behind the first inside Hyper and returned 200 after 461ms rather than timing out; its trace correctly did not invent local pool wait. Hyper 1.10's public h2 `SendRequest::ready()` only checks connection closure, while the response future combines internal stream-open waiting with response-header wait, so wrapping that future would misclassify normal TTFB.
- The 1,000-iteration debug rules bench reported `p50_ns=9417`, `p99_ns=12500`, and `max_ns=95542` for the one indexed rule.
- Evidence exports are `/tmp/rsproxy-dogfood85.6g8B4m/success.har`, `/tmp/rsproxy-dogfood85.6g8B4m/timeout-final.json`, `/tmp/rsproxy-dogfood85.6g8B4m/timeout-final.har`, `/tmp/rsproxy-dogfood85.6g8B4m/singleflight-final.json` / `.har`, and `/tmp/rsproxy-dogfood85.6g8B4m/remote-settings-final.json`.
- Final workspace verification passed 94 CLI tests, 38 rules tests, 6 trace tests, all doc tests, formatting checks, and `cargo check --workspace --all-targets`.

Optimization from observation:

- Added configurable per-key h2 stream leases via `--h2-pool-max-active-streams-per-key` and `--h2-pool-wait-timeout-ms`, defaulting to 256 streams and 15 seconds, with control-status visibility and startup validation.
- A lease spans pooled checkout or fresh connection selection through complete response body/trailer collection. Local admission and connector ownership wait on a condition variable under one deadline; response dispatch remains outside that deadline so origin TTFB cannot be mislabeled as pool starvation.
- Added generation-token connector ownership. One cold request establishes and publishes the connection; concurrent misses wait and then use cloned senders on that same session. Connector failure and h1 ALPN fallback release ownership safely.
- Active h2 streams now prevent the 60-second idle sweeper and opportunistic lookup pruning from evicting their connection.
- Successful waits reuse the shared `pool_wait_ms` JSON/spill/HAR/TUI path. Expiration returns 504 with `h2-upstream-pool-wait-timeout`, without incorrectly labeling the request as a pool hit or miss.
- Added focused active-stream timeout/leak, connector serialization, connector timeout, h2 response timing propagation, error classification, and CLI configuration tests.

## Loop 86

Runtime setup:

- Temporary storage and evidence directory: `/tmp/rsproxy-dogfood86.LGO8Xi`; its rsproxy CA issued the certificate later used by the valid recovery origin.
- A Node TCP peer on `127.0.0.1:18340` first accepted TLS bytes without replying, then was replaced by a peer that immediately sent plaintext HTTP, and finally by a valid TLS HTTP/1.1 origin.
- Rsproxy ran on `127.0.0.1:19031`, API `127.0.0.1:19032`, with `--upstream-tls-handshake-timeout-ms 120`.
- Rule: `127.0.0.1:18340 res.header(x-rsproxy-tls-timeout: dogfood86) tag(tls-handshake:${path})`.

Observed:

- CLI `status` exposed `timeouts.upstream_tls_handshake_ms:120`. CA init/issue, `rules check/set/test`, trace commands, HAR export, TUI snapshot, and rules bench all operated against the live daemon.
- Curl negotiated client-side HTTP/2 through CONNECT/MITM, then the silent origin hit the upstream TLS deadline. Curl received HTTP/2 504 in 183ms including client setup; trace duration was 126ms, failed `upstream_tls.handshake_ms` was 123ms, and the staged error was `stage=tls_handshake: timeout after 120ms`.
- The timeout trace kept `pool_wait_ms:0`, client TLS 1.3/h2, the failed upstream TLS record, rule tag, `upstream-timeout`, and `upstream-tls-handshake-timeout`. TUI showed the same stage and timing.
- Replacing the peer with immediate plaintext produced HTTP/2 502 in 11ms. Trace duration was 6ms, upstream handshake time was 2ms, error was `stage=tls: received corrupt message of type InvalidContentType`, and no timeout flags were present. This proves protocol/certificate failures are not broadened into 504s.
- Replacing the peer with a valid TLS h1 origin restored 200 responses. Before optimization, h2-client to h1-origin requests each opened a new origin TLS connection because h1 pooling admitted only literal HTTP/1.1 clients.
- After expanding h1 pool eligibility to HTTP/2 clients and rebuilding/restarting, two independent HTTP/2 curl requests both returned 200; the origin reported sequence 1 then 2 on one connection. Trace moved from `h1-upstream-pool-miss` at 8ms to `h1-upstream-pool-hit` at 1ms, and the second request contained no repeated upstream TLS record.
- The 1,000-iteration debug rules bench reported `p50_ns=7333`, `p99_ns=8167`, and `max_ns=12458` for the one indexed rule.
- Evidence exports are `/tmp/rsproxy-dogfood86.LGO8Xi/timeout.json` / `.har`, `/tmp/rsproxy-dogfood86.LGO8Xi/invalid.json`, `/tmp/rsproxy-dogfood86.LGO8Xi/recovery-final.json` / `.har`, and `/tmp/rsproxy-dogfood86.LGO8Xi/recovery-optimized.json` / `.har`.
- Final workspace verification passed 96 CLI tests, 38 rules tests, 6 trace tests, all doc tests, formatting checks, and `cargo check --workspace --all-targets`.

Optimization from observation:

- Added `--upstream-tls-handshake-timeout-ms`, defaulting to 10 seconds, with startup validation and `status.timeouts` visibility.
- Added a deadline-aware rustls transport wrapper. Before every underlying read/write it applies only the remaining total deadline to the recursively nested TCP socket, preventing a trickle peer from renewing a full timeout on each operation.
- Successful handshakes restore the established 60-second read and 30-second write timeouts. Timeout errors preserve `TimedOut`, return 504, record the failed TLS phase, and add dedicated flags; non-timeout TLS errors preserve their original kind and 502 behavior.
- Enabled the shared upstream h1 pool for ordinary HTTP/2 client requests that negotiate h1 at the origin, eliminating repeated TCP/TLS setup across separate h2 client sessions.
- Added silent-peer network coverage, non-timeout error-kind coverage, h1 pool eligibility assertions, and CLI configuration validation.

## Loop 87

Runtime setup:

- Temporary storage and evidence directory: `/tmp/rsproxy-dogfood87.v6lNBS`.
- A local Python TCP fixture listened on `127.0.0.1:18350` with backlog one, never accepted, and retained one filler connection to make subsequent TCP handshakes stall deterministically. It was later stopped for refusal testing and replaced by a normal Node HTTP/1.1 origin.
- Rsproxy ran on `127.0.0.1:19041`, API `127.0.0.1:19042`, with `--tcp-connect-timeout-ms 120` and no initialized CA so HTTPS curl could exercise raw CONNECT bypass.
- Rule: `127.0.0.1:18350 res.header(x-rsproxy-connect-timeout: dogfood87) tag(tcp-connect:${path})`.

Observed:

- CLI `status` exposed `timeouts.tcp_connect_ms:120`. `rules check/set/test`, trace commands, HAR export, and rules bench all operated against the live daemon.
- HTTP curl through the proxy hit the saturated accept queue and received `HTTP/1.1 504 Gateway Timeout` in 124ms. Trace duration was 122ms with `pool_wait_ms:0`, `stage=connect: timeout after 120ms connecting to 127.0.0.1:18350`, `upstream-timeout`, and `upstream-tcp-connect-timeout`.
- Stopping the listener changed the same request to a 502 in 3.8ms. Trace duration was 1ms with `stage=connect: Connection refused`, and no timeout flags were present. This proves configured deadlines do not broaden immediate network failures into 504s.
- Replacing the fixture with a normal origin restored two independent curl requests to 200. Both used origin connection 1, sequence 1 then 2; trace moved from h1 pool miss at 13ms to pool hit at 1ms, and both retained the rule-added response header.
- Initial review found raw CONNECT bypass still hard-coded 502 for every connection failure. After fixing, rebuilding, and recreating the saturated backlog, HTTPS curl failed its CONNECT tunnel with response 504 in 126ms; the tunnel trace recorded status 504, duration 123ms, and the same dedicated timeout flags.
- The 1,000-iteration debug rules bench reported `p50_ns=6750`, `p99_ns=7542`, and `max_ns=74458` for the one indexed rule.
- Evidence exports are `/tmp/rsproxy-dogfood87.v6lNBS/timeout.json` / `.har`, `/tmp/rsproxy-dogfood87.v6lNBS/refused.json`, `/tmp/rsproxy-dogfood87.v6lNBS/recovery-final.json` / `.har`, and `/tmp/rsproxy-dogfood87.v6lNBS/connect-timeout.json`.
- Final workspace verification passed 97 CLI tests, 38 rules tests, 6 trace tests, all doc tests, formatting checks, and `cargo check --workspace --all-targets`.

Optimization from observation:

- Added `--tcp-connect-timeout-ms`, defaulting to 10 seconds, with startup validation and `status.timeouts` visibility.
- Core direct, SOCKS5, HTTP/HTTPS proxy, CONNECT, and multi-hop routes now resolve candidate socket addresses once and share one absolute connect deadline across them. A timeout preserves `TimedOut` and emits an explicit target/stage error.
- Resolution errors are already separated as `stage=dns`; DNS resolution itself still uses blocking `ToSocketAddrs`, pending the designed Hickory cache and DNS deadline.
- Immediate refusal/unreachable errors preserve their original kind and 502 behavior. Connect deadline expiration returns 504 with dedicated flags in both HTTP sessions and raw CONNECT tunnel sessions.
- Added timeout/refusal classification coverage and CLI configuration validation. Dogfood also revalidated lease cleanup and h1 pool recovery after both failure modes.

## Loop 88

Runtime setup:

- Temporary storage and evidence directory: `/tmp/rsproxy-dogfood88.yRaFcd`.
- A local UDP DNS fixture on `127.0.0.1:15360` returned A records for `*.rsproxy.test`, AAAA NODATA with SOA, and NXDOMAIN with SOA for `missing.rsproxy.test`, all with 60-second TTLs. A Python HTTP/1.1 origin on `127.0.0.1:18360` forced `Connection: close` so an upstream pool hit could not hide DNS behavior.
- Rsproxy ran on `127.0.0.1:19051`, API `127.0.0.1:19052`, with `--dns-server 127.0.0.1:15360 --dns-timeout-ms 120 --dns-cache 60 --tcp-connect-timeout-ms 500` and no initialized CA so HTTPS curl exercised raw CONNECT.
- Rules added response markers for `origin.rsproxy.test` and rewrote `bypass.rsproxy.test` with `host(127.0.0.1:18360)` to exercise literal-IP DNS bypass. `rules check`, `rules set`, and `rules test` all succeeded against the live daemon.

Observed:

- `status` exposed custom DNS mode, server, 60-second cache cap, 120ms deadline, and live counters. The final snapshot reported seven resolver calls, three successes, four failures including two timeouts, and one literal-IP bypass.
- Before optimization, an A success paired with AAAA NODATA was resolved successfully once but the cached AAAA negative result won on the next combined lookup; with DNS stopped, the second request timed out. A temporary full A+AAAA response proved that Hickory's positive cache itself worked, narrowing the defect to lookup result ordering.
- After changing to parallel A/AAAA lookup with IPv4 result priority and rebuilding, the IPv4-only fixture served `cache4.rsproxy.test` in 12.6ms, issuing one A and one AAAA query. DNS was then stopped; the second independent curl still returned 200 in 4.7ms without another nameserver query. Trace moved from `dns_ms:4, connect_ms:3` to `dns_ms:0, connect_ms:1`; the origin sequence advanced, proving this was a fresh TCP request rather than a pooled connection.
- NXDOMAIN returned 502 in 5.7ms with `dns_ms:3`. After stopping DNS, the same name returned the same 502 in 2.9ms with `dns_ms:0`, proving negative-cache reuse without broadening a normal lookup failure into 504.
- An uncached HTTP name with DNS stopped returned 504 in 126ms. Trace duration was 123ms with `dns_ms:122`, `connect_ms:0`, `stage=dns: timeout after 120ms`, `upstream-timeout`, and `upstream-dns-timeout`.
- Raw CONNECT to another uncached name returned 504 to curl in 126ms. Its tunnel trace was 122ms with `dns_ms:121`, `connect_ms:0`, `no-ca`, and the same dedicated timeout flags.
- With DNS still stopped, the `host(127.0.0.1:18360)` rule returned 200 in 5.7ms and retained its response marker. Trace showed upstream `127.0.0.1:18360`, `dns_ms:0`, `connect_ms:1`, and the status counter recorded one literal bypass.
- Restarting DNS made the previously timed-out HTTP name return 200 in 18ms; trace duration was 6ms with a fresh `dns_ms:3`, proving timeouts do not poison the cache or resolver health state.
- HAR export exposed separate `blocked`, `dns`, and `connect` values matching trace details; the TUI overview displayed the same total/pool/DNS/connect decomposition. The 1,000-iteration rules bench reported `p50_ns=7875`, `p99_ns=8875`, and `max_ns=109208` for two indexed rules.
- Evidence exports are `/tmp/rsproxy-dogfood88.yRaFcd/final.json` and `/tmp/rsproxy-dogfood88.yRaFcd/final.har`, with per-phase curl headers/bodies in the same directory.
- Final workspace verification passed 103 CLI tests, 38 rules tests, 6 trace tests, all doc tests, formatting checks, and `cargo check --workspace --all-targets`.

Optimization from observation:

- Added hickory-resolver with system resolver loading, custom IP/port nameservers, positive and negative caching, a configurable 60-second TTL cap, and explicit zero-cache behavior.
- Added `--dns-timeout-ms` as an absolute stage deadline around the complete lookup future. Timeout errors preserve `TimedOut`, return 504 in HTTP and CONNECT paths, and receive dedicated trace flags; NXDOMAIN/NODATA remain 502.
- Changed Hickory's result strategy from its IPv6-first default to parallel `Ipv4AndIpv6`. This retains both address families while ensuring a cached A result is not hidden by an AAAA NODATA result; a focused dual-stack/IPv4-only cache regression now covers both cases.
- Replaced all proxy-core `ToSocketAddrs` use with one shared resolver while preserving route semantics: direct origin and proxy-hop hosts resolve locally, names delegated through SOCKS/HTTP CONNECT remain delegated, and literal IP/`host(...)` routes skip DNS.
- Added `dns_ms` and `connect_ms` to Session, JSON summary/detail, spill, HAR, and TUI. Status now exposes DNS configuration and runtime counters, and CLI parsing validates DNS deadline, cache, and nameserver values.

## Loop 89

Runtime setup:

- Temporary storage and evidence directory: `/tmp/rsproxy-dogfood89.8pCIIr`; its rsproxy CA issued the leaf certificate used by the local TLS h2 origin.
- A persistent raw HTTP/1.1 fixture on `127.0.0.1:18370` recorded connection/request sequence and independently delayed response headers or body by 250ms. A Node TLS HTTP/2 fixture on `127.0.0.1:18371` recorded h2 session/stream sequence and provided equivalent slow-head/slow-body paths.
- Rsproxy ran on `127.0.0.1:19061`, API `127.0.0.1:19062`, with `--upstream-ttfb-timeout-ms 120`. Rules added protocol-specific response markers and request trace tags; `status`, `rules check/set/test`, trace commands, TUI, HAR export, and rules bench all ran against the live daemon.

Observed:

- Status exposed `timeouts.upstream_ttfb_ms:120`. The h1 prime request returned 200 in 12ms on origin connection 1; the next slow-head request reused connection 1 and returned 504 in 124ms. Trace duration was 121ms with `ttfb_ms:120`, `pool_wait_ms:0`, `upstream_h1 pool_hit ttfb: timeout after 120ms`, and dedicated timeout flags.
- The post-timeout h1 request recovered with 200 in 3.3ms on a new origin connection 2 and a pool miss. A 250ms slow-body request then reused connection 2 and returned 200 in 255ms with `ttfb_ms:0`, proving the TTFB deadline neither covered body collection nor leaked the failed h1 lease.
- `Accept: text/event-stream` forced the existing manual h1 path. Its slow-head request returned 504 in 125ms with `stage=ttfb: timeout after 120ms`; its 250ms slow-body peer returned 200 in 259ms. This independently validates the deadline-aware blocking reader outside Hyper.
- Curl negotiated client-side HTTP/2 through CONNECT/MITM to the TLS h2 origin. Prime, slow-head, recovery, and slow-body requests all reached upstream h2 session 1 with stream sequences 1 through 4. Slow-head returned HTTP/2 504 in 127ms and the origin observed that stream being cancelled; the immediately following recovery returned 200 in 6.9ms on the same session, proving one timed-out stream does not evict a healthy shared connection.
- The h2 slow-body response returned 200 in 258ms with `ttfb_ms:0`; its trace remained an h2 pool hit. The h2 timeout trace was 122ms with `ttfb_ms:120`, `upstream_h2 ttfb: timeout after 120ms`, `upstream-ttfb-timeout`, and no invented pool wait.
- An origin that closed immediately returned 502 in 4.4ms with a normal send error and no timeout flags, proving TTFB classification does not broaden protocol/connection failures.
- HAR mapped the three 504 requests to `timings.wait:120`; slow-body requests mapped their 252-256ms remainder to `receive`. TUI rendered total/pool/DNS/connect/TTFB values consistently. The 1,000-iteration rules bench reported `p50_ns=8500`, `p99_ns=9333`, and `max_ns=47125` for two indexed rules.
- Evidence exports are `/tmp/rsproxy-dogfood89.8pCIIr/final.json` and `/tmp/rsproxy-dogfood89.8pCIIr/final.har`, with protocol-specific curl headers/bodies in the same directory.
- Final pre-refactor workspace verification passed 107 CLI tests, 38 rules tests, 6 trace tests, all doc tests, and formatting/check gates.

Optimization from observation:

- Added `--upstream-ttfb-timeout-ms`, defaulting to 60 seconds, with startup validation and status visibility. HTTP errors preserve `TimedOut`, return 504, and add `upstream-timeout` plus `upstream-ttfb-timeout`; immediate transport failures remain 502.
- Manual h1 now uses a deadline-aware reader only until the first response byte, then restores the normal response-body read timeout. Hyper h1/h2 wrap only `send_request`, after local pool admission/readiness and before body collection, so DNS/TCP/TLS/pool/body time cannot be mislabeled as TTFB.
- A timed-out h1 request drops its exclusive sender and releases its lease. A timed-out h2 stream is cancelled without removing the shared pool entry; focused regression and live session evidence prove the same h2 connection remains usable.
- Added `ttfb_ms` to Session, JSON summary/detail, spill, HAR, and TUI. HAR now maps TTFB to standard `wait` and places the remaining measured duration in `receive`.
- Added manual h1, Hyper h1, Hyper h2 connection-survival, slow-body exclusion, classifier, spill, and CLI configuration coverage.

## Post-Loop 89 Structural Refactor

No new feature loop was started after Loop 89. The workspace was reorganized before further Dogfooding:

- Converted `rsproxy-cli` to a thin binary plus a testable library entry point. CLI commands are grouped by API, arguments, daemon, CA/trust, rules, trace, and platform-specific system proxy implementations.
- Replaced the monolithic proxy implementation with focused modules for server admission, HTTP flow, upstream forwarding/response handling, stream state, routing, connect/TLS/proxy handshakes, WebSocket framing, body/SSE handling, rule actions, mock responses, cookies, tunnels, auth, and trace helpers.
- Split `rsproxy-rules` into model facade, resolve, index, matcher/matching, and parser syntax/action modules. Split `rsproxy-trace` into its public store facade, spill subsystem, and serializer.
- Moved all inline tests into explicit module test files. Large proxy and rules suites now use behavior-oriented `src/.../tests/` directories; every crate also has a conventional black-box `tests/` integration test directory. `docs/testing.md` documents the layout and commands.
- Added `scripts/check-rust-lines.sh`; every Rust source and test file is at or below 500 lines. The final maximum was 487 lines.
- Final gates passed `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, and `cargo test --workspace --all-targets`: 107 CLI unit tests, 38 rules unit tests, 6 trace unit tests, and 3 public integration tests. Clippy was not available in the installed Rust toolchain.
- A post-refactor runtime smoke sent a real request through rsproxy (`127.0.0.1:19071`) to a local origin (`127.0.0.1:18089`), returned 200 with 18,034 bytes, and produced one control-plane session with pool/DNS/connect/TTFB and spill fields. The proxy, API, and origin ports were released afterward.

## Loop 90

Runtime setup:

- Evidence and storage directory: `/tmp/rsproxy-dogfood90.ilWBkd`. Its rsproxy CA issued the certificate used by an OpenSSL TLS origin on `127.0.0.1:18490`.
- Rsproxy ran on `127.0.0.1:19081`, API `127.0.0.1:19082`, with `--client-tls-handshake-timeout-ms 120`.

Observed:

- Status exposed `timeouts.client_tls_handshake_ms:120` alongside the existing DNS/TCP/upstream-TLS/TTFB deadlines.
- A normal curl HTTPS request negotiated client-side HTTP/2 through CONNECT/MITM and returned 200 with 4,373 bytes in 66ms. This established the normal path before fault injection.
- A raw client sent CONNECT, received `200 Connection Established`, then sent no ClientHello. Rsproxy closed it after 124ms. Trace session 2 recorded status 408, duration 122ms, `stage=client_tls_handshake: timeout after 120ms`, `client-timeout`, `client-tls-handshake-timeout`, and a failed `client_mitm_tls` record with the same stage and timing.
- A second normal curl HTTPS request returned 200 in 17ms after the timed-out client, proving the listener and certificate cache remained healthy. `rsproxy tui --once --filter client_tls_handshake` selected and rendered the timeout session consistently.
- Native JSON, HAR, and spill exports are `/tmp/rsproxy-dogfood90.ilWBkd/final.json`, `/tmp/rsproxy-dogfood90.ilWBkd/final.har`, and the directory's trace segment. All fixture/proxy/API ports were released afterward.
- Dogfooding exposed that `trace ls --limit 1` silently ignored the long option and only honored `-n`; after the fix, the same live command returned exactly the latest session.

Optimization from observation:

- Added `--client-tls-handshake-timeout-ms`, defaulting to 10 seconds, with positive-value validation, help text, and status visibility.
- Added a dedicated downstream TLS deadline reader/writer. Every rustls read, write, and flush receives only the absolute deadline's remaining time; original socket timeouts are restored on success and failure.
- Timeout errors preserve `TimedOut` and receive client-specific trace classification; protocol errors preserve their original `io::ErrorKind` and existing 502 behavior. A focused network test covers silent-peer timing and timeout restoration.
- Added the natural `--limit` alias for `trace ls` and a focused CLI regression test.

## Loop 91

Runtime setup:

- Evidence and storage directory: `/tmp/rsproxy-dogfood91.qYsECM`. Its rsproxy CA signed a programmable TLS HTTP/1.1 origin on `127.0.0.1:18591`; a second fixture on `127.0.0.1:18593` accepted an upstream HTTP-proxy CONNECT request and then stayed silent.
- Rsproxy ran on `127.0.0.1:19191`, API `127.0.0.1:19192`, with `--request-timeout-ms 120`, a 1-second upstream TLS deadline, and a 1-second TTFB deadline. Status exposed `timeouts.request_total_ms:120`.

Observed:

- Normal HTTPS baselines returned 200 with 7 bytes: forced client HTTP/1.1 completed in 98ms on the first certificate path, and client HTTP/2 completed in 18ms. A post-fault HTTP/2 request completed in 16ms.
- The origin sent response headers immediately and delayed its four-byte body by 350ms. Forced client h1 returned 504 in 131ms; client h2 through MITM returned 504 in 135ms. Both bodies and trace errors were exactly `stage=request_total: timeout after 120ms`; trace carried `request-timeout` / `request-total-timeout`, and the h2 request retained `h2-client`.
- A matched `delay(req, 300ms)` request returned the same 504 in 137ms without contacting the origin. A real SSE request sent its first event after 250ms and still returned 200 with `data: late` in 274ms, proving the deadline is removed after the streaming response is established.
- CONNECT passthrough routed through the silent HTTP proxy returned 504 in 124ms. Trace recorded a 121ms tunnel session with `upstream-timeout`, both request-total flags, and the exact error. Dogfooding exposed that failed CONNECT setup omitted its planned upstream label; after the fix, a repeated 122ms timeout recorded `proxy://127.0.0.1:18593->127.0.0.1:19591`.
- TUI filtering by `request_total` rendered all four timeout sessions and selected the CONNECT detail consistently. JSON and spill retained all errors/flags; HAR contained the three HTTP 504 entries but, as before, does not carry trace error/flag extensions. Evidence includes `final.json`, `final.har`, `spill.ndjson`, per-session JSON, curl bodies/headers, and `tui.txt`. All fixture/proxy/API ports were released.

Optimization from observation:

- Added a shared monotonic `RequestDeadline` and effective stage budgets. Stage-specific timeouts remain authoritative when shorter; when the total remainder is shorter, blocking socket timeouts and Tokio cancellation map to one exact `request_total` error.
- The deadline now spans request rule delay, h1/h2 pool admission and readiness, DNS, TCP, proxy/SOCKS negotiation, upstream TLS and protocol handshakes, TTFB, and complete buffered response collection. Manual I/O recomputes the remainder before every read/write; Hyper h1/h2 wrap readiness, handshake, response-head, and body futures.
- Established SSE, WebSocket, and CONNECT tunnels restore regular I/O behavior. An h2 stream cancelled by request-total timeout does not evict its healthy shared connection; h1 releases its exclusive lease.
- Added focused tests for request-delay 504/trace behavior, manual h1 body timeout, pooled h1 body timeout and lease release, pooled h2 body cancellation plus connection reuse, and CONNECT proxy setup. Split h2 timeout tests into `upstream_h2/tests/timeouts.rs` to keep every Rust file below 500 lines.
- Final gates passed formatting, the Rust line-count guard, all-target compilation, and the full workspace test suite: 116 CLI unit tests, 38 rules unit tests, 6 trace unit tests, and 3 integration tests. The largest Rust file remains 487 lines. Clippy remains unavailable in the installed toolchain.

## Loop 92

Runtime setup:

- Evidence and storage directory: `/tmp/rsproxy-dogfood92.AgMIzt`. A programmable TLS HTTP/1.1 origin ran on `127.0.0.1:18592`; rsproxy ran on `127.0.0.1:19193`, API `127.0.0.1:19194`, with a 120ms request-total deadline.
- Curl used client HTTP/2 through CONNECT/MITM. The origin exposed an immediate seven-byte response and a response that sent headers immediately but delayed its four-byte body by 350ms.

Observed:

- The initial slow request returned 504 over h2 and retained `stage=request_total: timeout after 120ms`; two following fast requests returned 200 in 17ms and 8ms, proving post-timeout recovery and h1 upstream pool reuse.
- The first HAR export proved RFC 3339 UTC timestamps, client `HTTP/2`, standard `timings.ssl`, full TLS arrays, request-total flags/error, and balanced standard timings. It also exposed that client MITM TLS records attached to h2 streams predate the stream request timeline and must not reduce stream `unattributed_ms`.
- After the timing-model fix and restart, a repeated slow request returned 504 over h2 in 159ms end-to-end with a 122ms proxy session; fresh and pooled fast requests returned 200 in 9ms and 6ms. The exported repeated query `q=hello%20world&x=1&x=2` became ordered values `hello world`, `1`, `2`.
- A strict jq assertion over `final-post.har` verified all three entries: RFC 3339, HTTP/2, standard timing sum equals `time`, `_rsproxy` detail timing sum equals `time`, exactly one request-total error, ordered query values, fresh `ssl:1`, and pooled `ssl:-1`. Native JSON remained available as `final-post.json`. All fixture/proxy/API ports were released.

Optimization from observation:

- Replaced manual HAR string assembly with structured `serde_json::Value` construction and added RFC 3339 formatting through `time`. This removes escaping/comma risks while leaving native JSON and spill contracts unchanged.
- Added standard upstream TLS `ssl` timing and `_rsproxy` diagnostics containing session identity, routes, flags, errors, redacted rules, TLS records, frame count, and detailed timing fields.
- Added `recorded_tls_ms`, `timeline_tls_ms`, and `client_tls_in_timeline`; h2 client handshakes remain visible but no longer distort stream timing. Standard and detailed residual timings now close independently against total session time.
- Added ordered percent-decoded HAR `queryString` values, h2 protocol labeling, focused parsed-JSON tests, and a dedicated `json/har.rs` module after the 500-line guard correctly rejected a 502-line intermediate file.
- Final gates passed formatting, the Rust line-count guard, all-target compilation, and the full workspace suite: 118 CLI unit tests, 38 rules unit tests, 6 trace unit tests, and 3 integration tests. Clippy remains unavailable in the installed toolchain.

## Loop 93

Runtime setup:

- Evidence and storage directory: `/tmp/rsproxy-dogfood93.LxGTMi`. The foreground TCP instance used proxy `127.0.0.1:19203` and API `127.0.0.1:19204`; the Unix instance used proxy `127.0.0.1:19205` and `/tmp/rsproxy-dogfood93.LxGTMi/run/control.sock`.
- A separate daemon lifecycle check used proxy `127.0.0.1:19206`, API `127.0.0.1:19207`, and `/tmp/rsproxy-dogfood93.LxGTMi/daemon`.

Observed:

- TCP startup automatically created `<storage>/run/api-token` as exactly 64 hex characters with mode 0600 and printed only its path. Status reported `api_auth.mode:token` without exposing the token.
- Curl without credentials and with a wrong Bearer token both returned 401; the response included `WWW-Authenticate: Bearer` and `{"error":"unauthorized"}`. A correct Bearer token returned 200.
- `rsproxy status --api ... --storage ...` discovered the token file automatically; `--api-token` worked without storage discovery. An unauthenticated rules POST returned 401, an authenticated CLI rules set succeeded, a second unauthenticated overwrite returned 401, and authenticated `rules cat` still showed only `example.test status(209)`.
- The Unix control socket was mode 0600. CLI and curl over the socket worked without a token, reported `api_auth.mode:peer`, and retained the existing rule set.
- Real daemon `start` generated the token before spawn, the authenticated readiness probe succeeded, CLI status returned 200, and `stop` released both ports. All foreground TCP/Unix and daemon ports were released.

Optimization from observation:

- Added mandatory pre-routing auth for TCP control APIs with case-insensitive Bearer parsing, `X-Rsproxy-Token` support, constant-work comparison, explicit 401 responses, and no unauthenticated status exception.
- Added 256-bit CSPRNG token generation, secure storage/reuse/explicit rotation, positive token validation, startup token-path diagnostics, and non-secret `status.api_auth.mode` observability.
- Added process-global CLI auth configuration so every existing API-backed command and the TUI reuse one header injection path. Discovery supports explicit CLI, environment, and storage file priorities; Unix transport clears token state.
- Added tests for auth header positive/negative cases, Bearer challenge output, request construction, CLI validation, token generation length, 0600 permissions, restart reuse, explicit override, and Unix peer mode.
- Final gates passed formatting, the Rust line-count guard, all-target compilation, and the full workspace suite: 122 CLI unit tests, 38 rules unit tests, 6 trace unit tests, and 3 integration tests. Clippy remains unavailable in the installed toolchain.

## Post-Loop 93 Structural Refactor

This was a code-organization pass, not a new Dogfooding loop. No new product
behavior was introduced.

- Converted `rsproxy-rules` and `rsproxy-trace` to thin public facades with
  separate models, parsers/resolvers, stores, spill lifecycle, path/index and
  codec responsibilities.
- Split the CLI control plane into transport, authentication, request routing
  and resource handlers; split TUI state, terminal rendering and pure formatting.
- Isolated h1/h2 connection-pool state machines from dispatch and request/response
  processing. Split proxy route models from route planning and transformation
  content/framing/SSE helpers.
- Standardized white-box tests under `src/<module>/tests/`, retained public
  black-box tests under each crate's `tests/`, and split the largest CLI, rules
  action and timeout suites by behavior. Added a direct control-route contract
  test for status and rule mutation/error paths.
- Added the repository README, architecture map and explicit Rust test-layout
  documentation. The 500-line guard covers production and test sources.
- Final gates passed formatting, the line-count guard, warning-free all-target
  compilation and the complete workspace suite: 123 CLI unit tests, 38 rules
  unit tests, 6 trace unit tests and 3 integration tests. The largest Rust source
  file is 431 lines. Clippy remains unavailable in the installed toolchain.

## Loop 94

Runtime setup:

- Evidence and storage directory: `/tmp/rsproxy-dogfood94.F7XIqT`. A release daemon used proxy `127.0.0.1:19300`, authenticated API `127.0.0.1:19301`, and a programmable HTTP/1.1 origin on `127.0.0.1:19302`.
- The explicit TOML file set a 1MB body aggregation limit, 4KB trace preview, disabled spill, and a 120-second request deadline. The large endpoint emitted 64MB in 64KB chunks; curl applied an 8MB/s downstream limit to create sustained backpressure.

Observed:

- `start --config` loaded every runtime value, status exposed the actual config path and `body_buffer_limit:1048576`, API token auth succeeded, and a small proxied response used chunked downstream framing.
- Baseline daemon RSS was 15,472KB. Curl received exactly 67,108,864 bytes in 7.998 seconds; five one-second RSS samples were all 23,472KB, showing no growth with bytes transferred. Trace recorded the exact byte count, `response-streamed`, an h1 pool hit, and only the configured 4KB preview.
- With `res.body.append("!") res.trailer(x-rule-end: yes)`, the small response became `small-body!`. A chunked origin response became `hello!` and preserved both `x-origin-end: done` and the rule trailer.
- The same body rule on 64MB exceeded the 1MB limit. Curl still received exactly 67,108,864 unmodified bytes; trace recorded `body-rewrite-skipped-limit`, `response-streamed`, the rule, the trailer, and a 4KB preview without an error.
- `restart --body-buffer-limit 2mb` proved CLI-over-TOML precedence through `status.body_buffer_limit:2097152`; the persisted rule remained loaded. Daemon and origin shutdown released all three ports.

Optimization from observation:

- Added one bounded `Bytes` frame channel for Hyper h1/h2 responses. Response heads return before body completion, while h1 connection leases and h2 stream leases remain owned by producer tasks until body/trailers end.
- Ordinary HTTP/1.1 responses now stream with reconstructed chunked framing, bounded trace tee capture, response throttling, and trailer preservation. A body failure after the response head closes that response and records the session error instead of appending a second HTTP response.
- Response body actions aggregate only when needed under `body_buffer_limit` (default 8MB). Overflow preserves the already-read prefix, continues with the same stream, skips all body mutation, and emits an explicit trace flag. Small HTTP/1.0 responses retain length framing under the same bound; oversized ones fall back to close-delimited streaming.
- Added TOML/CLI/status support for the positive body limit and tests for bounded collection, overflow continuity, small-body mutation, post-head errors, 32MB multi-frame relay, and h1 lease lifetime.
- Final gates passed formatting, warning-free all-target compilation, the 500-line guard, release build, and the full workspace suite: 135 CLI unit tests, 38 rules unit tests, 6 trace unit tests, and 3 integration tests (182 total). The largest Rust file is 438 lines. Clippy remains unavailable in the installed toolchain.

## Post-Loop 94 Structural Optimization

No new Dogfooding loop was started. This pass changed module and test boundaries
without introducing new proxy behavior:

- Split HTTP/1 request handling into a thin request facade, request-head/framing
  validation, and request-body/chunk/trailer decoding. The HTTP facade now uses
  explicit re-exports instead of exposing every child symbol.
- Split upstream TLS connection support into handshake IO, TLS observation
  records, and DNS/TCP/TTFB timing budgets. Split TLS ownership into rule policy,
  rustls/trust-root configuration, and leaf-certificate lifecycle modules.
- Split upstream response completion into buffered and streaming writers. Split
  forwarding into pool/protocol selection and a dedicated manual h1 path, and
  moved HTTP session initialization out of the request lifecycle orchestrator.
- Reorganized large white-box suites by behavior: h1 message/pool/timeout/trailer,
  response content/framing/header actions, single-hop/proxy-chain routing, and
  request-period/response-period rule conditions. Public black-box tests remain
  in each crate's standard `tests/` directory.
- Expanded `docs/architecture.md` and `docs/testing.md` with the concrete module
  and test trees, visibility rules, and the distinction between private unit
  tests and public integration tests.
- Removed unconnected request-streaming prework so this refactor remains
  warning-free and does not silently start the next feature loop.
- Final gates passed formatting, warning-free all-target compilation, the
  500-line guard, release workspace build, and all 182 tests. The largest Rust
  file is now 396 lines; no Rust source or test file exceeds 400 lines.

## Loop 95

Runtime setup:

- Evidence and storage directory: `/tmp/rsproxy-dogfood95.8aVjVI`. A release
  daemon used proxy `127.0.0.1:19400`, authenticated API `127.0.0.1:19401`, a
  backpressure-aware HTTP/1.1 origin on `127.0.0.1:19402`, and an rsproxy-CA
  signed TLS HTTP/1.1 origin on `127.0.0.1:19403`.
- The explicit TOML file set a 1MB body aggregation limit, 4KB full trace
  preview, disabled disk spill, and a 180-second request deadline. CLI CA,
  lifecycle, status, rule check/set/stats/test, trace export and TUI commands all
  used the same isolated configuration.

Observed:

- Curl uploaded exactly 67,108,864 Content-Length bytes under origin
  backpressure in 11.026 seconds. Origin SHA-256 was
  `3b6a07d0d404fab4e23b6d34bc6696a6a312dd92821332385e5af7c01c421351`,
  identical to the source. Daemon RSS moved from 11,120KB to 11,728KB and all
  ten later samples stayed at 11,728KB. Trace kept only the configured 4KB
  prefix, exact request byte count, `request-streamed`, `expect-continue`, and no
  error or upstream-pool flag.
- With `req.header(x-kept: yes) req.body.append("!")`, a five-byte request was
  buffered and reached origin as `small!`. A 2MB request stayed byte-for-byte
  unchanged while the header action and a response status rule still applied;
  its body condition did not match and trace recorded
  `request-body-rewrite-skipped-limit`.
- A raw chunked upload crossed the 1MB boundary by one byte, preserved all
  1,048,577 decoded bytes and SHA-256
  `7cabacb90701f3e9ca8d198e6bfb7e42467b56e4880e30483dbfc83524efc9b6`,
  and delivered `X-Upload-End: done` to origin. Export retained the trailer,
  exact byte count and streaming/limit flags.
- HTTPS CONNECT/MITM uploaded the same 2MB fixture in 61ms curl time. Origin
  received the exact source hash and `x-kept: yes`, but no upstream `Expect`;
  trace duration was 54ms with separate client and upstream TLS 1.3 records,
  `mitm`, `request-streamed`, and the expected body-rule limit flag.
- `tui --once` and native JSON export exposed the same five sessions, byte
  counts, rules, flags, trailers and TLS records. Daemon and both origins shut
  down cleanly; ports `19400` through `19403` were released.

Implementation and optimization validated:

- Split HTTP/1 request parsing into head/framing validation and a stateful body
  reader with bounded collection, fixed/chunked continuation and trailer
  preservation. Request reads now consume the absolute request-total deadline.
- Added candidate-aware rule body planning and bodyless resolution. Oversized
  requests retain body-independent matches/actions while body conditions and
  mutations are skipped explicitly instead of forcing unbounded aggregation.
- Added a dedicated request-stream relay with trace tee capture, exact byte
  accounting, Content-Length/chunked framing, trailers, local 100 Continue, and
  isolated manual origin h1 forwarding. Small bodies keep existing pooled h1/h2
  behavior.
- Added six real-network request-streaming tests covering fixed and chunked
  uploads, keep-alive reuse, 100 Continue, pre-body proxy auth, slow-upload
  deadline handling, and small/oversized rule behavior, plus parser and rule
  planning unit coverage.
- Final gates passed formatting, warning-free all-target compilation, the
  500-line guard, release workspace build, and all 193 tests. The largest Rust
  file is 432 lines. Clippy remains unavailable in the installed toolchain.

## Post-Loop 95 Structural Optimization

No new feature or Dogfooding loop was started. This pass completed the requested
module and test-boundary cleanup:

- Split proxy server admission from CONNECT routing and MITM TLS/inner HTTP
  sessions. `server.rs` now owns only listener/plain-client orchestration, while
  `server/{connect,mitm,request}.rs` own independent protocol responsibilities.
- Split request orchestration from upstream completion. `http_flow.rs` now owns
  request-period execution; `http_flow/{pending,session,completion}.rs` own body
  planning, session initialization, and success/error/timeout attribution.
- Split upstream h2 pool admission from its connection lifecycle. The facade now
  owns API models and single-flight dispatch, while
  `upstream_h2/{message,pool,connection}.rs` independently own wire conversion,
  lease/connector state, and handshake/send/TTFB/body pumping.
- Split WebSocket policy from transport execution. `websocket.rs` now owns frame
  state, upgrade detection and mode selection; `websocket/{nonblocking,
  concurrent}.rs` independently own TLS/MITM polling and plain-TCP bidirectional
  thread lifecycles, while `websocket_frame.rs` remains the frame codec/trace
  projection boundary.
- Split CA command orchestration from certificate construction, filesystem
  state, and native trust operations. `ca.rs` now owns only CLI workflows;
  `ca/{certificates,storage,trust}.rs` own rcgen/fingerprint logic, path/private
  key handling, and platform keychain lifecycle respectively.
- Replaced the downstream `h2.rs` implementation with a 19-line facade.
  `h2/server.rs` owns Hyper service and blocking proxy bridge lifecycle,
  `h2/message.rs` owns pseudo/header/trailer/body conversion, and `h2/runtime.rs`
  owns the shared Tokio runtime plus rustls readiness adapter.
- Replaced the rules `matcher.rs` implementation with an 8-line facade.
  `matcher/{pattern,action,condition,url}.rs` independently own URL matching and
  captures, action family/stacking metadata, condition evaluation, and the
  public `UrlParts` model.
- Split the largest remaining test module by behavior. `upstream_h2/tests/mod.rs`
  now contains only shared deadline setup; message conversion, real pooled gRPC
  transport, pool/connector admission, and timeout scopes live in separate
  `message`, `connection`, `pool`, and `timeouts` files with explicit imports.
- Split upstream h1 admission from connection execution. The facade now owns API
  models and error classification; `dispatch.rs` owns lease/checkout/stale
  fallback, while `connection.rs` owns handshake, readiness, TTFB, body pumping
  and reusable-connection check-in. Its pool tests now use an independent test
  key counter instead of coupling to the production connection generation.
- Split HTTP/1 request body state from collection and trailer parsing.
  `request/body.rs` owns fixed/chunked reader transitions,
  `request/body/collect.rs` owns bounded/all aggregation and overflow continuity,
  and `request/body/trailers.rs` owns chunk-line limits and trailer validation.
- Split rule-command dispatch from request construction and benchmarking.
  `cli/rules.rs` now owns subcommand routing and rule-source fallback,
  `cli/rules/request.rs` owns request metadata parsing and API query construction,
  and `cli/rules/bench.rs` owns local benchmark execution and percentile output.
- Moved the final inline upstream-body tests to `src/upstream_body/tests.rs`. A
  repository-wide scan confirms every `#[test]` now lives under a module test
  path or a crate-level public `tests/` directory. Added
  `scripts/check-test-layout.sh` to enforce this layout and require a public
  integration-test directory for every crate.
- `http_flow.rs` fell from 432 to 267 lines and `server.rs` from 415 to 140 lines.
  `upstream_h2.rs` fell from 396 to 193 lines, with connection/message/pool files
  between 194 and 253 lines. `websocket.rs` fell from 394 to 104 lines, with its
  transport modules at 98 and 208 lines. `ca.rs` fell from 393 to 173 lines,
  with certificate/storage/trust children between 49 and 271 lines. Downstream
  `h2.rs` fell from 373 to 19 lines, with children between 43 and 230 lines. The
  rules `matcher.rs` fell from 362 to 8 lines, with children between 65 and 117
  lines. `upstream_h2/tests/mod.rs` fell from 358 to 11 lines, with behavior
  files between 54 and 178 lines. `upstream_h1.rs` fell from 348 to 97 lines,
  with dispatch/connection/message/pool children between 55 and 207 lines. The
  request `body.rs` fell from 341 to 123 lines, with collect/trailer children at
  75 and 158 lines. CLI `rules.rs` fell from 323 to 109 lines, with request and
  bench children at 166 and 59 lines. The largest Rust file in the entire
  workspace is now 322 lines, so no production or test source reaches 400 lines.
- Final gates passed formatting, warning-free all-target compilation, the
  500-line guard, release workspace build, and all 193 tests. Clippy remains
  unavailable in the installed toolchain.

## Post-Loop 95 M1 Rule Group Closure

No new Dogfooding loop or proxy traffic was started. This implementation batch
closed the ordered multi-group runtime gap identified by the completion audit:

- Added `RuleSet::parse_groups`, preserving group order before line order while
  retaining global `@important` precedence and group-aware parse errors.
- Added an ArcSwap-backed `RuleStore`. `rules/groups.toml` persists order and
  enabled state, legacy `*.rules` directories are discovered deterministically,
  and invalid updates leave both the published snapshot and group file intact.
- Requests retain one Arc snapshot from body planning through response-period
  rule evaluation, so an update cannot split one exchange across generations.
- Added CLI/API `ls/cat/edit/set/rm/enable/disable` support for named groups.
  Online export and offline fallback make `rules test`, `stats`, and `bench`
  evaluate the complete enabled group set.
- Split snapshot coordination from manifest/filesystem code into
  `rule_store.rs` and `rule_store/storage.rs`, both below 230 lines, with
  dedicated store, control-route, rule-engine, and executable CLI tests.
- Final gates passed formatting, warning-free all-target compilation, both
  structure guards, all 203 tests, and the release workspace build. The largest
  Rust file remains 322 lines. Clippy remains unavailable in the installed
  toolchain.

## Post-Loop 95 M1 Corpus And Error Contract

No new Dogfooding loop or proxy traffic was started. This batch made the rules
contract independently executable:

- Added stable `syntax`, `matcher`, `action`, `condition`, and `property` error
  codes alongside group, line, and message. Control API parse failures now emit
  structured objects instead of requiring clients to parse display strings.
- Added 43 TOML cases under `rsproxy-rules/tests/corpus/`, organized by matcher,
  condition, composition, and error behavior. The public API runner validates
  action families, matched group/line provenance, response-period resolution,
  rendered explain output, and parse-error fields.
- Bound 16 representative corpus cases bidirectionally to
  `docs/rules-dsl-spec.md`; missing or orphaned `<!-- corpus:id -->` anchors fail
  the corpus test.
- Added explicit IPv6 literal, punycode, default-port, path-boundary,
  cross-group priority, `@important`, stack/skip, response-condition, and all
  five error-stage cases. Full action-option and whistle-migration matrices,
  proptest, and fuzz reuse remain open M1 work.
- Final gates passed formatting, warning-free all-target compilation, both
  structure guards, all 205 tests, and the release workspace build. The largest
  Rust file remains 322 lines.

## Post-Loop 95 Header And Host Action Contract

No new Dogfooding loop or proxy traffic was started. This static implementation
batch closed two additional M1 action gaps:

- Added `req.header` / `res.header` regex replacement with
  `/regex/replacement` syntax, escaped slash handling, duplicate-value updates,
  stack ordering, parse-time regex validation, and cached compiled patterns.
- Added per-rule `host(addr, addr...)` round-robin pools. Atomic cursors are
  shared across concurrent request resolutions, while each resolved action
  caches one lazy selection so planned trace routes and actual forwarding cannot
  advance to different targets.
- Split the stateful rule primitives into `action/host_pool.rs` and
  `action/replace_pattern.rs`, and centralized header application in
  `proxy/transforms/headers.rs`.
- Expanded the rules corpus to 46 cases and 19 bidirectional specification
  anchors. Dedicated rule and proxy tests cover parsing, invalid regexes,
  escaped delimiters, commas, stack order, duplicate headers, streaming
  responses, cursor isolation, concurrent balance, and route-selection reuse.
- Final gates passed formatting, warning-free all-target compilation, both
  structure guards, all 217 tests, and the release workspace build. The largest
  workspace source file remains 322 lines. No Dogfooding loop was added.

## Post-Loop 95 V1 Template Contract

No new Dogfooding loop, proxy process, CLI runtime request, or curl traffic was
started. This static batch closed the v1 template-variable contract:

- Added stable per-request metadata for `id`, `now`, `random`, and UUID v4, plus
  URL port, request/response headers, status code, and request/response cookies.
  All 20 variables from section 6.3 now render through one implementation.
- Response-period actions share one immutable `Arc<ResponseMeta>`, avoiding a
  response-header clone per action while keeping original response values stable
  across stacked actions.
- Added `${var.replace(/regex/, replacement)}` with optional `i`, numbered and
  named replacement captures, parse-time validation, escaped delimiters, and a
  bounded thread-local compiled-regex cache. Regex matcher `$0` now correctly
  means the complete match instead of aliasing `$1`.
- Split template metadata, rendering, and transform parsing into dedicated
  `template/` modules. Action template validation remains in the action layer;
  proxy modules call `ResolvedAction::render` without carrying response context.
- Extended `rules test` with `--response-status` and repeatable
  `--response-header`, supported by both the control API and offline fallback.
- Expanded the executable corpus to 49 cases and 22 specification anchors.
  Dedicated rule, proxy, control-route, CLI-option, and executable-help tests
  cover the new contract.
- Static gates passed during implementation; the final workspace total is 227
  tests. No Dogfooding loop was added.

## Post-Loop 95 External Rule Watcher

No new Dogfooding loop, proxy process, CLI runtime request, or curl traffic was
started. This static batch closed the external rule-file watch contract:

- Added opt-in `--watch` / TOML `watch = true` and configurable positive
  `--watch-debounce-ms` / `watch_debounce_ms`, defaulting to 200ms.
- Split watcher ownership into `rule_store/watch.rs`. A notify callback filters
  unrelated events and uses `try_send` into a capacity-64 queue; the worker
  performs trailing-edge debounce and reloads the complete rules directory.
- A valid batch compiles every group before one ArcSwap publication. Invalid
  files retain the previous snapshot; later valid edits recover automatically.
  API-generated duplicate events skip publication when group state is equal.
- `/api/status.rule_watch` exposes enabled/debounce state, event and dropped
  event counts, successful reloads, failures, last reload time, and last error.
- Four dedicated watcher tests cover atomic rollback/no-op publication,
  real filesystem reload and recovery, bounded queue filtering, zero debounce,
  and clean worker shutdown. The final workspace total is 231 tests.
- Final gates passed formatting, warning-free all-target compilation, both
  structure guards, all 231 tests, and the release workspace build. The largest
  Rust source file is 336 lines; Clippy remains unavailable in this toolchain.
- This section records implementation and automated tests only. It is not a new
  Dogfooding loop.

## Post-Loop 95 Rule Property And Fuzz Contract

No new Dogfooding loop, proxy process, CLI runtime request, or curl traffic was
started. This static batch strengthened the M1 executable contract:

- Added `Action::FAMILIES` with 45 stable family names. The action corpus now
  declares that complete set, and the public runner compares implementation,
  declaration, and actual resolved coverage.
- Expanded the TOML corpus from 49 to 62 cases. The action file now exercises
  every family, stackable set/remove/replace forms, value references, file and
  inline values, templates, body operations, inject modes, flow control, TLS,
  control actions, and representative invalid parameters.
- Added three 256-case proptest properties for valid-rule AST/resolution
  reparse stability, structured errors from near-valid mutations, and bounded
  arbitrary UTF-8 traversal across parse/resolve/explain APIs.
- Added a cargo-fuzz `parse_resolve` target and seven valid/invalid seeds. The
  seed integration test and libFuzzer target import the same harness.
- Installed cargo-fuzz 0.13.2 plus a minimal nightly without changing the
  default stable toolchain. A nightly ASan/libFuzzer smoke completed 1000 runs
  without a crash. The reusable script runs against a temporary corpus; its
  100-run verification also passed without changing the seven checked-in seeds.
- This closes the missing property/fuzz entry point, not the complete M1 action
  contract: every action still needs all seven value/capture/error forms and a
  whistle migration matrix. No Dogfooding loop was added.
- Final gates passed workspace and fuzz formatting, warning-free all-target
  compilation, both structure guards, all 235 tests, fuzz-package compilation,
  shell validation, and the release workspace build. The largest Rust source
  file remains 336 lines.

## Post-Loop 95 Structured Value And Module Boundary Refactor

No new Dogfooding loop, proxy process, CLI runtime request, or curl traffic was
started. This static refactor completed the current code-splitting round:

- Replaced stringly action operands with `Value::{Inline, File, Reference}`
  across routing, redirects, request/response metadata, cookies, CORS, cache,
  URL transforms, host pools, attachments, and trace tags. TLS certificate/key
  path operands and inline regex replacement grammar remain intentionally
  distinct.
- Split the public value model/key grammar into `rsproxy-rules/src/action/value.rs`
  and centralized runtime text/byte resolution in
  `rsproxy-cli/src/proxy/transforms/values.rs`. Parser and runtime both reject
  invalid value keys; text actions reject non-UTF-8 input while body/inject/mock
  paths preserve binary bytes.
- Preserved URL regex replacement `$1`/`${name}` semantics through a dedicated
  raw-value resolver. Loaded UTF-8 values and files otherwise render request,
  response, numbered, and named capture context at execution time.
- Added a 40-slot public value matrix with inline, template/capture, `@key`, and
  `<file>` sources (160 parse combinations), public key/source boundary tests,
  and five proxy runtime tests spanning request, response, URL, routing, mock,
  body, trace, UTF-8, binary, and path-traversal behavior. The TOML corpus now
  contains 64 cases.
- Final gates passed formatting, warning-free all-target compilation, both
  structure guards, all 244 tests, fuzz-package compilation, a 100-run
  ASan/libFuzzer smoke, and the release workspace build. Every Rust file remains
  at or below 500 lines; the maximum is 336 lines.
- This section records implementation and automated tests only. It is not a new
  Dogfooding loop.

## Post-Loop 95 M1 Runtime, Migration, And Fuzz Contract

No new Dogfooding loop, proxy process, CLI runtime request, or curl traffic was
started. This static batch extended the M1 executable contract:

- Added a source-backed Whistle migration matrix with 45 supported mappings,
  exactly covering `Action::FAMILIES`, plus 10 explicitly deferred or removed
  capabilities. Its runner verifies the pinned Whistle documentation/unit fixture,
  parses every rsproxy counterpart, and rejects family-set drift.
- Added a seven-form runtime resolver matrix for every one of the 40 structured
  value fields: basic, quoted, `@key`, `<file>`, template, numbered/named
  capture, and invalid reference. This adds 280 field/source checks while the
  existing proxy behavior suite continues to verify actual category effects.
- Added deterministic complexity tests at the 64KB fuzz input ceiling for a
  large inline value, many-rule input, malformed delimiters, fancy-regex
  backtracking, and 8x input scaling.
- Extended the sanitizer smoke script with validated run-count, duration, and
  max-length controls. Added an eighth reviewable structured-value seed; both
  count-based and duration-based ASan/libFuzzer smoke modes passed without
  modifying the checked-in corpus.
- Final gates passed formatting, warning-free all-target compilation, both
  structure guards, all 249 tests, fuzz-package compilation, shell validation,
  and the release workspace build. The largest Rust source file remains 336
  lines.
- M1 remains partial: complete per-family network effects, uncommon Whistle
  option/alias parity, and scheduled continuous fuzz are still open. This is not
  a new Dogfooding loop.

## Post-Loop 95 Trace Event Collector And Structural Closure

No new Dogfooding loop or curl traffic was started. This batch completed the
current trace/code-splitting round and then ran a dedicated automated release
resource acceptance process:

- Split trace configuration, counters, pending aggregation, completed-memory
  storage, follow receiver, stats fallback, and collector worker into dedicated
  `store/` modules. Every Rust file remains below 500 lines; the maximum is 381.
- Added the public incremental `TraceEvent` lifecycle with bounded `Bytes` body
  chunks, authoritative final snapshots, final kind correction, idempotent
  aborts, orphan/incomplete accounting, and five-minute partial-session cleanup.
- Migrated HTTP request and response streaming producers to emit body events
  before completion. HTTP/2 shares `Bytes` slices; h1/SSE copies only the
  remaining configured preview. Deterministic tests prove both upload and
  response body previews are visible while the transfer is still open.
- Partitioned the configured total trace memory budget between queued events
  and resident pending/completed sessions. Queue reservation includes dynamic
  event storage and observed body-chunk size; stats expose all partitions and
  drop/partial counters.
- Replaced polling `trace follow` behavior with a close-delimited live NDJSON
  route backed by per-subscriber bounded queues, atomic backlog registration,
  heartbeat lines, and slow/disconnected subscriber handling. The polling
  endpoint remains compatible.
- Added event concurrency, metadata ordering, queue-drop snapshot correction,
  memory-pressure, live control-route, and executable CLI follow tests. Final
  gates passed formatting, warning-free all-target compilation, both structure
  guards, all 302 regular tests, fuzz-target compilation, a 1000-run sanitizer
  smoke, and the release workspace build.
- Added an ignored-by-default release black-box acceptance test and explicit
  script. Its final run moved 1,073,741,824 bytes from a real TCP origin through
  rsproxy to a chunk-decoding client in 260ms; trace bytes and 4KiB preview were
  exact, queue/partial counters stayed zero, and RSS grew from 16,720KiB to a
  27,296KiB peak, only 10,576KiB against a 96MiB limit.
- M3 remains partial because independent request-send/response-receive timings,
  CONNECT/tunnel lifecycle-event migration, collector-independent spill export,
  and runtime Dogfooding of the new event/follow path are still open. No
  Dogfooding loop was added.

## Post-Loop 95 Independent Transfer Timing Closure

No new Dogfooding loop, proxy CLI session, or curl traffic was started. This
static M3 batch closed the independent transfer-boundary implementation and
reran the automated release resource process:

- Added nullable `request_send_ms` and `response_receive_ms` to completed
  sessions, incremental `TraceEvent::End`, pending assembly, native/summary
  JSON, NDJSON spill, TUI timing output, and HAR diagnostics. Null remains
  distinct from a measured zero-millisecond boundary.
- Added a shared monotonic one-shot transfer timer. Hyper h1/h2 request bodies
  freeze it at EOF or drop; bounded h1/h2 response pumps freeze a second timer
  at body/trailer EOF or error. Manual h1 records the corresponding wire write
  and read boundaries directly, including SSE while leaving WebSocket tunnel
  receive timing intentionally unset after the handshake.
- HAR projects values into sequential standard `send`, `wait`, and `receive`;
  full-duplex h2 overlap is retained as exact extension values plus
  `transfer_overlap_ms`. Remaining rule/processing time is attributed to
  `blocked`, so standard timings close against total session time. Legacy or
  non-network sessions retain nullable diagnostics and residual receive fallback.
- Added deterministic one-shot/EOF/drop tests, delayed h1 and pooled h2 upload
  tests, delayed h1/h2 response tests, response-body-error coverage, and a
  response-head-timeout test that preserves completed send while leaving
  receive unknown.
- Final gates passed formatting, warning-free all-target compilation, both
  structure guards, 308 regular tests with one explicit resource test ignored
  by default, fuzz-target compilation, a 1000-run sanitizer smoke, and release
  workspace build. Every Rust file remains at or below 500 lines; the maximum
  remains 381.
- The final explicit release test moved 1,073,741,824 bytes in 248ms with exact
  trace accounting and 4KiB preview. RSS rose from 16,752KiB to 26,368KiB, a 9,616KiB
  increase against the 96MiB limit; queue drops and partial sessions stayed at
  zero.
- M3 remains partial only for CONNECT/tunnel lifecycle-event migration,
  collector-independent large spill export, and runtime Dogfooding of the new
  timing/event/follow paths.

## Post-Loop 95 CONNECT Tunnel Event Migration

No new Dogfooding loop, CLI runtime session, or curl traffic was started. This
static M3 batch removed the final proxy-side atomic Session producer:

- Passthrough CONNECT now starts a visible trace when the policy commits to a
  tunnel, emits the established response, and keeps the session pending until
  both copy directions finish. TCP and TLS-to-upstream copies emit bounded,
  nonblocking direction-aware byte events.
- Tunnel events intentionally carry empty body previews plus observed byte
  counts. Opaque/encrypted payloads are not retained, while final snapshots
  preserve exact totals if intermediate events are dropped by queue pressure.
- Failure, empty-connection, and MITM-handshake failure paths lazily create the
  same Start/Request/continuation lifecycle. A local abort guard prevents an
  unexpected socket write failure from leaving a live partial. `hide` never
  allocates a trace id.
- `record_session_if_visible` now upgrades any missed non-hidden id-zero session
  to the event lifecycle before finishing. No proxy production path calls the
  compatibility `TraceStore::record(Session)` API; it remains available only as
  a public compatibility surface.
- Added deterministic tests for in-progress pending visibility, exact duplex
  totals without payload previews, direction aggregation, connection refusal,
  hidden tunnels, TLS handshake timeout, and zero orphan/partial residue.
- Final gates passed formatting, warning-free all-target compilation, 313
  regular tests with one explicit resource test ignored by default, both
  structure guards, fuzz-target compilation, a 1000-run sanitizer smoke, and
  release workspace build. All Rust files remain below 500 lines.
- The explicit 1GiB release process completed in 273ms with exact trace bytes
  and 4KiB preview. RSS rose from 17,104KiB to 28,688KiB, an 11,584KiB increase
  against the 96MiB limit; queue drops and partial sessions remained zero.
- M3 now remains partial only for moving large spill export reads out of the
  collector owner and runtime Dogfooding of the tunnel/timing/event/follow paths.

## Post-Loop 95 Collector-Independent Spill Export

No new Dogfooding loop, proxy process, CLI runtime request, or curl traffic was
started. This static M3 batch removed disk scan/decompression from the trace
collector owner:

- `spill.ndjson` now obtains an ordered snapshot consisting of open segment and
  index handles plus captured byte lengths. The collector immediately resumes;
  the query caller performs file reads, index parsing, CRC verification, zstd
  decoding, corrupt-record filtering, and output assembly.
- Captured lengths make the export a stable point-in-time window: appends after
  the snapshot are excluded. Open handles preserve the captured window if
  `trace clear` or disk-budget rotation removes its paths while the read is in
  progress.
- A clear generation accompanies each snapshot. Corruption diagnostics from an
  older read are ignored after clear, so a delayed export cannot restore stale
  `spill_corrupt_records` state.
- Added deterministic tests that pause immediately after snapshot acquisition.
  While export is paused, record/stats complete through the collector; separate
  cases prove append isolation, clear survival, budget-eviction survival, and
  stale-report rejection. Existing CRC, zstd, restart and rotation tests remain
  unchanged and pass.
- Final gates passed formatting, warning-free all-target compilation, 316
  regular tests with one explicit resource test ignored by default, both
  structure guards, fuzz-target compilation, a 1000-run sanitizer smoke, and
  release workspace build. Every Rust file remains below 500 lines.
- The explicit 1GiB release process completed in 268ms with exact trace bytes
  and 4KiB preview. RSS rose from 17,088KiB to 27,808KiB, a 10,720KiB increase
  against the 96MiB limit; queue drops and partial sessions remained zero.
- M3 static implementation gaps are now closed. Runtime Dogfooding of the new
  tunnel/timing/event/follow/spill-snapshot paths remains required.

## Post-Loop 95 M0 Observability And Benchmark Closure

No numbered Dogfooding loop was added. This acceptance batch did start isolated
release origin/proxy processes and curl traffic solely to close the M0 logging
and runnable-benchmark requirements:

- Added `tracing`/`tracing-subscriber` behind `logging.rs`. Filters resolve as
  `RSPROXY_LOG` > `RUST_LOG` > `rsproxy=info`; text and JSON formats always write
  stderr. Stable events cover daemon/listener startup, trust roots, listener and
  connection errors, and session success/failure without exposing credentials.
- Added a real executable test that starts proxy and control listeners on port 0
  and parses stderr NDJSON. The first run exposed that the formatter defaulted to
  stdout, which would corrupt CLI JSON/NDJSON output; the implementation now
  explicitly uses stderr and the test verifies all startup/bound events.
- Added `bench_origin`, `bench_client`, `benches/e2e/benchmark.sh`, and
  `scripts/test-benchmark.sh`. The driver uses persistent concurrent h1 requests,
  exact response-byte accounting, latency percentiles, and a versioned
  `rsproxy-benchmark/v1` result. An initial run exposed that the proxy correctly
  reframes fixed-length origin responses as chunked downstream responses; the
  client was extended with bounded CL/chunked decoding instead of assuming CL.
- The contract run completed 128/128 requests and 131,072 bytes with zero status
  or IO errors after a 1,024-byte curl preflight. The default run completed
  1000/1000 requests and 1,024,000 bytes with zero errors in 158ms, reporting
  6,292.6 req/s, p50 2,260us and p99 14,778us. This proves M0 script execution;
  the unpinned one-off result is not §9.3 performance evidence.
- Final gates passed 319 regular tests with one explicit resource test ignored by
  default, warning-free all-target compilation, formatting, both structure
  guards, 1000 sanitizer fuzz runs, release workspace build, and shell syntax.
  The explicit 1GiB run completed exact transfer/trace bytes in 272ms; RSS grew
  from 17,408KiB to 30,000KiB (12,592KiB), with zero queue drops or partials.
- M0 is now complete. No Loop 96 was created; M1-M4 remain partial and M5 remains
  incomplete.

## Post-Loop 95 M1 Action Effect And Source Registry Closure

No numbered Dogfooding loop was added. This M1 acceptance batch used isolated
real TCP/TLS origin, proxy, and client fixtures rather than starting the next
manual CLI/curl loop:

- Added `proxy/tests/action_effects/`, whose owner registry must be an exact,
  duplicate-free partition of all 45 `Action::FAMILIES`. Fourteen executable
  tests observe request/URL mutation at the origin, response mutation at the
  client, local short circuits, real host/upstream/direct routing, timing,
  trace/control behavior, CONNECT bypass ClientHello forwarding, and upstream
  TLS policy through the production `handle_client` path.
- The first unified run exposed that streaming `throttle(res, ...)` recreated
  pacing state for every small body frame, so no frame reached the delay
  threshold. A shared monotonic `ThrottlePacer` now persists across response
  frames, SSE writes, and oversized request relay, while buffered paths reuse the
  same implementation. It honors absolute request deadlines and normalizes a
  programmatically constructed zero rate instead of panicking.
- Extended the Whistle migration runner to parse the exact protocol and alias
  declarations in the pinned fixture's `lib/rules/protocols.js`. All 74 canonical names
  and 22 explicit alias keys must now map to supported action/syntax entries or
  explicit deferred/removed classifications; the 45 supported action mappings
  still exactly cover `Action::FAMILIES`.
- Added `scripts/test-action-effects.sh` as one repeatable M1 entry point for the
  action corpus, Whistle migration/source registry, and real-network effect
  suite. Full gates pass 336 regular tests with one explicit 1GiB test ignored by
  default, formatting, warning-free all-target compilation, both structure
  guards, 1000 sanitizer runs, release workspace build, and shell syntax. The
  explicit 1GiB run transferred exact bytes in 243ms; RSS grew from 17,200KiB to
  26,528KiB (9,328KiB), with zero queue drops or partial sessions.
- This closes family-level network effects and source-name/alias omissions, not
  complete Whistle option semantics. Uncommon option-level behavior,
  matcher/condition edge corpus, and scheduled continuous fuzz remain M1 work.

## Post-Loop 95 Typed Delete And Whistle Option Contract

No numbered Dogfooding loop or manual daemon/curl round was added. This static
M1 batch converted the remaining option-level ambiguity into an executable
contract and implemented the common delete surface end to end:

- Auditing Whistle source showed that `enable` and `disable` are not one boolean
  action: their options span MITM, h2, compression, routing, Trace, plugins, UI,
  and file capture. `tests/contracts/whistle_options.toml` therefore classifies
  all 56 documented enable options, 66 disable options, and 16 delete categories
  as implemented, native-default, milestone-deferred, v2-deferred, or removed.
  Its runner extracts the source documents, rejects omissions/duplicates, and
  parses/resolves every implemented recipe.
- Added the 46th public family, `delete`, backed by typed `DeleteOp` and
  `DeletePathSegment` enums. The parser compiles pathname/all-or-indexed segment,
  all/named query, request/response header, named/all cookie, whole buffered body,
  MIME/charset, and named/all trailer operations. Unknown properties and nested
  body paths fail at parse time instead of becoming stringly no-ops.
- Split execution into `proxy/transforms/delete.rs`. URL, request, response, and
  trailer phases handle only their variants; path indexes in one call use the
  original path, negative indexes count from the end, and `pathname.last`
  preserves the trailing slash. Request-body planning removes only the body
  variant during over-limit degradation, so header/query effects still apply.
- Moved migration and option TOML contracts out of the rules case corpus into
  `tests/contracts/`, removing the old filename exception from the corpus runner.
  `scripts/test-action-effects.sh` now runs the 65-case corpus, 96-name source
  registry, 138-option contract, and 16 real TCP/TLS effect tests.
- Real origin/client tests prove deletion of URL path/query, request and response
  headers, individual/all cookies, request/response bodies, MIME/charset parts,
  and individual/all trailers. Final gates pass 345 regular tests with one 1GiB
  test ignored by default, warning-free all-target compilation, formatting, both
  structure guards, 1000 sanitizer runs, release build, and shell syntax. The
  explicit 1GiB transfer completed in 256ms; RSS grew from 17,280KiB to
  27,040KiB (9,760KiB), with exact trace bytes and zero queue drops/partials.
- Nested JSON/JSONP/form body-property deletion remains explicitly deferred, as
  do cross-protocol enable/disable options that have no exact current action.
  Matcher/condition edge expansion and scheduled continuous fuzz remain M1 work.

## Post-Loop 95 Matcher And Condition Contract Closure

No numbered Dogfooding loop, daemon, or manual curl round was added. This rules
batch made every documented matcher/condition shape executable and closed two
parser/phase defects found by the expanded corpus:

- Expanded the TOML corpus from 65 to 86 cases. New cases cover negated
  matchers, single/double-star path boundaries, query glob components, positive
  scheme and default-port equivalence, case-insensitive numbered regex captures,
  method/IP/status lists, header presence, URL glob, response-header presence,
  deterministic chance, environment fallback, nested any/not, and malformed
  matcher/condition parameters.
- Increased DSL-spec bidirectional anchors from 22 to 37 so every matcher row
  and every condition row is represented. Adding, removing, or renaming a
  documented form without a matching public case now fails the corpus runner.
- The edge matrix exposed that malformed `host:port`, bracketed IPv6 ports,
  empty schemes, and malformed exact URLs silently became broader/no-op
  matchers. The parser now validates RFC-style schemes and authorities, numeric
  or explicit wildcard port patterns, nonzero port ranges, and exact URLs before
  publication.
- Empty method/client-IP/server-IP/status lists, invalid method/header tokens,
  empty header contains values, invalid chance ranges, and empty env/any/body
  inputs now return stable `condition` errors. Quoted methods are normalized
  before token validation.
- `!status(...)` and `!res.header(...)` previously evaluated true during the
  request phase because the absent response first produced false and was then
  inverted. Response dependency now propagates through nested `Not`/`Any`; a
  negated response expression remains deferred until a response snapshot exists.
- Final gates still pass 345 regular tests with one explicit 1GiB test ignored
  by default, warning-free all-target compilation, formatting, both structure
  guards, 1000 sanitizer runs, and release build. The explicit 1GiB transfer
  completed in 252ms; RSS grew from 17,328KiB to 28,176KiB (10,848KiB), with
  exact trace bytes and zero queue drops/partials.
- M1 no longer has an unclassified matcher/condition corpus gap. Remaining M1
  work is the explicitly deferred option/body-path behavior and scheduled
  continuous fuzz; no Loop 96 was created.

## Post-Loop 95 Cross-Platform CI And Scheduled Fuzz Gate

No numbered Dogfooding loop, daemon round, or manual curl request was added. This
static engineering batch closed the missing CI/nightly scheduling surface while
keeping hosted execution evidence separate from local validation:

- Added `.github/workflows/ci.yml`. A fail-fast-disabled matrix runs locked
  workspace check, all-target tests, and release builds on Ubuntu, macOS, and
  Windows. A separate Ubuntu job owns formatting, Clippy, source/test/workflow
  guards, shell syntax, fuzz-target compilation, and the complete action-effect
  contract.
- Added `.github/workflows/fuzz.yml`. It installs pinned `cargo-fuzz 0.13.2` on
  nightly Ubuntu, replays the eight versioned seeds, fuzzes parse/resolve for 300
  seconds every day, and retains crash artifacts for 14 days only after failure.
- Added `scripts/check-workflows.sh`. It fixes the workflow inventory, parses YAML
  when Ruby is available, rejects tabs, mutable branch action refs and
  `continue-on-error`, and requires least-privilege contents access, released
  action majors, all platforms, triggers and gate commands.
- Introduced an all-target Clippy gate. Equivalent expression lints and test
  initializers were cleaned up across rules, trace, CLI and benchmark code;
  complex response tuples now have named aliases. The existing low-level
  protocol orchestration signatures retain one explicit project-level
  `too_many_arguments` exception rather than growing a hidden set of new local
  suppressions.
- The exact scheduled fuzz command ran locally for 300 seconds and completed
  463,561 executions with no crash or artifact. The temporary corpus left the
  eight checked-in seeds unchanged.
- Final gates pass 345 regular tests with one explicit resource test ignored by
  default, locked check/release build, Clippy, formatting, all three structure
  contracts, shell syntax and the 16 real-network action tests. The explicit
  1GiB release transfer completed in 244ms; RSS grew from 17,264KiB to 26,944KiB
  (9,680KiB), with exact trace bytes and zero queue drops/partials.
- The workflow definitions and their local equivalent commands are proven; the
  GitHub-hosted Ubuntu/macOS/Windows results are not yet available in this local
  non-repository workspace. No Loop 96 was created.

## Post-Loop 95 Nested Body Delete And Module Boundary Closure

No numbered Dogfooding loop, daemon round, or manual curl request was added.
This static refactoring and acceptance batch closed the documented nested-body
delete gap without starting Loop 96:

- Audited Whistle's English delete contract and local `parseKeys`/request/
  response inspector implementations. The resulting rsproxy contract covers
  request JSON and urlencoded form plus response JSON and JSONP, escaped dot and
  special-character keys, and trailing array indexes.
- Split the public model into `rsproxy-rules/action/delete.rs`, kept property
  compilation in `parser/delete.rs`, and isolated MIME-gated data-plane work in
  `proxy/transforms/delete/body.rs`. No Rust file exceeds 500 lines; the largest
  remains `proxy/h2_bridge/response.rs` at 381 lines, and all tests remain in
  dedicated module or crate-level test paths.
- Body paths are typed and bounded to 16KiB/128 segments. Missing paths,
  malformed JSON/UTF-8, incompatible media types, and compressed bodies are
  no-ops. Request and response paths reuse the existing bounded body planner, so
  overflow preserves the stream and skips only body-dependent effects.
- Extended the option contract from deferred to implemented, added parser/
  explain, planner, transform and negative tests, and added a real origin/proxy/
  client test that observes request JSON/form and response JSONP mutations plus
  corrected Content-Length. The aggregate now contains 17 real-network tests
  while retaining exact one-owner coverage of all 46 action families.
- Final gates pass formatting, Clippy with warnings denied, locked all-target
  check, fuzz-target compilation, release build, all structure/workflow/shell
  contracts, and 352 regular tests with one explicit resource test ignored by
  default. A post-change 60-second sanitizer run completed 121,726 executions
  with no crash or artifact and left all eight versioned seeds unchanged.
- The explicit release resource test transferred and traced exactly 1GiB in
  279ms. RSS rose from 17,312KiB to 28,096KiB, a 10,784KiB increase, with zero
  queue drops or partial sessions. No related process or fuzz artifact remained.
- The weighted completion estimate is now 86%, with about 14% remaining across
  explicit deferred options, protocol/runtime Dogfooding, native platform work,
  and M5 hosted/performance/release evidence. No Loop 96 was created.

## Loop 96

This loop resumed numbered Dogfooding after the code-splitting batch. It targeted
the remaining M3 runtime evidence with release binaries, real CLI processes and
curl rather than direct transform calls.

### Setup

- Rust benchmark HTTP origin on `127.0.0.1:18296` and an OpenSSL TLS origin on
  `127.0.0.1:18496`, using a root and leaf generated by the release rsproxy CLI.
- Release rsproxy on `127.0.0.1:18961`, authenticated control API on
  `127.0.0.1:18962`, isolated storage `/tmp/rsproxy-dogfood96`, no-MITM mode,
  1KiB trace previews, 2KiB zstd spill segments and a 32KiB disk budget.
- A persisted rule for the HTTP origin added `X-Dogfood-Loop: 96`,
  `Cache-Control: max-age=96` and `tag:loop96`; `rules set` and `rules test`
  exercised the authenticated CLI path before traffic.

### Observations

- Preflight exposed that `rsproxy run --help` started the default daemon and
  other subcommand help flags were treated as commands. The process was stopped
  immediately. Help is now centralized in `cli/help.rs` and intercepted before
  config, token, API, daemon or platform side effects. A watchdog-backed binary
  matrix covers lifecycle, status, rules, values, trace, TUI, replay, CA and
  system proxy commands; unknown commands remain errors.
- HTTP curl through the proxy returned 200, exactly 1,024 bytes and both rule
  headers. HTTPS curl received `200 Connection Established`, trusted the
  generated root, completed TLS directly to the origin and returned HTTP 200.
- A pre-existing `trace follow --count 2` received the HTTP session followed by
  a tunnel session. The HTTP record exposed pool/DNS/connect/send/TTFB/receive
  timing and 1,024 response bytes; the no-MITM tunnel exposed 489 request bytes,
  6,254 response bytes and no opaque body preview.
- Trace stats showed zero queue drops, partials, orphans, follower drops and
  spill errors. The zstd spill snapshot and JSON export contained both sessions;
  HAR correctly contained only the HTTP session. `tui --once` rendered timing,
  flags and the selected session, and `replay 1` returned exactly 1,024 bytes.
- After count-limited follow exited, stats still reported one subscriber until
  another session was published, and the normal close produced a WARN Broken
  pipe. `TraceFollow` now owns a strong liveness token while the worker holds a
  weak reference, so stats prunes closed subscribers immediately. Expected
  control client closes are debug events; genuine request errors remain WARN.
- Release re-verification proved a one-item follow exited with no subsequent
  publish, `follow_subscribers=0`, and no info-level disconnect warning.

### Regression And Optimization

- The first full run exposed a two-second help-test watchdog that was too tight
  under parallel binary startup; it is now ten seconds and still kills a daemon
  regression. A later full run exposed a pre-existing h1-to-h2 upload fixture
  race: after the response completed, a normal peer close could make graceful
  shutdown return Broken pipe. The fixture now accepts only explicit
  close/cancel peer I/O outcomes; all other h2 errors remain failures. The
  focused upload test then passed nine consecutive runs.
- Final gates pass formatting, strict Clippy, locked all-target check, fuzz-target
  compilation, release build, source/test/workflow/shell guards and 355 regular
  tests with one explicit resource test ignored by default.
- The explicit 1GiB release run transferred and traced 1,073,741,824 bytes in
  248ms. RSS rose from 17,248KiB to 27,664KiB, a 10,416KiB increase, with exact
  preview/accounting and no queue drop or partial session. The largest Rust file
  is 382 lines; no daemon, origin, fuzz process or crash artifact remained.
- M3 is now complete. The weighted overall estimate is 88%, with about 12%
  remaining in M1 deferred behavior, the M2 h2 protocol matrix, M4 native
  platform/schema work and M5 hosted/performance/release evidence.

## Loop 97

This loop closed the real-client HTTP/2 streaming evidence gap before the next
code-splitting batch. It used a release daemon, curl's forced h1/h2 modes and a
real TLS/ALPN h2 origin; no direct transform call was counted as runtime proof.

### Setup

- `nghttpd --echo-upload` listened with TLS and h2 only on
  `127.0.0.1:18497`, echoed request DATA, and emitted
  `X-Origin-Trailer: loop97`.
- Release rsproxy listened on `127.0.0.1:18963` with its authenticated API on
  `127.0.0.1:18964`, isolated storage `/tmp/rsproxy-dogfood97`, a 1MiB body
  aggregation limit, 4KiB trace previews and disk spill disabled.
- The rule for `h2.loop97.test` routed through
  `host(127.0.0.1:18497)`, appended a request-body marker when bounded,
  injected `X-Loop: 97`, and tagged the session. The upload fixture was 8MiB
  with SHA-256
  `2daeb1f36095b44b318410b3f4e8b5d989dcc7bb023d1426c492dab0a3053e74`.

### Observations And Fix

- The first forced-h2 request returned 502 after sending 1,113,953 bytes.
  Upstream TLS reported that the certificate for `h2.loop97.test` was invalid
  for `127.0.0.1`. `host(...)` had incorrectly replaced both the transport dial
  target and the origin TLS identity.
- Upstream TLS now always receives the parsed URL host for SNI and certificate
  verification; routing still uses the selected `host(...)` address for TCP.
  The obsolete route-level TLS-host helper was removed, and h1-to-h2 plus
  h2-to-h2 fixtures now issue certificates for their logical origin names.
- A forced-h2 curl then uploaded and downloaded exactly 8,388,608 bytes in
  2.056s with HTTP/2 status 200. Request and echoed response hashes matched;
  `X-Loop: 97` and the origin trailer were preserved. Daemon RSS rose from
  13,024KiB to a sampled peak of 20,304KiB, a 7,280KiB increase.
- A forced-h1 curl over the same MITM route proved h1 client to h2 origin
  bridging: exactly 8,388,608 bytes in both directions in 2.060s, HTTP/1.1 200,
  matching hashes, chunked downstream framing and the final origin trailer.
- Trace sessions recorded logical TLS host `h2.loop97.test` on both client and
  upstream phases. The h2 session had `h2-client`; both sessions had
  `request-streamed`, `response-streamed`,
  `request-body-rewrite-skipped-limit`, `h2-upstream`, pool-miss and trailer
  flags. Each retained exactly 4KiB request/response previews and 8MiB byte
  totals. Stats had zero queue/follower drops, pending sessions, partials,
  orphans or spill errors, and no live follower remained.

### Acceptance And Refactoring

- Added `scripts/test-protocol-matrix.sh`, a 29-owner h1/CONNECT/h2/trailer/
  streaming/gRPC/SSE/WebSocket/TLS/header matrix. It inventories exact Rust test
  names before execution, so a missing owner fails instead of running zero
  tests successfully. CI and its static workflow contract now require it.
- The matrix's first run found one remaining fixture signed only for
  `127.0.0.1`; updating its SAN to the logical `stream.test` origin fixed the
  test and strengthened the route-versus-identity contract. All 29 owners then
  passed.
- Final gates pass formatting, strict Clippy, locked all-target check, fuzz
  target compilation, release build, all source/test/workflow/shell guards,
  action contracts and 355 regular tests with one explicit resource test
  ignored by default. The benchmark completed 128/128 requests with exact bytes
  and zero errors.
- The explicit 1GiB release run transferred and traced 1,073,741,824 bytes in
  256ms. RSS rose from 17,312KiB to 26,944KiB, a 9,632KiB increase, with no
  queue drop or partial session. The largest Rust file remains 382 lines; all
  Loop 97 daemons and origins were stopped.
- The weighted overall estimate is now 90%, with about 10% remaining in M1
  deferred behavior, full network-depth automation for the remaining M2 matrix
  edges, M4 native platform/schema work and M5 hosted/performance/release
  evidence. No next Dogfooding loop has started.

## Post-Loop 97 M2 Network Matrix Closure

No Loop 98 daemon or manual curl round was started. This acceptance and
code-structure batch converted the remaining M2 historical/unit evidence into
repeatable real-network tests while preserving the requested pause between
numbered Dogfooding loops.

- Added `proxy/tests/protocol_matrix/` with separate `websocket.rs`, `mtls.rs`,
  `headers.rs` and `names.rs` owners; the largest new file is 170 lines. A real
  WebSocket origin performs upgrade, sends a frame before the client, receives
  masked text/close frames and returns echo/close frames; the resulting session
  verifies status, response rule injection and both trace directions.
- A rustls origin requiring a generated client CA now proves that
  `tls(client-cert=..., client-key=...)` succeeds over the actual MITM/origin
  path and sets `upstream-mtls`. The same origin rejects an anonymous proxy
  connection; the client receives 502 and trace preserves the stable
  `upstream_h1 pool_miss` failure boundary.
- Real h1 and h2 clients each send a 200KB request header successfully, then
  exceed the configured 256KB application limit and receive 431 with an
  explanatory body. The first h2 version was rejected by the transport before
  service code could answer; the decoder now advertises only a fixed 64KiB
  diagnostic margin above the application limit, preserving a bounded input
  window while allowing explicit 431 responses.
- A real IPv6 `::1` origin and a punycode host routed through `host(...)` verify
  URL, Host header, dial target and trace identity. The first IPv6 run exposed
  that no-op URL reconstruction removed brackets and changed
  `http://[::1]:port` into a different host with default port. Shared authority
  formatting now brackets IPv6 consistently across URLs, Host headers, route
  labels and dial addresses.
- `scripts/test-protocol-matrix.sh` grew from 29 to 34 exact owners and retains
  its test-list guard. All 34 pass, including the original fine-grained codec,
  framing and policy owners plus the five network cases above.
- Final gates pass formatting, warning-denied Clippy, locked all-target check,
  release build, fuzz-target compilation, action contracts, workflow/shell/
  source/test layout guards and 360 regular tests with one explicit resource
  test ignored by default. The benchmark completed 128/128 requests with exact
  bytes and zero errors.
- The explicit release resource test moved and traced 1,073,741,824 bytes in
  254ms. RSS rose from 17,008KiB to 28,208KiB, an 11,200KiB increase, with zero
  queue drops or partial sessions. M2 is now complete; the weighted overall
  estimate is 92%, with about 8% remaining in M1 deferred behavior, M4 native
  platform/schema work and M5 hosted/performance/release evidence.

## Post-Loop 97 M1 Contract Scope Closure

No numbered Dogfooding loop or runtime daemon was added. This batch reconciled
the executable Whistle option inventory with the already-defined v1 action
scope instead of treating open-ended Whistle parity as an unstated milestone.

- The technical design explicitly chooses a new DSL and fixes the v1 action set
  in §6.3. Its M1 acceptance line requires corpus A/C/D and the 10k-rule target;
  it does not require every Whistle `enable`/`disable` switch. Existing evidence
  covers all 46 action families, 86 corpus cases/37 spec anchors and a release
  10,000-rule p99 of 3.458µs against the 10µs limit.
- `whistle_options.toml` no longer uses stale `deferred-m2` or `deferred-m4`
  labels. `bigData` and `dnsCache` are classified as `process-config` and point
  to `--trace-body-limit` / `--dns-cache`; frame pausing, forced compression,
  connection-pool per-rule overrides and other actions absent from §6.3 remain
  explicit `deferred-v2`, not approximate v1 implementations.
- The option runner now accepts only `implemented`, `native-default`,
  `process-config`, `deferred-v2` and `removed-v1`. A process-config item must
  reference a unique real option in `cli/help.rs`; any future milestone-scoped
  deferred label fails the contract.
- The focused option test, complete action-effect entry point, formatting,
  strict Clippy, locked check, release build, all structure/workflow/shell
  guards and all 360 regular tests pass. Production code and the previously
  measured 1GiB resource path were unchanged by this contract-only batch.
- M1 is now complete on its documented acceptance line. The weighted overall
  estimate is 94%, with about 6% remaining in M4 native platform/schema/lifecycle
  work and M5 hosted/performance/soak/artifact/release evidence.

## Post-Loop 97 M4 CLI and Platform Closure

No Loop 98 proxy/curl Dogfooding round was started. This batch stayed within the
requested pause and converted the remaining M4 implementation and evidence gaps
into deterministic CLI, platform-compile and product contracts.

- Daemon startup now constructs state and synchronously binds proxy/control
  listeners before reporting readiness. Either listener exiting terminates the
  daemon; failed or timed-out startup kills the child and removes runtime files.
  Stop authenticates status and verifies storage identity before terminating a
  pidfile process, preventing PID reuse from killing an unrelated process.
- `cli_daemon_lifecycle` drives real detached binaries through start, duplicate
  start, status, restart with retained rules, stop, abnormal kill recovery,
  malformed pidfiles, occupied listeners, ephemeral-port rejection and an
  unrelated `sleep` PID. Unix now defaults to a storage-local 0600 socket; a
  long temporary storage path exposed the `sun_path` limit, so deterministic
  UID+storage-hash fallback and stop cleanup were added and tested.
- Query commands gained consistent JSON forms, including rules check/cat/stats/
  test/bench, values list/cat, CA status, system-proxy plans and TUI snapshots.
  A `--json` failure emits exactly one `rsproxy.cli.error/v1` object on stderr.
  Separate black-box suites cover JSON shapes and offline/online product flows.
- `rsproxy completions` now generates Bash, Zsh, Fish and PowerShell scripts.
  Every supported command/subcommand help path is executed before config,
  authentication or runtime side effects.
- System proxy backends now execute macOS `networksetup`, Linux GNOME
  `gsettings` with rollback, and Windows current-user registry changes with
  rollback plus WinINet refresh. CA trust is split into macOS `security`, Linux
  p11-kit and Windows current-user Root store backends, all with dry-run output.
- Windows gained an authenticated named-pipe control transport shared by the
  router and CLI client. The first instance is exclusive, remote clients are
  rejected, and the Windows-only daemon test covers named-pipe start/status/stop.
  Local MinGW runs completed Windows all-target check, warning-denied Clippy and
  release linking; hosted Windows execution remains part of M5 evidence.
- Final gates pass formatting, native and Windows warning-denied Clippy/check,
  native and Windows release builds, 34-owner protocol and 46-family action
  contracts, fuzz-target compilation, source/test/workflow guards and 376
  regular tests with one explicit resource test ignored by default. The largest
  Rust file is 383 lines.
- The release benchmark completed 128/128 requests and 131,072 bytes with zero
  status/IO errors in 31ms. The explicit 1GiB run transferred and traced exactly
  1,073,741,824 bytes in 385ms; RSS rose from 13,840KiB to 26,160KiB, a
  12,320KiB increase, with no queue drop or partial session.
- M4 is complete on its documented implementation and CLI-test acceptance line.
  The weighted overall estimate is 97%, with about 3% remaining solely in M5
  hosted CI, formal performance/coverage/soak, artifact and release evidence.

## Current State

Implemented; Dogfooding coverage is identified by the loop records above:

- Rust Cargo workspace.
- `rsproxy-rules` parser/matcher/action resolver for the documented v1 DSL, with exact/suffix domain index, global rule bucket, Aho-Corasick multi-literal regex prefilter, all 20 v1 template variables plus regex transforms, an 86-case/37-spec-anchor corpus, a 46-family action and 17-test real-network effect contract, typed whole and nested JSON/form/JSONP delete operations, a 40-field runtime value matrix, source-backed Whistle migration matrix classifying 74 canonical protocols and 22 aliases, an exact 56-enable/66-disable/16-delete option contract with verified process-config and explicit v2/removed boundaries, bounded complexity checks, proptest and shared-seed cargo-fuzz coverage, request/optional-response `rules test` explain support, `method`/`host`/`url`/`header`/`body`/`res.header`/`clientIp`/`ip`/`serverIp`/`status`/`chance`/`env`/`any` conditions, and request-period `rules bench` observability.
- HTTP/1.0 and HTTP/1.1 forward proxy path, including plain and MITM downstream keep-alive loops, ordered pipeline handling, HTTP/1.0 persistence/downgrade semantics, 90-second idle read timeout, safe upgrade/streaming exit, dogfooded and automated `--max-header-size` / `--max-header-count` limits, strict Content-Length/Transfer-Encoding admission, incremental fixed/chunked request-body readers, and request-trailer validation.
- Proxy admission authentication through `--proxy-auth user:pass`, including ordinary HTTP and CONNECT 407/authorized paths, case-insensitive Basic parsing, startup validation, and pre-dispatch credential stripping from rules, upstream forwarding, trace, spill, and export.
- CONNECT passthrough tunnel, including passthrough through HTTP proxy, HTTPS proxy, and SOCKS5 upstreams, plus configurable TCP and request-total setup deadlines with 504 attribution.
- CONNECT HTTPS MITM for HTTP/1.1 and client-side HTTP/2 requests when rsproxy CA is initialized, including request/response rules, `h2` / `http/1.1` ALPN negotiation, disk leaf certificate reuse plus bounded in-memory `ServerConfig` LRU caching, TLS handshake/certificate/ALPN trace records, configurable absolute client TLS handshake deadline with 408 attribution, configurable absolute upstream TLS handshake deadline with 504 attribution, stricter root CA key usage, merged WebPKI/native/storage-CA upstream validation with cached deduplicated roots and status diagnostics, chunked response decoding, and staged upstream failure attribution.
- Client h2→upstream h1 bridging through Hyper/Tokio, including pseudo-header mapping, POST bodies, request/response trailers, connection-header normalization, concurrent multiplexed streams, pooled h1-origin fallback across client sessions, h2 header limits, `h2-client` trace/export/spill visibility, and Unix `AsyncFd` readiness-driven TLS IO.
- HTTPS origin h2 support through independent ALPN and a pooled Hyper client, including h1→h2/h2→h2 bridging, request/response trailers, concurrent cross-client upstream streams, h1 fallback, direct/SOCKS/HTTP-proxy-CONNECT origin routes, shared readiness-driven rustls IO, header limits, response-period rules, body/trailer preservation, 256-key capacity, active-aware 60-second idle eviction, per-key stream admission, connector single-flight, configurable pool-wait/504 attribution, per-stream TTFB deadline that preserves healthy shared connections, and `h2-upstream` pool hit/miss trace visibility. Loop 97 used forced h1 and h2 curl clients to echo 8MiB through a TLS/h2-only origin with exact hashes and trailers.
- Shared HTTP/1.1 upstream keep-alive pooling across plain and TLS origins, including multi-connection route/TLS-isolated keys, 256 global/per-key idle capacity, 90-second idle eviction, configurable per-key active admission and pool-wait timeout, 504 starvation attribution, independent response-head TTFB deadline, lease ownership through streamed body completion, `pool_wait_ms`/`request_send_ms`/`ttfb_ms`/`response_receive_ms` JSON/HAR/spill/TUI visibility, stale pre-dispatch recovery, `Connection: close`/body-error exclusion, trailer capability advertisement, hop-header normalization, and `h1-upstream` pool hit/miss trace visibility.
- Bounded downstream HTTP/1 request streaming for plain and MITM proxy paths, including candidate-aware body dependency planning, local 100 Continue, pre-body proxy auth, request-total upload deadlines, fixed/chunked framing and trailer fidelity, trace-prefix tee capture, exact byte accounting, body-rule overflow degradation, and isolated origin h1 forwarding. Loop 95 uploaded 64MB under backpressure with stable RSS; small bodies retain pooled h1/h2 behavior.
- Bounded upstream h1/h2 response streaming to ordinary HTTP/1.1 clients, including chunked framing, trailer fidelity, trace-prefix tee capture, body-error session attribution, default 8MB response-rule aggregation, `--body-buffer-limit`/TOML/status configuration, and unmodified overflow fallback with `body-rewrite-skipped-limit`. Loop 94 transferred 64MB under curl backpressure with stable RSS.
- Request-trailer preservation across h1→h1, h1→h2, h2→h1, and h2→h2, including framing normalization, forbidden-field rejection, `req-trailers` trace flags, and JSON/HAR/spill/TUI observability.
- End-to-end unary gRPC transport through TLS h2→h2, including binary frame fidelity, `application/grpc`, `TE: trailers`, `grpc-status` / `grpc-message`, rule-added trailers, and curl/nghttpd protocol evidence.
- Upstream mTLS for HTTPS origin connections via `tls(client-cert=<path>, client-key=<path>)`, including template-rendered storage-relative certificate/key paths, rustls client-auth configuration, dogfooded direct-origin and HTTP-upstream-proxy CONNECT verification, plus automated required-client-cert success/anonymous-failure and `upstream-mtls` trace visibility.
- Origin TLS policy via `tls(min=1.2|1.3, ciphers=<list>)`, including IANA/OpenSSL aliases, parse-time compatibility checks, ordered rustls aws-lc provider filtering, origin/proxy-hop isolation, successful negotiated-cipher trace, and structured failed-handshake trace/export/spill records.
- WebSocket HTTP/1.1 upgrade forwarding with decoded frame trace, including concurrent bidirectional forwarding for plain TCP sessions, nonblocking TLS/MITM WSS forwarding, server-first frames, fragmentation metadata, ping/pong control-frame trace, and binary hex previews with trace-limit truncation. A real-network CI owner verifies upgrade, response rules, masked client text/close, server-first/echo/close and both trace directions.
- SSE response streaming with frame trace, including chunked upstream decoding and incremental frame capture.
- Event-level trace collection for HTTP sessions, including nonblocking
  Start/Request/Response/body/snapshot/frame/TLS/End/Abort events, bounded
  queue/resident memory partitions, pending-session cleanup and diagnostics,
  exact final snapshot correction, and live NDJSON follow with independent
  bounded subscribers and liveness-token cleanup visible on the next stats
  query. Passthrough CONNECT/tunnel uses the same lifecycle with
  direction-only observed byte events and no opaque payload preview; proxy
  production paths no longer use the atomic compatibility batch. Loop 96
  exercised follow, timing, tunnel, zstd spill, export, TUI and replay with a
  release daemon, CLI and curl.
- HTTP `upstream(proxy://...)`, `upstream(http://...)`, HTTP proxy multi-hop chains via `upstream(proxy://p1, proxy://p2)`, mixed HTTP/SOCKS/HTTPS-proxy multi-hop chains via `upstream(proxy://p1, socks5://s1)` and `upstream(proxy://p1, https-proxy://hp1)`, nested multiple-HTTPS-proxy chains via `upstream(https-proxy://hp1, https-proxy://hp2)`, single-hop `upstream(https-proxy://...)`, HTTP request forwarding through SOCKS5 upstreams with no-auth or username/password `upstream(socks://...)` / `upstream(socks5://user:pass@...)`, `direct` route overrides for matched upstream rules, and a shared configurable absolute TCP connect deadline across all route types.
- Shared Hickory DNS resolution for direct origins and locally connected proxy hops, including system/custom nameservers, positive and negative TTL caches, zero-cache mode, absolute DNS deadline with HTTP/CONNECT 504 attribution, literal-IP/`host(...)` bypass, status counters, IPv4-priority dual-stack ordering and bracket-safe IPv6 URL/Host/dial/trace handling.
- Configurable absolute request-total deadline, defaulting to 360 seconds, shared across downstream h1 body reads/uploads, request rule delay, pool admission/readiness, DNS/TCP/proxy/TLS/protocol setup, TTFB, manual buffered response reads, and Hyper h1/h2 response frame production. It returns an exact staged 504 with trace flags, covers CONNECT setup, and explicitly exempts established CONNECT/WebSocket/SSE streams.
- Regex matchers compiled at rule parse time: linear Rust `regex` by default, `fancy-regex` fallback for backreferences/lookaround with a hard backtrack budget, plus numbered and named captures.
- Daemon lifecycle: `start`, `stop`, `restart`, foreground `run`, and API-backed
  `status`; listeners are synchronously prebound/supervised, failed starts clean
  runtime state, stop verifies process identity, and real-binary tests cover
  restart/rule retention, abnormal exit, malformed pidfiles and bind failures.
  Root and every hierarchical `-h/--help` exit before config/auth/runtime side
  effects; Bash/Zsh/Fish/PowerShell completion generation has the same contract.
- Process observability through stderr-only `tracing` text/JSON with stable
  startup, listener, trust-root, connection and session events; request Trace is
  a separate bounded collector contract.
- A self-contained release h1/curl macrobenchmark under `benches/e2e/` with a
  versioned JSON result and exact zero-error contract test. Local criterion/oha,
  Whistle, coverage, 1GiB resource and short-soak gates are implemented; the
  Apple M1 Pro release baseline is 45,392 rps with a 40,853 rps regression floor.
- Cross-platform CI definitions for locked Ubuntu/macOS/Windows workspace gates,
  an Ubuntu quality-contract job with 34-owner protocol and 46-family action matrices, and
  daily Ubuntu/nightly parse/resolve fuzzing, all constrained by a local
  workflow contract. They remain best-effort compatibility automation; hosted
  runner and multi-platform artifact evidence are outside the current v1 scope.
- Strongly typed TOML configuration with CLI > explicit/default config file > defaults precedence, runtime validation, token discovery precedence, secret-safe status, and shared command resolution. Optional external rule watching uses bounded event delivery, configurable debounce, whole-snapshot validation and atomic publication. Loops 94-97 dogfooded explicit config startup, CLI override, authenticated status, restart/rule persistence, request/response body limits, h1/h2 origin routing and the status/rules/trace/TUI/replay client paths; the watcher itself currently has static automated coverage only.
- Replay: `rsproxy replay <id>` for captured HTTP sessions.
- `host` single/multi-address round robin, `upstream`, `direct`, `mock` inline/file/@key/candidate/directory, `mock.raw`, `status`, `redirect`, `req.header` / `res.header` set/remove/regex-replace, `res.status`, `req.method`, `req.cookie`, `res.cookie` basic/detailed, `req.ua`, `req.referer`, `req.auth`, `req.forwarded`, `req.type`, `req.charset`, `res.type`, `res.charset`, `res.cors` basic/detailed, `cache` basic/detailed, `attachment`, `url.rewrite` plain/regex, `url.query`, typed `delete`, `req.body.set/prepend/append/replace`, `res.body.set/prepend/append/replace`, `inject` html/js/css append/prepend/replace, `skip` family/all control, `hide` trace suppression, `tag` trace flags, `res.merge`, `res.trailer`, `delay`, and `throttle`.
- In-memory trace ring, separate request/response trailer capture, structured pool/DNS/connect/request-send/TTFB/response-receive timing, TLS handshake/certificate/protocol/cipher/ALPN/error records, HAR 1.2 RFC 3339 timestamps, h2/query fidelity, blocked/DNS/connect/SSL/send/wait/receive decomposition, and `_rsproxy` error/flag/rule/TLS/timing diagnostics; header-only body capture suppression via `--trace-filter headers-only`, media body preview exclusion via `--trace-filter media` with `--trace-filter full` opt-out, append-only segmented NDJSON disk spill with segment rotation, disk-budget eviction, sidecar index files, CRC-verified spill recovery, optional zstd compressed segments, collector-independent immutable read snapshots, `trace ls/get/follow/stats/clear/export`, and `/api/sessions/spill.ndjson`.
- Authenticated TCP, 0600 Unix socket and Windows named-pipe control APIs plus CLI client for status, rules, trace, CA and platform operations. Unix defaults to a storage-local socket with deterministic short-path fallback; Windows defaults to an exclusive local named pipe. TCP/pipe use explicit or automatically persisted 256-bit tokens. CA install/uninstall uses macOS security, Linux p11-kit or the Windows current-user Root store; system proxy mutation uses macOS networksetup, rollback-aware Linux gsettings or rollback-aware Windows registry plus WinINet refresh. All retain dry-run plans.
- Ratatui TUI via `rsproxy tui`, including status panel, recent session table, selected session detail, refresh/selection keys, text/JSON `--once` snapshot mode, `--filter`/interactive filtering, overview/headers/body/rules detail tabs, and selected-session replay shortcut.
- Values CRUD CLI/API plus structured `@key`/`<file>`/inline sources across all
  value-bearing v1 action fields, with centralized runtime text/binary handling.

Remaining design gaps:

- The v1 qualification host is the current Apple M1 Pro macOS ARM64 machine.
  Linux/Windows target-OS execution, hosted workflows and multi-platform release
  qualification are intentionally outside the current scope.
- The h1/h2 request and response streaming paths are bounded, have automated
  early-delivery evidence, and the h1 release path has passed the explicit 1GiB
  resource run. Loop 97 closed real-client h1→h2 and h2→h2 runtime evidence and
  the post-loop batch closed the 34-owner executable protocol matrix, including
  network WS, mTLS, header and name boundaries. CONNECT/WebSocket over h2 and
  h2c remain explicit v2+ boundaries; SSE/WebSocket/throttled-request paths pin
  origin ALPN to h1.
- Remote h2 `SETTINGS_MAX_CONCURRENT_STREAMS` below the configured local limit is queued inside Hyper and is not separately attributable as pool wait; exact remote-capacity timing requires lower-level h2 dispatch instrumentation.
- M5 local steady-state evidence is complete: 6,307 seconds at 1k QPS covered
  6,379,936 sessions and 106 minute samples; RSS ended below its start, the
  last-half slope was negative, FD peak stayed below its concurrency-derived
  limit, and trace had no pending/incomplete/orphan/drop/spill state. No new
  Dogfooding loop is required for Linux/Windows validation.
