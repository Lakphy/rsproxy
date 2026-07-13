use std::fs;
use std::path::{Path, PathBuf};

use semver::Version;
use serde_json::{Value, json};

use super::super::{FileChange, ReleaseError, release, transaction};
use super::{Fixture, read_json, write, write_json};

#[test]
fn cargo_release_preflight_rejects_invalid_workspace_shapes() {
    assert_invalid(
        |fixture| write(fixture.root(), "Cargo.toml", "[package]\nname = \"root\"\n"),
        "missing `[workspace]`",
    );
    assert_invalid(
        |fixture| write(fixture.root(), "Cargo.toml", "[workspace]\nmembers = []\n"),
        "missing `[workspace.package]`",
    );
    assert_invalid(
        |fixture| {
            write(
                fixture.root(),
                "Cargo.toml",
                "[workspace]\nmembers = []\n[workspace.package]\nversion = 1\n",
            )
        },
        "version must be a string",
    );
    assert_invalid(
        |fixture| {
            write(
                fixture.root(),
                "Cargo.toml",
                "[workspace]\nmembers = []\n[workspace.package]\nversion = \"invalid\"\n",
            )
        },
        "workspace version `invalid`",
    );
    assert_invalid(
        |fixture| {
            write(
                fixture.root(),
                "Cargo.toml",
                "[workspace]\nmembers = \"crates/example\"\n[workspace.package]\nversion = \"0.1.0\"\n",
            )
        },
        "members must be an array",
    );
    assert_invalid(
        |fixture| {
            write(
                fixture.root(),
                "Cargo.toml",
                "[workspace]\nmembers = [1]\n[workspace.package]\nversion = \"0.1.0\"\n",
            )
        },
        "member paths must be strings",
    );
    assert_invalid(
        |fixture| {
            replace(
                fixture.root(),
                "Cargo.toml",
                "\"crates/xtask\"",
                "\"crates/*\"",
            )
        },
        "member globs are not supported",
    );
    assert_invalid(
        |fixture| {
            write(
                fixture.root(),
                "crates/xtask/Cargo.toml",
                "name = \"xtask\"\n",
            )
        },
        "missing `[package]`",
    );
    assert_invalid(
        |fixture| {
            write(
                fixture.root(),
                "crates/xtask/Cargo.toml",
                "[package]\nname = 1\nversion.workspace = true\n",
            )
        },
        "package name must be a string",
    );
    assert_invalid(
        |fixture| {
            write(
                fixture.root(),
                "crates/xtask/Cargo.toml",
                "[package]\nname = \"xtask\"\nversion = \"0.1.0\"\n",
            )
        },
        "version.workspace = true",
    );
    assert_invalid(
        |fixture| {
            replace(
                fixture.root(),
                "crates/xtask/Cargo.toml",
                "xtask",
                "rsproxy-cli",
            )
        },
        "duplicate workspace package name",
    );
}

#[test]
fn release_preflight_rejects_invalid_lockfiles_and_json_manifests() {
    assert_invalid(
        |fixture| write(fixture.root(), "Cargo.lock", "version = 4\n"),
        "no package inventory",
    );
    assert_invalid(
        |fixture| {
            write(
                fixture.root(),
                "Cargo.lock",
                "version = 4\n\n[[package]]\nversion = \"0.1.0\"\n",
            )
        },
        "missing a string name",
    );
    assert_invalid(
        |fixture| {
            replace(
                fixture.root(),
                "Cargo.lock",
                "version = \"0.1.0\"",
                "checksum = \"x\"",
            )
        },
        "has no version",
    );
    assert_invalid(
        |fixture| {
            replace(
                fixture.root(),
                "Cargo.lock",
                "name = \"xtask\"",
                "name = \"other\"",
            )
        },
        "packages missing from lockfile",
    );
    assert_invalid(
        |fixture| write(fixture.root(), "packages/npm/targets.json", "{not-json"),
        "failed to parse JSON",
    );
    assert_inventory_invalid(
        |targets| {
            targets["targets"].as_array_mut().unwrap().pop();
        },
        "exactly 8 entries",
    );
    assert_inventory_invalid(
        |targets| {
            let duplicate = targets["targets"][0]["rustTarget"].clone();
            targets["targets"][1]["rustTarget"] = duplicate;
        },
        "duplicate or empty Rust target",
    );
    assert_inventory_invalid(
        |targets| {
            targets["targets"][0]["package"] = json!("external");
        },
        "duplicate or invalid npm package",
    );
    assert_inventory_invalid(
        |targets| {
            targets["targets"][0]["package"] = json!("@rsproxy/cli");
        },
        "launcher package cannot be a native target",
    );
    assert_invalid(
        |fixture| write(fixture.root(), "package.json", "[]\n"),
        "top-level JSON value must be an object",
    );
    assert_manifest_invalid(
        "package.json",
        |manifest| {
            manifest["version"] = json!(1);
        },
        "`version` must be a string",
    );
    assert_manifest_invalid(
        "packages/npm/runtime/package.json",
        |manifest| {
            manifest["name"] = json!("@rsproxy/wrong");
        },
        "expected package name",
    );
    assert_manifest_invalid(
        "packages/npm/cli/package.json",
        |manifest| {
            manifest["dependencies"] = json!([]);
        },
        "`dependencies` must be an object",
    );
    assert_manifest_invalid(
        "packages/npm/bun/package.json",
        |manifest| {
            manifest.as_object_mut().unwrap().remove("dependencies");
        },
        "missing required `dependencies` object",
    );
}

#[test]
fn release_transaction_cleans_staged_files_after_stage_and_verify_failures() {
    let directory = tempfile::tempdir().expect("transaction fixture");
    let first_path = directory.path().join("first.txt");
    fs::write(&first_path, b"old").unwrap();
    let missing_parent = directory.path().join("missing").join("second.txt");
    let error = transaction::apply(&[
        change(first_path.clone(), b"old", b"new"),
        change(missing_parent.clone(), b"old", b"new"),
    ])
    .expect_err("second file cannot be staged");
    assert!(matches!(error, ReleaseError::Stage { path, .. } if path == missing_parent));
    assert_eq!(fs::read(&first_path).unwrap(), b"old");
    assert_no_transaction_files(directory.path());

    let absent = directory.path().join("absent.txt");
    let error = transaction::apply(&[change(absent.clone(), b"old", b"new")])
        .expect_err("absent original cannot be verified");
    assert!(matches!(error, ReleaseError::Verify { path, .. } if path == absent));
    assert_no_transaction_files(directory.path());

    let changed = directory.path().join("changed.txt");
    fs::write(&changed, b"actual").unwrap();
    let error = transaction::apply(&[change(changed.clone(), b"expected", b"new")])
        .expect_err("changed original must be detected");
    assert!(matches!(error, ReleaseError::ConcurrentModification { path } if path == changed));
    assert_eq!(fs::read(&changed).unwrap(), b"actual");
    assert_no_transaction_files(directory.path());
}

fn assert_invalid(mutate: impl FnOnce(&Fixture), expected: &str) {
    let fixture = Fixture::new();
    mutate(&fixture);
    let error = release(fixture.root(), &Version::parse("0.2.0").unwrap(), false)
        .expect_err("invalid release input must fail");
    assert!(error.to_string().contains(expected), "{error}");
}

fn assert_inventory_invalid(mutate: impl FnOnce(&mut Value), expected: &str) {
    assert_manifest_invalid("packages/npm/targets.json", mutate, expected);
}

fn assert_manifest_invalid(relative: &str, mutate: impl FnOnce(&mut Value), expected: &str) {
    assert_invalid(
        |fixture| {
            let mut document = read_json(fixture.root(), relative);
            mutate(&mut document);
            write_json(fixture.root(), relative, &document);
        },
        expected,
    );
}

fn replace(root: &Path, relative: &str, from: &str, to: &str) {
    let source = fs::read_to_string(root.join(relative)).unwrap();
    assert!(source.contains(from));
    write(root, relative, &source.replacen(from, to, 1));
}

fn change(path: PathBuf, original: &[u8], updated: &[u8]) -> FileChange {
    FileChange {
        path,
        original: original.to_vec(),
        updated: updated.to_vec(),
    }
}

fn assert_no_transaction_files(directory: &Path) {
    let entries = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(
        entries
            .iter()
            .all(|name| !name.contains(".rsproxy-release-"))
    );
}
