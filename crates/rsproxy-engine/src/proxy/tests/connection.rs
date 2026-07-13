use super::*;

#[test]
fn client_persistence_follows_http_version_and_connection_tokens() {
    let request = |version: &str, headers: Vec<(String, String)>| RawRequest {
        method: "GET".to_string(),
        target: "http://example.test/".to_string(),
        version: version.to_string(),
        headers,
        body: Vec::new(),
        trailers: Vec::new(),
    };

    assert_eq!(
        requested_client_connection(&request("HTTP/1.1", Vec::new())),
        ClientPersistence::KeepAlive
    );
    assert_eq!(
        requested_client_connection(&request(
            "HTTP/1.1",
            vec![("Connection".to_string(), "close".to_string())]
        )),
        ClientPersistence::Close
    );
    assert_eq!(
        requested_client_connection(&request("HTTP/1.0", Vec::new())),
        ClientPersistence::Close
    );
    assert_eq!(
        requested_client_connection(&request(
            "HTTP/1.0",
            vec![("Proxy-Connection".to_string(), "Keep-Alive".to_string())]
        )),
        ClientPersistence::KeepAlive
    );
}

#[test]
fn client_connection_processes_pipelined_requests_in_order() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut state = test_state();
    state.rules = RuleStore::from_compiled(
        &state.config.storage,
        rsproxy_rules::RuleSet::parse("default", "example.test status(209)").unwrap(),
    );
    let server_state = state.clone();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        handle_client(stream, server_state)
    });

    let mut client = TcpStream::connect(addr).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
            .write_all(
                b"GET http://example.test/one HTTP/1.1\r\nHost: example.test\r\n\r\nGET http://example.test/two HTTP/1.1\r\nHost: example.test\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    server.join().unwrap().unwrap();

    assert_eq!(response.matches("HTTP/1.1 209 OK\r\n").count(), 2);
    let keep_alive = response.find("Connection: keep-alive\r\n").unwrap();
    let close = response.rfind("Connection: close\r\n").unwrap();
    assert!(keep_alive < close);

    let sessions = state.trace.list(10);
    assert_eq!(sessions.len(), 2);
    let first = sessions
        .iter()
        .find(|session| session.url.ends_with("/one"))
        .unwrap();
    let second = sessions
        .iter()
        .find(|session| session.url.ends_with("/two"))
        .unwrap();
    assert!(first.flags.contains(&"h1-client-keepalive".to_string()));
    assert!(second.flags.contains(&"h1-client-close".to_string()));
    assert!(
        second
            .flags
            .contains(&"h1-client-connection-reused".to_string())
    );
    assert_eq!(first.client, second.client);
}

#[test]
fn proxy_auth_accepts_case_insensitive_basic_scheme_and_whitespace() {
    let request = request_with_proxy_authorization(Some("basic\t dXNlcjpwYXNz  "));

    assert!(authorized(&request, Some("user:pass")));
    assert!(authorized(&request_with_proxy_authorization(None), None));
}

#[test]
fn proxy_auth_rejects_missing_wrong_or_malformed_credentials() {
    assert!(!authorized(
        &request_with_proxy_authorization(None),
        Some("user:pass")
    ));
    assert!(!authorized(
        &request_with_proxy_authorization(Some("Basic dXNlcjp3cm9uZw==")),
        Some("user:pass")
    ));
    assert!(!authorized(
        &request_with_proxy_authorization(Some("Bearer dXNlcjpwYXNz")),
        Some("user:pass")
    ));
    assert!(!authorized(
        &request_with_proxy_authorization(Some("Basic dXNlcjpwYXNz extra")),
        Some("user:pass")
    ));
}

#[test]
fn proxy_auth_credentials_are_stripped_before_dispatch() {
    let mut configured = request_with_proxy_authorization(Some("Basic dXNlcjpwYXNz"));
    assert!(authorize_and_strip_proxy_credentials(
        &mut configured,
        Some("user:pass")
    ));
    assert!(http::header(&configured.headers, "proxy-authorization").is_none());

    let mut disabled = request_with_proxy_authorization(Some("Basic dW5leHBlY3RlZDpjcmVkZW50aWFs"));
    assert!(authorize_and_strip_proxy_credentials(&mut disabled, None));
    assert!(http::header(&disabled.headers, "proxy-authorization").is_none());
}
