use rsproxy_rules::*;

struct ValueSlot {
    name: &'static str,
    action: &'static str,
}

const VALUE_SLOTS: &[ValueSlot] = &[
    ValueSlot {
        name: "host.address",
        action: "host({value})",
    },
    ValueSlot {
        name: "upstream",
        action: "upstream({value})",
    },
    ValueSlot {
        name: "mock",
        action: "mock({value})",
    },
    ValueSlot {
        name: "mock.raw",
        action: "mock.raw({value})",
    },
    ValueSlot {
        name: "redirect.url",
        action: "redirect({value})",
    },
    ValueSlot {
        name: "req.header.value",
        action: "req.header(x-value: {value})",
    },
    ValueSlot {
        name: "res.header.value",
        action: "res.header(x-value: {value})",
    },
    ValueSlot {
        name: "req.method",
        action: "req.method({value})",
    },
    ValueSlot {
        name: "req.cookie.value",
        action: "req.cookie(sid={value})",
    },
    ValueSlot {
        name: "req.cookie.attribute",
        action: "req.cookie(sid=fixed; Path={value})",
    },
    ValueSlot {
        name: "res.cookie.value",
        action: "res.cookie(sid={value})",
    },
    ValueSlot {
        name: "res.cookie.attribute",
        action: "res.cookie(sid=fixed; Path={value})",
    },
    ValueSlot {
        name: "req.ua",
        action: "req.ua({value})",
    },
    ValueSlot {
        name: "req.referer",
        action: "req.referer({value})",
    },
    ValueSlot {
        name: "req.auth",
        action: "req.auth({value})",
    },
    ValueSlot {
        name: "req.forwarded",
        action: "req.forwarded({value})",
    },
    ValueSlot {
        name: "req.type",
        action: "req.type({value})",
    },
    ValueSlot {
        name: "req.charset",
        action: "req.charset({value})",
    },
    ValueSlot {
        name: "res.cors.origin",
        action: "res.cors({value})",
    },
    ValueSlot {
        name: "res.cors.methods",
        action: "res.cors(*, methods={value})",
    },
    ValueSlot {
        name: "res.cors.headers",
        action: "res.cors(*, headers={value})",
    },
    ValueSlot {
        name: "res.cors.expose",
        action: "res.cors(*, expose={value})",
    },
    ValueSlot {
        name: "res.cors.max-age",
        action: "res.cors(*, max-age={value})",
    },
    ValueSlot {
        name: "res.type",
        action: "res.type({value})",
    },
    ValueSlot {
        name: "res.charset",
        action: "res.charset({value})",
    },
    ValueSlot {
        name: "res.merge",
        action: "res.merge({value})",
    },
    ValueSlot {
        name: "res.trailer.value",
        action: "res.trailer(x-value: {value})",
    },
    ValueSlot {
        name: "attachment",
        action: "attachment({value})",
    },
    ValueSlot {
        name: "cache.directive",
        action: "cache(max-age={value})",
    },
    ValueSlot {
        name: "url.rewrite.from",
        action: "url.rewrite({value}, /to)",
    },
    ValueSlot {
        name: "url.rewrite.to",
        action: "url.rewrite(/from, {value})",
    },
    ValueSlot {
        name: "url.query.value",
        action: "url.query(value={value})",
    },
    ValueSlot {
        name: "req.body.set",
        action: "req.body.set({value})",
    },
    ValueSlot {
        name: "req.body.prepend",
        action: "req.body.prepend({value})",
    },
    ValueSlot {
        name: "req.body.append",
        action: "req.body.append({value})",
    },
    ValueSlot {
        name: "res.body.set",
        action: "res.body.set({value})",
    },
    ValueSlot {
        name: "res.body.prepend",
        action: "res.body.prepend({value})",
    },
    ValueSlot {
        name: "res.body.append",
        action: "res.body.append({value})",
    },
    ValueSlot {
        name: "inject.value",
        action: "inject(html, {value})",
    },
    ValueSlot {
        name: "tag",
        action: "tag({value})",
    },
];

#[test]
fn every_structured_value_slot_accepts_inline_template_reference_and_file_sources() {
    let sources = [
        ("inline", Value::inline("inline")),
        (r#""${host}-$1""#, Value::inline("${host}-$1")),
        ("@fixture", Value::Reference("fixture".to_string())),
        ("<fixture.txt>", Value::File("fixture.txt".to_string())),
    ];

    for slot in VALUE_SLOTS {
        for (source, expected) in &sources {
            let action = slot.action.replace("{value}", source);
            let rule = format!("example.test {action}");
            let parsed = RuleSet::parse("matrix", &rule)
                .unwrap_or_else(|errors| panic!("{} with {source}: {errors:?}", slot.name));
            let actual = value_at(slot.name, &parsed.rules()[0].actions[0]);
            assert_eq!(actual, expected, "{} with {source}", slot.name);
        }
    }
}

fn value_at<'a>(slot: &str, action: &'a Action) -> &'a Value {
    match (slot, action) {
        ("host.address", Action::Host(pool)) => &pool.addresses()[0],
        ("upstream", Action::Upstream(value))
        | ("mock", Action::Mock(value))
        | ("mock.raw", Action::MockRaw(value))
        | ("req.method", Action::ReqMethod(value))
        | ("req.ua", Action::ReqUa(value))
        | ("req.referer", Action::ReqReferer(value))
        | ("req.auth", Action::ReqAuth(value))
        | ("req.forwarded", Action::ReqForwarded(value))
        | ("req.type", Action::ReqType(value))
        | ("req.charset", Action::ReqCharset(value))
        | ("res.type", Action::ResType(value))
        | ("res.charset", Action::ResCharset(value))
        | ("res.merge", Action::ResMerge(value))
        | ("tag", Action::Tag(value)) => value,
        ("redirect.url", Action::Redirect { url, .. }) => url,
        ("req.header.value", Action::ReqHeader(HeaderOp::Set { value, .. }))
        | ("res.header.value", Action::ResHeader(HeaderOp::Set { value, .. }))
        | ("res.trailer.value", Action::ResTrailer(HeaderOp::Set { value, .. })) => value,
        ("req.cookie.value", Action::ReqCookie(CookieOp::Set { value, .. }))
        | ("res.cookie.value", Action::ResCookie(CookieOp::Set { value, .. })) => value,
        ("req.cookie.attribute", Action::ReqCookie(CookieOp::Set { attrs, .. }))
        | ("res.cookie.attribute", Action::ResCookie(CookieOp::Set { attrs, .. })) => {
            attrs[0].value.as_ref().unwrap()
        }
        ("res.cors.origin", Action::ResCors(operation)) => &operation.origin,
        ("res.cors.methods", Action::ResCors(operation)) => operation.methods.as_ref().unwrap(),
        ("res.cors.headers", Action::ResCors(operation)) => operation.headers.as_ref().unwrap(),
        ("res.cors.expose", Action::ResCors(operation)) => operation.expose.as_ref().unwrap(),
        ("res.cors.max-age", Action::ResCors(operation)) => operation.max_age.as_ref().unwrap(),
        ("attachment", Action::Attachment(filename)) => filename.as_ref().unwrap(),
        ("cache.directive", Action::Cache(CacheOp::Directives(directives))) => {
            directives[0].value.as_ref().unwrap()
        }
        (
            "url.rewrite.from",
            Action::UrlRewrite {
                from: UrlRewritePattern::Plain(value),
                ..
            },
        ) => value,
        ("url.rewrite.to", Action::UrlRewrite { to, .. }) => to,
        ("url.query.value", Action::UrlQuery(operations)) => match &operations[0] {
            QueryOp::Set { value, .. } => value,
            QueryOp::Remove { .. } => panic!("expected query set operation"),
        },
        ("req.body.set", Action::ReqBody(BodyOp::Set(value)))
        | ("req.body.prepend", Action::ReqBody(BodyOp::Prepend(value)))
        | ("req.body.append", Action::ReqBody(BodyOp::Append(value)))
        | ("res.body.set", Action::ResBody(BodyOp::Set(value)))
        | ("res.body.prepend", Action::ResBody(BodyOp::Prepend(value)))
        | ("res.body.append", Action::ResBody(BodyOp::Append(value))) => value,
        ("inject.value", Action::Inject(operation)) => &operation.value,
        _ => panic!("value slot {slot} resolved to unexpected action {action:?}"),
    }
}
