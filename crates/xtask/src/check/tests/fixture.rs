use std::fs;
use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::super::workflow_contracts::CONTRACTS;

pub(super) struct Fixture {
    directory: TempDir,
}

impl Fixture {
    pub(super) fn new() -> Self {
        Self {
            directory: tempfile::tempdir().expect("create check fixture"),
        }
    }

    pub(super) fn root(&self) -> &Path {
        self.directory.path()
    }

    pub(super) fn write(&self, relative: &str, contents: &str) {
        write(self.root(), relative, contents);
    }

    pub(super) fn remove(&self, relative: &str) {
        let path = self.root().join(relative);
        if path.is_dir() {
            fs::remove_dir_all(path).expect("remove fixture directory");
        } else {
            fs::remove_file(path).expect("remove fixture file");
        }
    }

    pub(super) fn basic_rust_tree(&self) {
        self.write(
            "crates/example/Cargo.toml",
            "[package]\nname = \"example\"\n",
        );
        self.write("crates/example/src/lib.rs", "pub fn value() -> u8 { 1 }\n");
        self.write("crates/example/tests/public_api.rs", "use example as _;\n");
        self.write("fuzz/fuzz_targets/example.rs", "fn main() {}\n");
    }

    pub(super) fn whistle(&self) {
        let fixture = "crates/rsproxy-rules/tests/fixtures/whistle-2.10.5";
        let license = b"MIT fixture license\n";
        let mut hashes = vec![format!("{}  LICENSE", digest(license))];
        self.write(
            &format!("{fixture}/LICENSE"),
            str::from_utf8(license).unwrap(),
        );
        for index in 0..75 {
            let name = format!("docs/evidence-{index:02}.txt");
            let contents = format!("evidence {index}\n");
            self.write(&format!("{fixture}/{name}"), &contents);
            hashes.push(format!("{}  {name}", digest(contents.as_bytes())));
        }
        self.write(&format!("{fixture}/lib/.keep"), "");
        self.write(&format!("{fixture}/test/.keep"), "");
        // Keep the evidence count at 75: empty marker files are removed after creating dirs.
        self.remove(&format!("{fixture}/lib/.keep"));
        self.remove(&format!("{fixture}/test/.keep"));
        self.write(
            &format!("{fixture}/SNAPSHOT.toml"),
            "schema = \"rsproxy.whistle-fixture/v1\"\n\
             upstream = \"https://github.com/avwo/whistle\"\n\
             version = \"2.10.5\"\n\
             commit = \"0123456789abcdef0123456789abcdef01234567\"\n\
             license = \"MIT\"\n\
             evidence_files = 75\n",
        );
        self.write(
            &format!("{fixture}/SHA256SUMS"),
            &(hashes.join("\n") + "\n"),
        );

        write_json(
            self.root(),
            "benches/e2e/whistle-driver/package.json",
            &json!({ "dependencies": { "whistle": "2.10.5" } }),
        );
        write_json(
            self.root(),
            "benches/e2e/whistle-driver/package-lock.json",
            &json!({
                "packages": { "node_modules/whistle": { "version": "2.10.5" } }
            }),
        );
        self.write("scripts/placeholder.sh", "#!/bin/sh\n");
    }

    pub(super) fn workflows(&self) {
        for contract in CONTRACTS {
            let mut source = String::from(
                "name: Fixture\non:\n  workflow_dispatch:\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v6\n",
            );
            match contract.file {
                "ci.yml" => source.push_str(
                    "      - uses: EmbarkStudios/cargo-deny-action@v2\n      - run: cargo xtask check all\n      - run: cargo xtask check all\n",
                ),
                "performance.yml" => source.push_str(
                    "      - run: cargo xtask targets criterion target/performance/criterion.json\n      - run: cargo xtask targets regression \"$RUNNER_TEMP/criterion-base.json\" target/performance/criterion.json 10\n",
                ),
                "release.yml" => source
                    .push_str("      - run: cargo xtask release \"$version\" --check\n"),
                _ => {}
            }
            for required in contract.required {
                source.push_str("# ");
                source.push_str(required);
                source.push('\n');
            }
            self.write(&format!(".github/workflows/{}", contract.file), &source);
        }
    }
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
    fs::write(path, contents).expect("write fixture");
}

fn write_json(root: &Path, relative: &str, value: &serde_json::Value) {
    write(
        root,
        relative,
        &(serde_json::to_string_pretty(value).expect("serialize fixture JSON") + "\n"),
    );
}

fn digest(contents: &[u8]) -> String {
    format!("{:x}", Sha256::digest(contents))
}
