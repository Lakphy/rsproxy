use crate::fuzz_harness;
use rsproxy_rules::RuleSet;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn parse_resolve_fuzz_seeds_are_replayed_by_cargo_test() {
    let mut paths = fs::read_dir(seed_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    paths.sort();
    assert!(paths.len() >= 7, "expected a representative fuzz seed set");

    for path in paths {
        let data = fs::read(&path).unwrap();
        assert!(!data.is_empty(), "{} is empty", path.display());
        fuzz_harness::exercise(&data);

        let name = path.file_name().unwrap().to_string_lossy();
        let text = String::from_utf8(data).unwrap();
        let parsed = RuleSet::parse("seed", &text);
        if name.starts_with("valid-") {
            parsed.unwrap_or_else(|errors| panic!("{}: {errors:?}", path.display()));
        } else if name.starts_with("invalid-") {
            assert!(parsed.is_err(), "{} should be invalid", path.display());
        } else {
            panic!("fuzz seed name must start with valid- or invalid-: {name}");
        }
    }
}

fn seed_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/parse_resolve")
}
