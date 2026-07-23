use super::*;

mod conditions;
mod delete;
mod error;
mod lexer;
mod matcher;
mod metadata;
mod migration_hints;
mod mock;
mod syntax;
mod tls;
mod transforms;

/// Hard source-line bound used before tokenization or regex compilation.
///
/// Keeping the limit at the fuzzing contract's maximum makes accepted input
/// explicit while preventing one accidental/generated line from consuming
/// unbounded parser memory.
pub(super) const MAX_RULE_LINE_BYTES: usize = MAX_RULE_SOURCE_LINE_BYTES;

/// Maximum recursive matcher/condition and delimiter nesting accepted by the DSL.
pub(super) const MAX_PARSE_NESTING: usize = MAX_RULE_PARSE_NESTING;

use crate::family::normalize_skip_family;
use conditions::parse_condition;
pub(super) use conditions::{body_condition_count, condition_node_count};
use delete::parse_delete_ops;
use error::{ParseRuleError, parse_error, validate_status_range};
pub(super) use lexer::{RuleToken, tokenize};
use matcher::{parse_matcher, parse_regex_matcher};
use metadata::*;
use migration_hints::whistle_syntax_hint;
use mock::{parse_mock, validate_map_remote_target};
pub(super) use syntax::*;
use tls::parse_tls_op;
use transforms::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SyntaxProfile {
    /// Headerless, programmatic sources retain v2 aliases for API compatibility.
    Compatible,
    /// Versioned v3 sources accept canonical spellings only.
    CanonicalV3,
}

pub(super) fn parse_rule(
    group: &str,
    line: usize,
    input: &str,
    profile: SyntaxProfile,
) -> Result<Rule, ParseRuleError> {
    let tokens = tokenize(input)
        .map_err(|source| parse_error(RuleErrorCode::Syntax, source).with_span(0, input.len()))?;
    if tokens.is_empty() {
        return Err(parse_error(
            RuleErrorCode::Syntax,
            RuleModelError::empty("rule", "empty rule"),
        ));
    }

    let matcher = parse_matcher(&tokens[0].text)
        .map_err(|source| parse_error(RuleErrorCode::Matcher, source).at_token(&tokens[0]))?;
    let mut actions = Vec::new();
    let mut conditions = Vec::new();
    let mut condition_nodes = 0usize;
    let mut important = false;
    let mut disabled = false;
    let mut tags = Vec::new();
    let mut properties = 0usize;
    let mut idx = 1;

    while idx < tokens.len() {
        let token = &tokens[idx];
        if token.text == "when" {
            idx += 1;
            let cond = tokens.get(idx).ok_or_else(|| {
                parse_error(
                    RuleErrorCode::Condition,
                    RuleModelError::missing(
                        "when property",
                        "`when` must be followed by a condition",
                    ),
                )
            })?;
            let condition = parse_condition(&cond.text, profile)
                .map_err(|source| parse_error(RuleErrorCode::Condition, source).at_token(cond))?;
            condition_nodes = condition_nodes.saturating_add(condition_node_count(&condition));
            if condition_nodes > MAX_RULE_CONDITION_NODES_PER_RULE {
                return Err(parse_error(
                    RuleErrorCode::Condition,
                    RuleModelError::limit(
                        "condition count",
                        format!(
                            "rule exceeds the {MAX_RULE_CONDITION_NODES_PER_RULE}-condition-node limit"
                        ),
                    ),
                ));
            }
            conditions.push(condition);
        } else if token.text.starts_with('@') {
            properties += 1;
            if properties > MAX_RULE_PROPERTIES_PER_RULE {
                return Err(parse_error(
                    RuleErrorCode::Property,
                    RuleModelError::limit(
                        "property count",
                        format!("rule exceeds the {MAX_RULE_PROPERTIES_PER_RULE}-property limit"),
                    ),
                ));
            }
            match language::canonical_property(&token.text) {
                Some(("@important", _)) => important = true,
                Some(("@disabled", _)) => disabled = true,
                Some(("@tag:", "")) => {
                    return Err(parse_error(
                        RuleErrorCode::Property,
                        RuleModelError::missing("rule tag", "@tag: requires a non-empty name"),
                    ));
                }
                Some(("@tag:", name)) => tags.push(name.to_string()),
                _ => {
                    return Err(parse_error(
                        RuleErrorCode::Property,
                        RuleModelError::unsupported(
                            "rule property",
                            format!("unknown property {}", token.text),
                        ),
                    ));
                }
            }
        } else {
            if actions.len() == MAX_RULE_ACTIONS_PER_RULE {
                return Err(parse_error(
                    RuleErrorCode::Action,
                    RuleModelError::limit(
                        "action count",
                        format!("rule exceeds the {MAX_RULE_ACTIONS_PER_RULE}-action limit"),
                    ),
                ));
            }
            actions
                .push(parse_action(&token.text, profile).map_err(|source| {
                    parse_error(RuleErrorCode::Action, source).at_token(token)
                })?);
        }
        idx += 1;
    }

    if actions.is_empty() {
        return Err(parse_error(
            RuleErrorCode::Action,
            RuleModelError::missing("rule action", "rule must include at least one action"),
        ));
    }

    Ok(Rule {
        group: Arc::from(group),
        line,
        raw: Arc::from(input),
        matcher,
        actions,
        conditions,
        important,
        disabled,
        tags,
    })
}

fn parse_action(input: &str, profile: SyntaxProfile) -> Result<Action, RuleModelError> {
    let canonical_name = |name| match profile {
        SyntaxProfile::Compatible => canonical_action_name(name),
        SyntaxProfile::CanonicalV3 => language::canonical_v3_action_name(name),
    };
    match canonical_name(input) {
        Some("direct") => return Ok(Action::Direct),
        Some("bypass") => return Ok(Action::Bypass),
        Some("hide") => return Ok(Action::Hide),
        _ => {}
    }
    if let Some(hint) = whistle_syntax_hint(input) {
        return Err(RuleModelError::unsupported("action", hint));
    }

    let (input_name, args) = parse_call(input)?;
    let Some(name) = canonical_name(input_name) else {
        if profile == SyntaxProfile::CanonicalV3
            && let Some(canonical) = canonical_action_name(input_name)
        {
            return Err(RuleModelError::unsupported(
                "action",
                format!(
                    "v3 accepts canonical action names only; replace `{input_name}` with `{canonical}`"
                ),
            ));
        }
        return Err(RuleModelError::unsupported(
            "action",
            format!("unknown action `{input_name}`"),
        ));
    };
    let action = match name {
        "host" => Ok(Action::Host(HostPool::new(
            args.iter()
                .map(|address| parse_value(address))
                .collect::<Result<Vec<_>, _>>()?,
        )?)),
        "upstream" => {
            if args.is_empty() {
                return Err(RuleModelError::missing(
                    "upstream action",
                    "upstream requires at least one argument",
                ));
            }
            let value = if args.len() == 1 {
                parse_value(args[0])?
            } else {
                Value::Inline(args.join(", "))
            };
            Ok(Action::Upstream(value))
        }
        "mock" => parse_mock(&args),
        "map.remote" => {
            let value = parse_value(require_one(&args, "map.remote")?)?;
            validate_map_remote_target(&value)?;
            Ok(Action::MapRemote(value))
        }
        "mock.raw" => Ok(Action::MockRaw(parse_value(require_one(
            &args, "mock.raw",
        )?)?)),
        "status" => {
            let raw = require_one(&args, "status")?;
            let code = raw.parse::<u16>().map_err(|source| {
                RuleModelError::integer("status code", raw, "status code must be numeric", source)
            })?;
            validate_status_range(
                code,
                MIN_FINAL_HTTP_STATUS..=MAX_HTTP_STATUS,
                "status code",
                "200..599",
            )?;
            Ok(Action::Status(code))
        }
        "redirect" => {
            if args.is_empty() {
                return Err(RuleModelError::missing(
                    "redirect action",
                    "redirect requires URL",
                ));
            }
            let code = if args.len() > 1 {
                args[1].parse::<u16>().map_err(|source| {
                    RuleModelError::integer(
                        "redirect code",
                        args[1],
                        "redirect code must be numeric",
                        source,
                    )
                })?
            } else {
                302
            };
            if args.len() > 2 {
                return Err(RuleModelError::constraint(
                    "redirect action",
                    "redirect accepts URL and at most one status code",
                ));
            }
            if !REDIRECT_STATUSES.contains(&code) {
                return Err(RuleModelError::constraint(
                    "redirect code",
                    "redirect code must be one of 301, 302, 303, 307, or 308",
                ));
            }
            let url = parse_value(args[0])?;
            if let Value::Inline(location) = &url
                && !location.contains('$')
            {
                validate_redirect_location(location)?;
            }
            Ok(Action::Redirect { url, code })
        }
        "req.header" => Ok(Action::ReqHeader(parse_header_op(require_call_body(
            input,
            "req.header",
        )?)?)),
        "res.header" => Ok(Action::ResHeader(parse_header_op(require_call_body(
            input,
            "res.header",
        )?)?)),
        "res.status" => {
            let raw = require_one(&args, "res.status")?;
            let code = raw.parse::<u16>().map_err(|source| {
                RuleModelError::integer(
                    "response status code",
                    raw,
                    "res.status code must be numeric",
                    source,
                )
            })?;
            validate_status_range(
                code,
                MIN_FINAL_HTTP_STATUS..=MAX_HTTP_STATUS,
                "response status code",
                "200..599",
            )?;
            Ok(Action::ResStatus(code))
        }
        "req.method" => Ok(Action::ReqMethod(parse_value(require_one(
            &args,
            "req.method",
        )?)?)),
        "req.cookie" => Ok(Action::ReqCookie(parse_cookie_op(require_one(
            &args,
            "req.cookie",
        )?)?)),
        "res.cookie" => Ok(Action::ResCookie(parse_cookie_op(require_one(
            &args,
            "res.cookie",
        )?)?)),
        "req.ua" => Ok(Action::ReqUa(parse_value(require_one(&args, "req.ua")?)?)),
        "req.referer" => Ok(Action::ReqReferer(parse_value(require_one(
            &args,
            "req.referer",
        )?)?)),
        "req.auth" => Ok(Action::ReqAuth(parse_value(require_one(
            &args, "req.auth",
        )?)?)),
        "req.forwarded" => Ok(Action::ReqForwarded(parse_value(require_one(
            &args,
            "req.forwarded",
        )?)?)),
        "req.type" => Ok(Action::ReqType(parse_value(require_one(
            &args, "req.type",
        )?)?)),
        "req.charset" => Ok(Action::ReqCharset(parse_value(require_one(
            &args,
            "req.charset",
        )?)?)),
        "res.cors" => Ok(Action::ResCors(parse_cors_op(&args)?)),
        "res.type" => Ok(Action::ResType(parse_value(require_one(
            &args, "res.type",
        )?)?)),
        "res.charset" => Ok(Action::ResCharset(parse_value(require_one(
            &args,
            "res.charset",
        )?)?)),
        "res.merge" => Ok(Action::ResMerge(parse_value(require_one(
            &args,
            "res.merge",
        )?)?)),
        "res.trailer" => {
            let operation = parse_header_op(require_call_body(input, "res.trailer")?)?;
            validate_trailer_op(&operation)?;
            Ok(Action::ResTrailer(operation))
        }
        "attachment" => {
            if args.len() > 1 {
                return Err(RuleModelError::constraint(
                    "attachment action",
                    "attachment accepts at most one filename",
                ));
            }
            Ok(Action::Attachment(
                args.first()
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| parse_value(value))
                    .transpose()?,
            ))
        }
        "cache" => Ok(Action::Cache(parse_cache_op(&args)?)),
        "tls" => Ok(Action::Tls(parse_tls_op(&args)?)),
        "url.rewrite" => {
            if args.len() != 2 {
                return Err(RuleModelError::missing(
                    "url.rewrite action",
                    "url.rewrite requires from and to",
                ));
            }
            Ok(Action::UrlRewrite {
                from: parse_url_rewrite_pattern(args[0])?,
                to: parse_value(args[1])?,
            })
        }
        "url.query" => Ok(Action::UrlQuery(parse_query_ops(&args)?)),
        "delete" => Ok(Action::Delete(parse_delete_ops(&args)?)),
        "req.body.set" => Ok(Action::ReqBody(BodyOp::Set(parse_value(require_one(
            &args,
            "req.body.set",
        )?)?))),
        "req.body.prepend" => Ok(Action::ReqBody(BodyOp::Prepend(parse_value(require_one(
            &args,
            "req.body.prepend",
        )?)?))),
        "req.body.append" => Ok(Action::ReqBody(BodyOp::Append(parse_value(require_one(
            &args,
            "req.body.append",
        )?)?))),
        "req.body.replace" => Ok(Action::ReqBody(parse_body_replace(
            &args,
            "req.body.replace",
        )?)),
        "res.body.set" => Ok(Action::ResBody(BodyOp::Set(parse_value(require_one(
            &args,
            "res.body.set",
        )?)?))),
        "res.body.prepend" => Ok(Action::ResBody(BodyOp::Prepend(parse_value(require_one(
            &args,
            "res.body.prepend",
        )?)?))),
        "res.body.append" => Ok(Action::ResBody(BodyOp::Append(parse_value(require_one(
            &args,
            "res.body.append",
        )?)?))),
        "res.body.replace" => Ok(Action::ResBody(parse_body_replace(
            &args,
            "res.body.replace",
        )?)),
        "inject" => Ok(Action::Inject(parse_inject_op(&args)?)),
        "delay" => {
            if args.len() != 2 {
                return Err(RuleModelError::missing(
                    "delay action",
                    "delay requires phase and duration",
                ));
            }
            let phase = match args[0].trim() {
                "req" => Phase::Req,
                "res" => Phase::Res,
                _ => {
                    return Err(RuleModelError::invalid(
                        "delay phase",
                        "delay phase must be req or res",
                    ));
                }
            };
            Ok(Action::Delay {
                phase,
                millis: parse_duration_ms(args[1].trim())?,
            })
        }
        "throttle" => {
            if args.len() != 2 {
                return Err(RuleModelError::missing(
                    "throttle action",
                    "throttle requires phase and speed",
                ));
            }
            let phase = match args[0].trim() {
                "req" => Phase::Req,
                "res" => Phase::Res,
                _ => {
                    return Err(RuleModelError::invalid(
                        "throttle phase",
                        "throttle phase must be req or res",
                    ));
                }
            };
            Ok(Action::Throttle {
                phase,
                bytes_per_sec: parse_speed_bps(args[1].trim())?,
            })
        }
        "tag" => Ok(Action::Tag(parse_value(require_one(&args, "tag")?)?)),
        "skip" => {
            let mut families = ActionFamilySet::EMPTY;
            for family in &args {
                let family = normalize_skip_family(&unquote(family));
                let Some(selected) = ActionFamilySet::from_prefix(&family) else {
                    return Err(RuleModelError::unsupported(
                        "skip action family",
                        format!(
                            "unknown skip family `{family}`; use an action family, a parent such as `res.body`, `all`, or `*`"
                        ),
                    ));
                };
                families.union(selected);
            }
            Ok(Action::Skip(families))
        }
        _ => Err(RuleModelError::unsupported(
            "action",
            format!("unknown action `{name}`"),
        )),
    }?;
    action.validate_templates()?;
    Ok(action)
}
