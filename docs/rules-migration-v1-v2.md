# Rules Language v1 to v2 Migration

Version 2 keeps the documented v1 grammar and makes previously ambiguous or
silently ineffective input fail during atomic snapshot validation. The help
JSON reports this as `language_version: 2`; `rsproxy.rules.help/v1` remains the
independent JSON output-schema version.

Run both checks against every enabled group before replacing a production
snapshot:

```sh
rsproxy rules check --file rules.txt
rsproxy rules lint --file rules.txt
```

## Required rewrites

| v1 input that may have parsed accidentally | v2 result | Rewrite |
| --- | --- | --- |
| `when status(600)` through `status(999)` | condition error | Use an HTTP status in `100..599` |
| `status(100..199)`, `res.status(100..199)`, or `mock(status=100..199)` | action error | Final responses must use `200..599`; informational responses are not final rule actions |
| `mock(status=204, body=...)`, `mock(status=205, body=...)`, or `mock(status=304, body=...)` | action error | Remove the body or choose a status that permits response content |
| `redirect(url, 300)`, `redirect(url, 304)`, or another unregistered 3xx | action error | Use `301`, `302`, `303`, `307`, or `308` |
| `when env(NAME WITH SPACE)` | condition error | Use a non-empty name without `=`, NUL, or whitespace |
| `redirect(url, 302, ignored)` | action error | Remove every argument after the optional status |
| `attachment(one, ignored)` | action error | Keep zero or one filename argument |
| `call(value,)`, `call(,value)`, `call(a,,b)` | syntax/action/condition error | Remove empty argument slots |
| a one-sided or unclosed quote, `(`, `[`, `{`, or `<file>` delimiter | syntax/action error | Balance the delimiter; quote a literal leading `<` |
| `@tag:` | property error | Supply a non-empty source tag or remove the property |
| `skip(unknown-family)` | action error | Use a canonical action family, a parent such as `res.body`, `all`, or `*` |
| a glob ending in an unmatched `\` | matcher/condition error | Escape the backslash as `\\` or remove it |
| `delay(req, NaNs)` or non-finite/overflowing speed | action error | Supply a finite, representable duration or positive speed |
| source exceeding a published snapshot/per-rule limit | stage-specific limit error | Split or simplify the ruleset using the values from `rules help concept.limits --json` |
| external rule value/mock over 8 MiB or PEM file over 1 MiB | execution/control error | Reduce or stream the asset outside the buffered rule-value mechanism |
| a rendered path over 4 KiB, value over 8 MiB, or rule-produced URL/header/body over its runtime budget | execution error before expansion allocation | Reduce the template/replacement or raise the relevant bounded runtime configuration |

Status conditions accept the protocol-wide `100..599` range because they may
observe informational upstream metadata. Final response actions and mocks use
`200..599`; `204`, `205`, and `304` never serialize response content. Redirect actions accept
only `301`, `302`, `303`, `307`, and `308`.

## New conservative lint findings

These findings do not make source invalid and do not change resolution. The
`rules lint` command reports them with schema `rsproxy.rules.lint/v1` and exits
1 so deployments can require an explicit review:

| `kind` | Proven issue | Typical rewrite |
| --- | --- | --- |
| `shadowed-rule` | An earlier unconditional broader matcher wins the same single-action family | Move the specific rule first or narrow the broader matcher |
| `duplicate-single-family` | A later action in one single-action family cannot win | Keep the intended action only |
| `unsatisfiable-conditions` | Method/status/env sets or constant chance/boolean conditions cannot match | Remove or widen the contradictory AND constraint |
| `request-action-requires-response` | A request-only action is guarded by response metadata that does not exist yet | Move the effect to a response action or use request-available conditions |
| `action-after-skip` | An earlier same-rule `skip` suppresses a later action | Reorder the action or narrow/remove `skip` |
| `conflicting-terminal-actions` | More than one of `status`, `redirect`, and `mock` is active | Keep one; existing precedence is status, redirect, then mock |
| `response-action-with-local-response` | A local response bypasses the upstream response-action pipeline | Put headers/body in the mock or remove the local response |
| `upstream-overridden-by-direct` | Same-rule `direct` always overrides `upstream` | Keep the intended route only |
| `body-action-with-bodyless-status` | `res.status(204/205/304)` makes response body mutation, injection, or merge output unobservable | Remove the body action or select a body-capable status |

The linter does not guess about regex overlap, ambiguous `any(...)` branches,
dynamic environment contents, or cross-rule capability interactions. A clean
result is not a proof that every intended request matches.

## Compatibility policy

The canonical spellings and compatibility aliases are exposed by:

```sh
rsproxy rules help --json
```

Read `topics[].dsl_spellings`, not the broader help-query `aliases`, when
generating rule source. Additive, unambiguous syntax can remain language v2.
Removing an accepted spelling or changing the meaning of accepted source
requires another language-version bump and migration notes.

## Rust API migration

`RuleSet` now keeps its AST and compiled indices immutable as one unit. Replace
direct field access as follows:

| v1 | v2 |
| --- | --- |
| `set.rules` | `set.rules()` |
| `set.version` | `set.version()` |
| `set.rules.is_empty()` | `set.is_empty()` |
| owned `MatchedRule.group` / `.raw` `String` assumptions | shared `Arc<str>`; borrow with `.as_ref()` or copy with `.to_string()` |

The returned rule slice is read-only; construct a new validated `RuleSet`
instead of mutating published rules in place.

New inspection APIs are additive: `RuleSet::semantic_lint()` exposes typed
same-rule findings; `Action::family_phases`, `Action::applies_in`, and
`Phase::as_str` expose request/response capabilities; and the public syntax
registry plus `RULE_LANGUAGE_VERSION` lets integrations generate canonical DSL
spellings without scraping human documentation.

`ResolvedAction::render_bounded` and `Captures::render_bounded` /
`render_with_response_bounded` return `RuleModelError::LimitExceeded` before
template or regex expansion allocates beyond the caller's byte budget. The
legacy unbounded render methods remain source-compatible for trusted,
caller-owned inputs; the proxy runtime uses only the bounded variants.

Use `RuleSet::lint_report()` and `semantic_lint_report()` when completeness
matters. Their `complete` flag distinguishes a genuinely complete empty report
from a bounded prefix after a comparison-count, charged matcher-byte, finding,
or report-byte budget is reached. `LintReport` also exposes
`comparison_bytes`; legacy `lint()` and `semantic_lint()` still return only the
finding vector.

`RuleSet::version()` remains a `u64`, but fresh values are now process-local
monotonic publication IDs rather than raw wall-clock milliseconds. Do not parse
them as timestamps; clones intentionally retain the same value.
