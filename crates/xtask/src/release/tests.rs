use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use semver::Version;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::{ReleaseError, release};

mod edges;

const MEMBERS: [&str; 8] = [
    "rsproxy-rules",
    "rsproxy-trace",
    "rsproxy-net",
    "rsproxy-engine",
    "rsproxy-control",
    "rsproxy-platform",
    "rsproxy-cli",
    "xtask",
];

const NATIVE_PACKAGES: [&str; 8] = [
    "@rsproxy/darwin-arm64",
    "@rsproxy/darwin-x64",
    "@rsproxy/linux-arm64-gnu",
    "@rsproxy/linux-arm64-musl",
    "@rsproxy/linux-x64-gnu",
    "@rsproxy/linux-x64-musl",
    "@rsproxy/win32-arm64-msvc",
    "@rsproxy/win32-x64-msvc",
];

pub(super) struct Fixture {
    directory: TempDir,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let directory = tempfile::tempdir().expect("create fixture");
        let root = directory.path();
        let members = MEMBERS
            .iter()
            .map(|name| format!("    \"crates/{name}\","))
            .collect::<Vec<_>>()
            .join("\n");
        write(
            root,
            "Cargo.toml",
            &format!(
                "[workspace]\nresolver = \"3\"\nmembers = [\n{members}\n]\n\
                 \n[workspace.package]\nversion = \"0.1.0\"\n"
            ),
        );
        for name in MEMBERS {
            write(
                root,
                &format!("crates/{name}/Cargo.toml"),
                &format!("[package]\nname = \"{name}\"\nversion.workspace = true\n"),
            );
        }

        let locked_packages = MEMBERS
            .iter()
            .map(|name| format!("[[package]]\nname = \"{name}\"\nversion = \"0.1.0\"\n"))
            .collect::<Vec<_>>()
            .join("\n");
        write(
            root,
            "Cargo.lock",
            &format!("version = 4\n\n{locked_packages}"),
        );
        write(
            root,
            "fuzz/Cargo.lock",
            "version = 4\n\n[[package]]\nname = \"rsproxy-rules\"\nversion = \"0.1.0\"\n",
        );
        write(
            root,
            "package.json",
            "{\n  \"name\": \"fixture\",\n  \"version\": \"0.1.0\"\n}\n",
        );

        let targets = NATIVE_PACKAGES
            .iter()
            .enumerate()
            .map(|(index, package)| {
                json!({
                    "rustTarget": format!("fixture-target-{index}"),
                    "package": package,
                    "platform": "fixture",
                    "arch": "fixture",
                    "executable": "rsproxy"
                })
            })
            .collect::<Vec<_>>();
        write_json(
            root,
            "packages/npm/targets.json",
            &json!({ "schemaVersion": 1, "targets": targets }),
        );
        write_json(
            root,
            "packages/npm/runtime/package.json",
            &json!({
                "name": "@rsproxy/runtime",
                "version": "0.1.0",
                "optionalDependencies": NATIVE_PACKAGES
                    .iter()
                    .map(|name| (*name, "0.1.0"))
                    .collect::<BTreeMap<_, _>>()
            }),
        );
        write_json(
            root,
            "packages/npm/cli/package.json",
            &json!({
                "name": "@rsproxy/cli",
                "version": "0.1.0",
                "dependencies": {
                    "@rsproxy/runtime": "0.1.0",
                    "external-package": "1.0.0"
                }
            }),
        );
        Self { directory }
    }

    pub(super) fn root(&self) -> &Path {
        self.directory.path()
    }

    fn snapshot(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fixture_files(self.root())
            .into_iter()
            .map(|path| {
                let contents = fs::read(self.root().join(&path)).expect("read snapshot");
                (path, contents)
            })
            .collect()
    }
}

#[test]
fn release_updates_every_derived_version_and_is_idempotent() {
    let fixture = Fixture::new();
    let version = Version::parse("0.2.0-beta.1").expect("valid version");

    let outcome = release(fixture.root(), &version, false).expect("apply release");
    assert_eq!(outcome.changed_files, 6);
    assert_eq!(cargo_workspace_version(fixture.root()), "0.2.0-beta.1");
    assert_lock_versions(fixture.root(), "Cargo.lock", &version);
    assert_lock_versions(fixture.root(), "fuzz/Cargo.lock", &version);

    let root = read_json(fixture.root(), "package.json");
    assert_eq!(root["version"], version.to_string());
    let runtime = read_json(fixture.root(), "packages/npm/runtime/package.json");
    let optional = runtime["optionalDependencies"]
        .as_object()
        .expect("optional dependencies");
    assert_eq!(optional.len(), 8);
    assert_eq!(
        optional.keys().cloned().collect::<Vec<_>>(),
        NATIVE_PACKAGES.map(ToOwned::to_owned)
    );
    let version_string = version.to_string();
    assert!(optional.values().all(|value| value == &version_string));
    let cli = read_json(fixture.root(), "packages/npm/cli/package.json");
    assert_eq!(cli["version"], version.to_string());
    assert_eq!(cli["dependencies"]["@rsproxy/runtime"], version.to_string());
    assert_eq!(cli["dependencies"]["external-package"], "1.0.0");
    assert!(!fixture.root().join("packages/npm/darwin-arm64").exists());

    let snapshot = fixture.snapshot();
    assert_eq!(
        release(fixture.root(), &version, false)
            .expect("idempotent release")
            .changed_files,
        0
    );
    assert_eq!(fixture.snapshot(), snapshot);
    release(fixture.root(), &version, true).expect("consistent check");
    assert_eq!(fixture.snapshot(), snapshot);
}

#[test]
fn check_reports_all_inconsistent_files_without_writing() {
    let fixture = Fixture::new();
    let before = fixture.snapshot();
    let version = Version::parse("0.2.0").expect("valid version");

    let error = release(fixture.root(), &version, true).expect_err("must be inconsistent");
    let ReleaseError::Inconsistent { files, .. } = error else {
        panic!("unexpected error: {error}");
    };
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "fuzz/Cargo.lock",
        "package.json",
        "packages/npm/runtime/package.json",
        "packages/npm/cli/package.json",
    ] {
        assert!(files.contains(path), "missing {path} from {files}");
    }
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn invalid_inventory_fails_preflight_without_writing() {
    let fixture = Fixture::new();
    let targets_path = "packages/npm/targets.json";
    let mut targets = read_json(fixture.root(), targets_path);
    targets["schemaVersion"] = json!(2);
    write_json(fixture.root(), targets_path, &targets);
    let before = fixture.snapshot();

    let error = release(
        fixture.root(),
        &Version::parse("0.2.0").expect("valid version"),
        false,
    )
    .expect_err("invalid schema must fail");
    assert!(error.to_string().contains("schemaVersion must be 1"));
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn unexpected_internal_launcher_dependency_is_not_deleted() {
    let fixture = Fixture::new();
    let path = "packages/npm/cli/package.json";
    let mut manifest = read_json(fixture.root(), path);
    manifest["dependencies"]["@rsproxy/unexpected"] = json!("0.1.0");
    write_json(fixture.root(), path, &manifest);
    let before = fixture.snapshot();

    let error = release(
        fixture.root(),
        &Version::parse("0.2.0").expect("valid version"),
        false,
    )
    .expect_err("unexpected dependency must fail");
    assert!(error.to_string().contains("unexpected internal dependency"));
    assert_eq!(fixture.snapshot(), before);
}

fn fixture_files(root: &Path) -> Vec<PathBuf> {
    let mut files = vec![
        PathBuf::from("Cargo.toml"),
        PathBuf::from("Cargo.lock"),
        PathBuf::from("fuzz/Cargo.lock"),
        PathBuf::from("package.json"),
        PathBuf::from("packages/npm/targets.json"),
        PathBuf::from("packages/npm/runtime/package.json"),
        PathBuf::from("packages/npm/cli/package.json"),
    ];
    files.extend(
        MEMBERS
            .iter()
            .map(|name| PathBuf::from(format!("crates/{name}/Cargo.toml"))),
    );
    assert!(files.iter().all(|path| root.join(path).is_file()));
    files
}

pub(super) fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
    fs::write(path, contents).expect("write fixture");
}

pub(super) fn write_json(root: &Path, relative: &str, value: &Value) {
    write(
        root,
        relative,
        &(serde_json::to_string_pretty(value).expect("serialize fixture") + "\n"),
    );
}

pub(super) fn read_json(root: &Path, relative: &str) -> Value {
    serde_json::from_slice(&fs::read(root.join(relative)).expect("read JSON fixture"))
        .expect("parse JSON fixture")
}

fn cargo_workspace_version(root: &Path) -> String {
    let document = fs::read_to_string(root.join("Cargo.toml"))
        .expect("read Cargo manifest")
        .parse::<toml_edit::DocumentMut>()
        .expect("parse Cargo manifest");
    document["workspace"]["package"]["version"]
        .as_str()
        .expect("workspace version")
        .to_owned()
}

fn assert_lock_versions(root: &Path, relative: &str, version: &Version) {
    let document = fs::read_to_string(root.join(relative))
        .expect("read lockfile")
        .parse::<toml_edit::DocumentMut>()
        .expect("parse lockfile");
    let expected = version.to_string();
    for package in document["package"]
        .as_array_of_tables()
        .expect("package inventory")
    {
        let name = package["name"].as_str().expect("package name");
        if package.get("source").is_none() && MEMBERS.contains(&name) {
            assert_eq!(package["version"].as_str(), Some(expected.as_str()));
        }
    }
}
