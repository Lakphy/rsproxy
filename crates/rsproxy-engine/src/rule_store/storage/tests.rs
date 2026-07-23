use super::*;
use std::fmt::Write as _;

fn temp_rules_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rsproxy-storage-bound-{label}-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ))
}

#[test]
fn bounded_file_reader_checks_one_extra_byte_and_utf8() {
    let rules_dir = temp_rules_dir("reader");
    fs::create_dir_all(&rules_dir).unwrap();
    let path = rules_dir.join("fixture.rules");
    fs::write(&path, b"abc").unwrap();
    assert_eq!(
        read_utf8_file_bounded(&path, 3, "fixture").unwrap(),
        Some("abc".to_string())
    );
    let error = read_utf8_file_bounded(&path, 2, "fixture").unwrap_err();
    assert!(matches!(error, RuleStoreError::Invalid(_)));
    assert!(error.to_string().contains("2-byte limit"));

    fs::write(&path, [0xff]).unwrap();
    let error = read_utf8_file_bounded(&path, 1, "fixture").unwrap_err();
    assert!(matches!(error, RuleStoreError::Io { .. }));
    assert!(error.to_string().contains("invalid utf-8"));
    let _ = fs::remove_dir_all(rules_dir);
}

#[test]
fn aggregate_group_reader_enforces_remaining_snapshot_budget() {
    let rules_dir = temp_rules_dir("aggregate");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::write(group_path(&rules_dir, "later"), "ab").unwrap();
    let mut source_bytes = MAX_RULE_SNAPSHOT_SOURCE_BYTES - 1;
    let error = read_group_text(&rules_dir, "later", &mut source_bytes).unwrap_err();
    assert!(matches!(error, RuleStoreError::Invalid(_)));
    assert_eq!(source_bytes, MAX_RULE_SNAPSHOT_SOURCE_BYTES - 1);
    assert!(error.to_string().contains("1-byte limit"));
    let _ = fs::remove_dir_all(rules_dir);
}

#[test]
fn manifest_group_limit_is_checked_before_group_file_reads() {
    let rules_dir = temp_rules_dir("manifest-groups");
    fs::create_dir_all(&rules_dir).unwrap();
    let mut manifest = String::from("version = 1\n");
    for index in 0..=MAX_RULE_GROUPS_PER_SNAPSHOT {
        writeln!(
            manifest,
            "\n[[groups]]\nname = \"g{index}\"\nenabled = true"
        )
        .unwrap();
    }
    fs::write(rules_dir.join(MANIFEST_FILE), manifest).unwrap();
    let error = load_snapshot(&rules_dir).unwrap_err();
    assert!(matches!(error, RuleStoreError::Invalid(_)));
    assert!(error.to_string().contains("group limit"));
    let _ = fs::remove_dir_all(rules_dir);
}
