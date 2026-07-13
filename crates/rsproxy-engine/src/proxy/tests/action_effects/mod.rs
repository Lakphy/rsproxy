use super::*;
use rsproxy_rules::Action;
use std::collections::BTreeSet;

mod control_flow;
mod harness;
mod local_routing;
mod request;
mod response;
mod tls;

use harness::*;

const REQUEST_FAMILIES: &[&str] = &[
    "req.header",
    "req.method",
    "req.cookie",
    "req.ua",
    "req.referer",
    "req.auth",
    "req.forwarded",
    "req.type",
    "req.charset",
    "url.rewrite",
    "url.query",
    "delete",
    "req.body.set",
    "req.body.prepend",
    "req.body.append",
    "req.body.replace",
];

const RESPONSE_FAMILIES: &[&str] = &[
    "res.header",
    "res.status",
    "res.cookie",
    "res.cors",
    "res.type",
    "res.charset",
    "res.merge",
    "res.trailer",
    "attachment",
    "cache",
    "res.body.set",
    "res.body.prepend",
    "res.body.append",
    "res.body.replace",
    "inject",
];

const LOCAL_ROUTING_FAMILIES: &[&str] =
    &["host", "upstream", "direct", "mock", "status", "redirect"];

const CONTROL_FLOW_FAMILIES: &[&str] = &[
    "delay.req",
    "delay.res",
    "throttle.req",
    "throttle.res",
    "bypass",
    "hide",
    "tag",
    "skip",
];

const TLS_FAMILIES: &[&str] = &["tls"];

#[test]
fn every_action_family_has_one_executable_effect_owner() {
    let mut actual = BTreeSet::new();
    for family in REQUEST_FAMILIES
        .iter()
        .chain(RESPONSE_FAMILIES)
        .chain(LOCAL_ROUTING_FAMILIES)
        .chain(CONTROL_FLOW_FAMILIES)
        .chain(TLS_FAMILIES)
    {
        assert!(
            actual.insert(*family),
            "duplicate effect owner for {family}"
        );
    }
    let expected = Action::FAMILIES.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
}
