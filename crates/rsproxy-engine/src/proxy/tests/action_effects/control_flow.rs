use super::super::support;
use super::*;
use std::io::{Read, Write};

#[test]
fn request_and_response_delay_families_delay_their_respective_paths() {
    let request_state = state_with_rules(
        "delay-request",
        "delay-effect.test delay(req, 35ms) status(209)",
    );
    let exchange = run_exchange(
        &request_state,
        "GET",
        "http://delay-effect.test/request",
        &[],
        &[],
    );
    assert_eq!(exchange.head.status, 209);
    assert!(exchange.elapsed >= Duration::from_millis(25));
    cleanup_state(&request_state);

    let origin = TestOrigin::spawn(OriginReply::ok("delay-ok"));
    let response_state = state_with_rules("delay-response", "127.0.0.1 delay(res, 35ms)");
    let url = format!("http://{}/response", origin.address);
    let exchange = run_exchange(&response_state, "GET", &url, &[], &[]);
    let _ = origin.finish();
    assert_eq!(exchange.body.body, b"delay-ok");
    assert!(exchange.elapsed >= Duration::from_millis(25));
    cleanup_state(&response_state);
}

#[test]
fn request_and_response_throttle_families_pace_real_bodies() {
    let request_body = vec![b'q'; 32 * 1024];
    let request_origin = TestOrigin::spawn(OriginReply::ok("upload-ok"));
    let request_state = state_with_rules("throttle-request", "127.0.0.1 throttle(req, 1MB/s)");
    let url = format!("http://{}/upload", request_origin.address);
    let exchange = run_exchange(&request_state, "POST", &url, &[], &request_body);
    let observed = request_origin.finish();
    assert_eq!(observed.body, request_body);
    assert_eq!(exchange.body.body, b"upload-ok");
    assert!(exchange.elapsed >= Duration::from_millis(10));
    cleanup_state(&request_state);

    let response_body = vec![b'r'; 32 * 1024];
    let response_origin = TestOrigin::spawn(OriginReply::ok(response_body.clone()));
    let response_state = state_with_rules("throttle-response", "127.0.0.1 throttle(res, 1MB/s)");
    let url = format!("http://{}/download", response_origin.address);
    let exchange = run_exchange(&response_state, "GET", &url, &[], &[]);
    let _ = response_origin.finish();
    assert_eq!(exchange.body.body, response_body);
    assert!(exchange.elapsed >= Duration::from_millis(10));
    cleanup_state(&response_state);
}

#[test]
fn tag_hide_and_skip_change_trace_or_forwarding_observably() {
    let tag_origin = TestOrigin::spawn(OriginReply::ok("tag-ok"));
    let tag_state = state_with_rules("tag", "127.0.0.1 tag(effect-${path})");
    let url = format!("http://{}/tagged", tag_origin.address);
    let _ = run_exchange(&tag_state, "GET", &url, &[], &[]);
    let _ = tag_origin.finish();
    let session = tag_state.trace.list(1).pop().unwrap();
    assert!(session.flags.contains(&"tag:effect-/tagged".to_string()));
    cleanup_state(&tag_state);

    let hide_origin = TestOrigin::spawn(OriginReply::ok("hide-ok"));
    let hide_state = state_with_rules("hide", "127.0.0.1 hide");
    let url = format!("http://{}/hidden", hide_origin.address);
    let exchange = run_exchange(&hide_state, "GET", &url, &[], &[]);
    let _ = hide_origin.finish();
    assert_eq!(exchange.body.body, b"hide-ok");
    assert!(hide_state.trace.list(10).is_empty());
    cleanup_state(&hide_state);

    let skip_origin = TestOrigin::spawn(OriginReply::ok("skip-ok"));
    let skip_state = state_with_rules(
        "skip",
        "127.0.0.1 skip(req.header) req.header(x-skipped: no) res.header(x-kept: yes)",
    );
    let url = format!("http://{}/skip", skip_origin.address);
    let exchange = run_exchange(&skip_state, "GET", &url, &[], &[]);
    let request = skip_origin.finish();
    assert_eq!(header(&request.headers, "x-skipped"), None);
    assert_eq!(header(&exchange.head.headers, "x-kept"), Some("yes"));
    cleanup_state(&skip_state);
}

#[test]
fn bypass_family_sends_a_real_client_hello_to_the_origin() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_address = listener.local_addr().unwrap();
    let origin = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut record = [0u8; 5];
        stream.read_exact(&mut record).unwrap();
        record
    });
    let rules = format!("bypass-effect.test host({origin_address}) bypass");
    let state = support::isolated_state("action-bypass", &rules);
    let (proxy, worker) = support::spawn_proxy(state.clone(), 1);
    let mut client = support::connect_client(proxy);
    support::connect_request(&mut client, "bypass-effect.test:443");
    let mut tls = support::h1_tls_client(client, &state, "bypass-effect.test");
    let _ = tls.write_all(b"GET / HTTP/1.1\r\nHost: bypass-effect.test\r\n\r\n");
    drop(tls);

    let record = origin.join().unwrap();
    worker.join().unwrap();
    assert_eq!(record[0], 0x16, "origin should receive a TLS handshake");
    let session = state.trace.list(1).pop().unwrap();
    assert!(session.flags.contains(&"bypass".to_string()));
    cleanup_state(&state);
}
