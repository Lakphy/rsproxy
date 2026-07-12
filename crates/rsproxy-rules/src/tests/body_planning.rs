use super::*;

#[test]
fn request_body_planning_uses_candidates_and_known_conditions() {
    let rules = RuleSet::parse(
        "default",
        "upload.test req.body.append(\"!\") when method(POST)\nother.test res.header(x-body: yes) when body(~token)\nupload.test res.header(x-late: yes) when method(POST) when body(~token) when status(200)",
    )
    .unwrap();
    let mut upload = req("http://upload.test/data");
    upload.method = "POST".to_string();
    assert!(rules.request_body_required(&upload));

    upload.method = "GET".to_string();
    assert!(!rules.request_body_required(&upload));

    let mut unrelated = req("http://unrelated.test/data");
    unrelated.method = "POST".to_string();
    assert!(!rules.request_body_required(&unrelated));

    let mut late = req("http://upload.test/data");
    late.method = "POST".to_string();
    let response_only = RuleSet::parse(
        "default",
        "upload.test res.header(x-late: yes) when body(~token) when status(200)",
    )
    .unwrap();
    assert!(response_only.request_body_required(&late));
}

#[test]
fn bodyless_resolution_skips_only_body_dependent_behavior() {
    let rules = RuleSet::parse(
        "default",
        "example.com req.header(x-kept: yes) req.body.append(\"!\")\nexample.com res.header(x-body: yes) when body(~token)\nexample.com res.header(x-status: yes) when status(200)",
    )
    .unwrap();
    let request = req("http://example.com/");
    let resolved = rules.resolve_without_request_body(&request);

    assert_eq!(resolved.actions.len(), 1);
    assert!(matches!(
        resolved.actions[0].action,
        Action::ReqHeader(HeaderOp::Set { ref name, .. }) if name == "x-kept"
    ));

    let response = rules.resolve_response_without_request_body(
        &request,
        &ResponseMeta {
            status: 200,
            headers: Vec::new(),
        },
    );
    assert_eq!(response.actions.len(), 2);
    assert!(response.actions.iter().any(|item| matches!(
        item.action,
        Action::ReqHeader(HeaderOp::Set { ref name, .. }) if name == "x-kept"
    )));
    assert!(response.actions.iter().any(|item| matches!(
        item.action,
        Action::ResHeader(HeaderOp::Set { ref name, .. }) if name == "x-status"
    )));
    assert!(!response.actions.iter().any(|item| matches!(
        item.action,
        Action::ResHeader(HeaderOp::Set { ref name, .. }) if name == "x-body"
    )));
}

#[test]
fn delete_request_body_paths_participate_in_bounded_body_planning() {
    let rules = RuleSet::parse(
        "default",
        "example.com delete(reqHeaders.x-old, reqBody.profile.secret)",
    )
    .unwrap();
    let request = req("http://example.com/");

    assert!(rules.request_body_required(&request));
    let resolved = rules.resolve_without_request_body(&request);
    assert!(matches!(
        &resolved.actions[0].action,
        Action::Delete(operations)
            if operations == &[DeleteOp::ReqHeader("x-old".to_string())]
    ));
}
