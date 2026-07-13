use rsproxy_engine::{EngineError, RuleStoreError};
use rsproxy_net::{NetError, NetStage};
use rsproxy_rules::{RuleModelError, UrlParts};
use std::error::Error as _;
use std::io;

#[test]
fn net_conversion_preserves_the_source_chain() {
    let error = EngineError::from(NetError::Timeout {
        stage: NetStage::Connect,
        timeout_ms: 250,
    });

    let source = error.source().expect("network source should be retained");
    assert!(source.is::<NetError>());
    assert_eq!(
        source.to_string(),
        "timeout during connection establishment after 250 ms"
    );
}

#[test]
fn certificate_conversion_retains_rcgen_error_type() {
    let error = EngineError::from(rcgen::Error::CouldNotParseKeyPair);
    let source = error
        .source()
        .expect("certificate source should be retained");
    assert!(source.is::<rcgen::Error>());
}

#[test]
fn rule_model_conversion_preserves_the_source_chain() {
    let source = UrlParts::parse("not-a-url").expect_err("relative input is not an absolute URL");
    let error = EngineError::from(source);

    assert!(error.to_string().starts_with("invalid input:"));
    let source = error
        .source()
        .expect("rule-model source should be retained");
    assert!(source.is::<RuleModelError>());
}

#[test]
fn rule_store_io_source_is_retained_through_engine_error() {
    let rule_store = RuleStoreError::Io {
        context: "read rules manifest".to_string(),
        source: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
    };
    let error = EngineError::from(rule_store);

    let source = error
        .source()
        .expect("rule-store source should be retained");
    assert!(source.is::<RuleStoreError>());
    let io_source = source
        .source()
        .expect("rule-store I/O source should be retained");
    assert!(io_source.is::<io::Error>());
}

#[test]
fn rule_watcher_source_is_retained() {
    let error = RuleStoreError::Watch(notify::Error::generic("watch failed"));

    let source = error.source().expect("notify source should be retained");
    assert!(source.is::<notify::Error>());
}
