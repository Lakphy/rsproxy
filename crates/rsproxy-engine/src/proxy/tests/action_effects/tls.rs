use super::super::support;
use super::*;
use std::io::{Read, Write};

#[test]
fn tls_family_reaches_the_upstream_handshake_with_selected_policy() {
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
    let rules = format!(
        "tls-effect.test host({origin_address}) \
         tls(min=1.3, ciphers=TLS_AES_128_GCM_SHA256)"
    );
    let state = support::isolated_state("action-tls", &rules);
    let (proxy, worker) = support::spawn_proxy(state.clone(), 1);
    let mut client = support::connect_client(proxy);
    support::connect_request(&mut client, "tls-effect.test:443");
    let mut tls = support::h1_tls_client(client, &state, "tls-effect.test");
    tls.write_all(b"GET /policy HTTP/1.1\r\nHost: tls-effect.test\r\nConnection: close\r\n\r\n")
        .unwrap();
    tls.flush().unwrap();
    let head = http::read_response_head(&mut tls, 64 * 1024, 128).unwrap();
    assert_eq!(head.status, 502);
    let _ = read_response_body(&mut tls, &head.headers).unwrap();
    drop(tls);

    let record = origin.join().unwrap();
    worker.join().unwrap();
    assert_eq!(record[0], 0x16, "origin should receive a TLS handshake");
    let session = state
        .trace
        .list(10)
        .into_iter()
        .find(|session| session.url.contains("/policy"))
        .unwrap();
    assert!(session.flags.contains(&"upstream-tls-policy".to_string()));
    assert!(session.flags.contains(&"upstream-tls-min:1.3".to_string()));
    assert!(
        session
            .flags
            .contains(&"upstream-tls-ciphers:1".to_string())
    );
    cleanup_state(&state);
}
