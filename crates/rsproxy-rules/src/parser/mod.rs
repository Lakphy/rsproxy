use super::*;

mod conditions;
mod delete;
mod matcher;
mod metadata;
mod migration_hints;
mod mock;
mod syntax;
mod tls;
mod transforms;

use conditions::*;
use delete::parse_delete_ops;
use matcher::{parse_matcher, parse_regex_matcher};
use metadata::*;
use migration_hints::whistle_syntax_hint;
use mock::{parse_mock, validate_map_remote_target};
pub(super) use syntax::*;
use tls::parse_tls_op;
use transforms::*;

pub(super) struct ParseRuleError {
    pub(super) code: RuleErrorCode,
    pub(super) source: RuleModelError,
}

pub(super) fn parse_rule(group: &str, line: usize, input: &str) -> Result<Rule, ParseRuleError> {
    let tokens = tokenize(input).map_err(|source| parse_error(RuleErrorCode::Syntax, source))?;
    if tokens.is_empty() {
        return Err(parse_error(
            RuleErrorCode::Syntax,
            RuleModelError::empty("rule", "empty rule"),
        ));
    }

    let matcher =
        parse_matcher(&tokens[0]).map_err(|source| parse_error(RuleErrorCode::Matcher, source))?;
    let mut actions = Vec::new();
    let mut conditions = Vec::new();
    let mut important = false;
    let mut disabled = false;
    let mut tags = Vec::new();
    let mut idx = 1;

    while idx < tokens.len() {
        let token = &tokens[idx];
        if token == "when" {
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
            conditions.push(
                parse_condition(cond)
                    .map_err(|source| parse_error(RuleErrorCode::Condition, source))?,
            );
        } else if let Some(prop) = token.strip_prefix('@') {
            match prop {
                "important" => important = true,
                "disabled" => disabled = true,
                _ if prop.starts_with("tag:") => tags.push(prop[4..].to_string()),
                _ => {
                    return Err(parse_error(
                        RuleErrorCode::Property,
                        RuleModelError::unsupported(
                            "rule property",
                            format!("unknown property @{prop}"),
                        ),
                    ));
                }
            }
        } else {
            actions.push(
                parse_action(token).map_err(|source| parse_error(RuleErrorCode::Action, source))?,
            );
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
        group: group.to_string(),
        line,
        raw: input.to_string(),
        matcher,
        actions,
        conditions,
        important,
        disabled,
        tags,
    })
}

fn parse_error(code: RuleErrorCode, source: RuleModelError) -> ParseRuleError {
    ParseRuleError { code, source }
}

fn parse_action(input: &str) -> Result<Action, RuleModelError> {
    match input {
        "direct" => return Ok(Action::Direct),
        "bypass" => return Ok(Action::Bypass),
        "hide" => return Ok(Action::Hide),
        _ => {}
    }
    if let Some(hint) = whistle_syntax_hint(input) {
        return Err(RuleModelError::unsupported("action", hint));
    }

    let (name, args) = parse_call(input)?;
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
        "map.remote" | "mapRemote" | "map_remote" | "map-remote" => {
            let value = parse_value(require_one(&args, "map.remote")?)?;
            validate_map_remote_target(&value)?;
            Ok(Action::MapRemote(value))
        }
        "mock.raw" | "mockRaw" | "mock_raw" | "mock-raw" => Ok(Action::MockRaw(parse_value(
            require_one(&args, "mock.raw")?,
        )?)),
        "status" => {
            let raw = require_one(&args, "status")?;
            let code = raw.parse::<u16>().map_err(|source| {
                RuleModelError::integer("status code", raw, "status code must be numeric", source)
            })?;
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
            Ok(Action::Redirect {
                url: parse_value(args[0])?,
                code,
            })
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
        "res.trailer" => Ok(Action::ResTrailer(parse_header_op(require_call_body(
            input,
            "res.trailer",
        )?)?)),
        "attachment" => Ok(Action::Attachment(
            args.first()
                .filter(|value| !value.trim().is_empty())
                .map(|value| parse_value(value))
                .transpose()?,
        )),
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
        "skip" => Ok(Action::Skip(args.iter().map(|s| unquote(s)).collect())),
        _ => Err(RuleModelError::unsupported(
            "action",
            format!("unknown action `{name}`"),
        )),
    }?;
    action.validate_templates()?;
    Ok(action)
}
