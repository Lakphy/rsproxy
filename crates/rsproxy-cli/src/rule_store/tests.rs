use super::*;
use rsproxy_rules::Action;

fn temp_storage(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rsproxy-rule-store-{name}-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ))
}

fn request() -> rsproxy_rules::RequestMeta {
    rsproxy_rules::RequestMeta {
        method: "GET".to_string(),
        url: "http://example.test/".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: Default::default(),
    }
}

#[test]
fn legacy_rule_files_are_discovered_in_deterministic_order() {
    let storage = temp_storage("legacy");
    let rules_dir = storage.join("rules");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::write(rules_dir.join("z.rules"), "example.test status(203)").unwrap();
    fs::write(rules_dir.join("default.rules"), "example.test status(201)").unwrap();
    fs::write(rules_dir.join("a.rules"), "example.test status(202)").unwrap();

    let store = RuleStore::load(&storage).unwrap();
    let snapshot = store.snapshot();
    assert_eq!(
        snapshot
            .groups
            .iter()
            .map(|group| group.name.as_str())
            .collect::<Vec<_>>(),
        vec!["default", "a", "z"]
    );
    assert!(matches!(
        snapshot.compiled.resolve(&request()).actions[0].action,
        Action::Status(201)
    ));

    let _ = fs::remove_dir_all(storage);
}

#[test]
fn set_disable_and_reload_preserve_group_order_and_state() {
    let storage = temp_storage("lifecycle");
    let store = RuleStore::load(&storage).unwrap();
    store
        .set_group("default", "example.test status(201)".to_string())
        .unwrap();
    store
        .set_group("later", "example.test status(202) @important".to_string())
        .unwrap();
    assert!(matches!(
        store.snapshot().compiled.resolve(&request()).actions[0].action,
        Action::Status(202)
    ));

    store.set_enabled("later", false).unwrap();
    assert!(matches!(
        store.snapshot().compiled.resolve(&request()).actions[0].action,
        Action::Status(201)
    ));

    let reloaded = RuleStore::load(&storage).unwrap().snapshot();
    assert_eq!(reloaded.groups[1].name, "later");
    assert!(!reloaded.groups[1].enabled);
    assert!(matches!(
        reloaded.compiled.resolve(&request()).actions[0].action,
        Action::Status(201)
    ));

    let _ = fs::remove_dir_all(storage);
}

#[test]
fn invalid_update_does_not_change_snapshot_or_group_file() {
    let storage = temp_storage("invalid");
    let store = RuleStore::load(&storage).unwrap();
    store
        .set_group("default", "example.test status(201)".to_string())
        .unwrap();

    let error = store
        .set_group("default", "example.test unknown()".to_string())
        .unwrap_err();
    assert!(matches!(error, RuleStoreError::Parse(_)));
    assert_eq!(
        store.snapshot().group("default").unwrap().text,
        "example.test status(201)"
    );
    assert_eq!(
        fs::read_to_string(storage.join("rules/default.rules")).unwrap(),
        "example.test status(201)"
    );

    let _ = fs::remove_dir_all(storage);
}

#[test]
fn published_snapshots_keep_in_flight_requests_on_their_original_rules() {
    let storage = temp_storage("snapshot");
    let store = RuleStore::load(&storage).unwrap();
    store
        .set_group("default", "example.test status(201)".to_string())
        .unwrap();
    let in_flight = store.snapshot();

    store
        .set_group("default", "example.test status(202)".to_string())
        .unwrap();
    assert!(matches!(
        in_flight.compiled.resolve(&request()).actions[0].action,
        Action::Status(201)
    ));
    assert!(matches!(
        store.snapshot().compiled.resolve(&request()).actions[0].action,
        Action::Status(202)
    ));

    let _ = fs::remove_dir_all(storage);
}

#[test]
fn removal_is_persisted_and_default_is_protected() {
    let storage = temp_storage("remove");
    let store = RuleStore::load(&storage).unwrap();
    store
        .set_group("temporary", "example.test status(202)".to_string())
        .unwrap();
    store.remove_group("temporary").unwrap();
    assert!(store.snapshot().group("temporary").is_none());
    assert!(!storage.join("rules/temporary.rules").exists());
    assert!(store.remove_group("default").is_err());
    assert!(store.set_group("../escape", String::new()).is_err());

    let reloaded = RuleStore::load(&storage).unwrap().snapshot();
    assert_eq!(reloaded.groups.len(), 1);
    assert_eq!(reloaded.groups[0].name, "default");

    let _ = fs::remove_dir_all(storage);
}
