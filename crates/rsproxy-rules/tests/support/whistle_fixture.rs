use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const EXPECTED_VERSION: &str = "2.10.5";
const EXPECTED_COMMIT: &str = "0b4c4bdb78ff5c53ffcb5a823ca9b53d7e6269c4";

#[derive(Deserialize)]
struct Snapshot {
    schema: String,
    upstream: String,
    version: String,
    commit: String,
    license: String,
    evidence_files: usize,
}

pub fn assert_pinned() {
    let source = fs::read_to_string(root().join("SNAPSHOT.toml")).unwrap();
    let snapshot: Snapshot = toml::from_str(&source).unwrap();
    assert_eq!(snapshot.schema, "rsproxy.whistle-fixture/v1");
    assert_eq!(snapshot.upstream, "https://github.com/avwo/whistle");
    assert_eq!(snapshot.version, EXPECTED_VERSION);
    assert_eq!(snapshot.commit, EXPECTED_COMMIT);
    assert_eq!(snapshot.license, "MIT");
    assert_eq!(snapshot.evidence_files, 75);
}

pub fn read(relative: &str) -> String {
    let path = Path::new(relative);
    assert!(
        !path.is_absolute()
            && !relative.is_empty()
            && !relative
                .split('/')
                .any(|part| part.is_empty() || part == ".."),
        "unsafe Whistle fixture path {relative}"
    );
    fs::read_to_string(root().join(path))
        .unwrap_or_else(|error| panic!("read Whistle fixture {relative}: {error}"))
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/whistle-2.10.5")
}
