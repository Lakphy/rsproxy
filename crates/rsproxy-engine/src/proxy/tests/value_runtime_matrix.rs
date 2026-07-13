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
    std::fs::create_dir_all(storage.join("values")).unwrap();
    std::fs::create_dir_all(storage.join("files")).unwrap();
    std::fs::write(
        storage.join("values/shared"),
        b"reference-${host}-$2-${kind}",
    )
    .unwrap();
    std::fs::write(storage.join("files/value.txt"), b"file-${host}-$2-${kind}").unwrap();
    let mut state = test_state();
    state.config.storage = storage.clone();
    let request = meta("http://example.test/items/42");
    let sources = [
        RuntimeSource {
            syntax: "plain",
            expected: "plain",
        },
        RuntimeSource {
            syntax: r#""quoted value""#,
            expected: "quoted value",
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
            let rules =
                rsproxy_rules::RuleSet::parse("runtime-matrix", &rule).unwrap_or_else(|errors| {
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
    let _ = std::fs::remove_dir_all(storage);
}

#[test]
fn every_value_slot_rejects_an_invalid_reference_key() {
    for slot in VALUE_SLOTS {
        let action = slot.action.replace("{value}", "@../escape");
        let rule = format!("example.test {action}");
        let errors = match rsproxy_rules::RuleSet::parse("runtime-matrix", &rule) {
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
