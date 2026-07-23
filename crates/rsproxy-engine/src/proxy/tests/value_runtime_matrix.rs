use super::*;

mod cases;

use cases::{VALUE_SLOTS, value_at};

struct RuntimeSource {
    syntax: &'static str,
    expected: &'static str,
}

#[test]
fn every_value_slot_resolves_all_runtime_source_and_capture_forms() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-value-runtime-matrix-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    fs::create_dir_all(storage.join("values")).unwrap();
    fs::create_dir_all(storage.join("files")).unwrap();
    fs::write(
        storage.join("values/shared"),
        b"reference-${host}-$2-${kind}",
    )
    .unwrap();
    fs::write(storage.join("files/value.txt"), b"file-${host}-$2-${kind}").unwrap();
    let mut state = test_state();
    state.config.storage = storage.clone();
    let request = meta("http://example.test/items/42");
    let sources = [
        RuntimeSource {
            syntax: "plain",
            expected: "plain",
        },
        RuntimeSource {
            syntax: r#""quoted-value""#,
            expected: "quoted-value",
        },
        RuntimeSource {
            syntax: "@shared",
            expected: "reference-example.test-42-items",
        },
        RuntimeSource {
            syntax: "<files/value.txt>",
            expected: "file-example.test-42-items",
        },
        RuntimeSource {
            syntax: r#""${host}""#,
            expected: "example.test",
        },
        RuntimeSource {
            syntax: r#""$2-${kind}""#,
            expected: "42-items",
        },
    ];

    for slot in VALUE_SLOTS {
        for source in &sources {
            let action = slot.action.replace("{value}", source.syntax);
            let rule = format!(r"/\/(?P<kind>items)\/(\d+)/ {action}");
            let rules = RuleSet::parse("runtime-matrix", &rule).unwrap_or_else(|errors| {
                panic!("{} with {}: {errors:?}", slot.name, source.syntax)
            });
            let resolved = rules.resolve(&request);
            let item = resolved
                .actions
                .first()
                .unwrap_or_else(|| panic!("{} did not resolve", slot.name));
            let actual =
                resolve_value_text(value_at(slot.name, &item.action), item, &request, &state)
                    .unwrap_or_else(|error| {
                        panic!("{} with {}: {error}", slot.name, source.syntax)
                    });
            assert_eq!(
                actual, source.expected,
                "{} with {}",
                slot.name, source.syntax
            );
        }
    }
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn every_value_slot_rejects_an_invalid_reference_key() {
    for slot in VALUE_SLOTS {
        let action = slot.action.replace("{value}", "@../escape");
        let rule = format!("example.test {action}");
        let errors = match RuleSet::parse("runtime-matrix", &rule) {
            Ok(_) => panic!("{} accepted invalid reference", slot.name),
            Err(errors) => errors,
        };
        assert_eq!(errors[0].code.as_str(), "action", "{}", slot.name);
        assert!(
            errors[0].message.contains("invalid value key"),
            "{}",
            slot.name
        );
    }
}

#[test]
fn external_value_reads_reject_one_byte_beyond_the_public_limit() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-value-limit-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    fs::create_dir_all(storage.join("values")).unwrap();
    let oversized = vec![b'x'; rsproxy_rules::MAX_RULE_EXTERNAL_VALUE_BYTES + 1];
    fs::write(storage.join("values/large"), &oversized).unwrap();
    fs::write(storage.join("large.txt"), &oversized).unwrap();
    let mut state = test_state();
    state.config.storage = storage.clone();
    let request = meta("http://example.test/");

    for syntax in ["@large", "<large.txt>"] {
        let rules = RuleSet::parse("limit", &format!("example.test tag({syntax})")).unwrap();
        let resolved = rules.resolve(&request);
        let item = &resolved.actions[0];
        let Action::Tag(value) = &item.action else {
            panic!("expected tag action");
        };
        let error = resolve_value_bytes(value, item, &request, &state).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{syntax}");
        assert!(error.to_string().contains("8388608-byte limit"), "{syntax}");
    }

    let rules = RuleSet::parse("limit", "example.test mock(<large.txt>)").unwrap();
    let resolved = rules.resolve(&request);
    let error = first_mock(&resolved.actions, &request, &state).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("8388608-byte limit"));

    let _ = fs::remove_dir_all(storage);
}
