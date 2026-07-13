use rsproxy_control::{
    ControlError, ControlListener, ControlOptions, ControlResult, ControlState, api_request,
    api_token_path, bind, prepare_server_api_auth, resolve_client_api_token, serve, set_api_token,
    unix_api_path, validate_api_token, windows_pipe_path,
};
use rsproxy_engine::{EngineError, ProxyConfig, SharedState};
use std::time::Duration;

#[test]
fn typed_error_facade_is_public() {
    fn assert_error<T: std::error::Error + Send + Sync + 'static>() {}
    fn accept_result(_: ControlResult<()>) {}
    fn convert_engine(error: EngineError) -> ControlError {
        error.into()
    }

    assert_error::<ControlError>();
    accept_result(Err(ControlError::HttpStatus {
        status: 503,
        body: "control unavailable".to_string(),
    }));
    let _convert: fn(EngineError) -> ControlError = convert_engine;
}

#[test]
fn public_facade_composes_an_engine_handle_with_control_options() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-control-public-api-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    let mut proxy = ProxyConfig::new(&storage);
    proxy.trace_disk_budget = 0;
    let engine = SharedState::new(proxy).unwrap();
    let mut options = ControlOptions {
        host: "127.0.0.1".to_string(),
        port: 8899,
        api: "127.0.0.1:0".to_string(),
        api_token: None,
        storage: storage.clone(),
        config_path: None,
        rules_watch: false,
        rules_watch_debounce: Duration::from_millis(200),
        max_header_size: 256 * 1024,
        max_header_count: 256,
        max_body_size: 8 * 1024 * 1024,
    };
    options.api_token = Some("public-api-secret-token".to_string());
    let debug = format!("{options:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("public-api-secret-token"));
    let _state = ControlState::new(options, engine.handle());
    let listener = bind("127.0.0.1:0").unwrap();
    assert!(listener.endpoint().unwrap().starts_with("127.0.0.1:"));

    let _request: fn(&str, &str, &str, &str) -> ControlResult<String> = api_request;
    let _bind: fn(&str) -> ControlResult<ControlListener> = bind;
    let _endpoint: fn(&ControlListener) -> ControlResult<String> = ControlListener::endpoint;
    let _serve: fn(ControlListener, ControlState) -> ControlResult<()> = serve;
    set_api_token(None);
    let _ = std::fs::remove_dir_all(storage);
}

#[test]
fn public_client_auth_and_endpoint_vocabulary_is_stable() {
    assert_eq!(
        unix_api_path("unix:/tmp/rsproxy.sock"),
        Some("/tmp/rsproxy.sock")
    );
    assert_eq!(windows_pipe_path("pipe:rsproxy"), Some("rsproxy"));
    assert!(validate_api_token("too-short").is_err());

    let storage = std::env::temp_dir().join("rsproxy-control-public-auth");
    let mut token = Some("0123456789abcdef".to_string());
    prepare_server_api_auth("unix:/tmp/rsproxy.sock", &storage, &mut token).unwrap();
    assert_eq!(token, None);
    assert_eq!(api_token_path(&storage), storage.join("run/api-token"));
    assert_eq!(
        resolve_client_api_token(
            "unix:/tmp/rsproxy.sock",
            &storage,
            Some("0123456789abcdef".to_string()),
            None,
            None,
        )
        .unwrap(),
        None
    );
}
