use super::*;
use std::fs;

fn request(url: &str) -> RequestMeta {
    RequestMeta {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: Default::default(),
    }
}

#[test]
fn request_advisory_requires_secure_matching_map_remote() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-advisory-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    let rules = RuleSet::parse("default", "secure.test map.remote(https://local.test)").unwrap();

    assert!(
        request_advisories(&rules, &request("http://secure.test/"), &storage, false).is_empty()
    );
    assert_eq!(
        request_advisories(&rules, &request("https://secure.test/"), &storage, false)[0].kind,
        HTTPS_MITM_UNAVAILABLE
    );
    assert!(
        request_advisories(&rules, &request("https://other.test/"), &storage, false).is_empty()
    );

    let ca_directory = storage.join("ca");
    rsproxy_platform::ca::initialize_root_ca(&ca_directory, "test CA", false).unwrap();
    assert!(
        request_advisories(&rules, &request("https://secure.test/"), &storage, false).is_empty()
    );
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn disabled_mitm_is_reported_even_with_initialized_ca() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-advisory-disabled-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    let rules = RuleSet::parse("default", "secure.test map.remote(https://local.test)").unwrap();
    let advisory = request_advisories(&rules, &request("https://secure.test/"), &storage, true);

    assert!(advisory[0].message.contains("disabled by configuration"));
    let _ = fs::remove_dir_all(storage);
}
