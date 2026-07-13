use std::fs;
use std::path::Path;

use syn::visit::{self, Visit};
use syn::{Attribute, ItemMod};

use super::fs_walk;
use super::{CheckError, CheckKind, Violation, fail_if_any, io_error, whistle};

pub(super) fn check(root: &Path) -> Result<String, CheckError> {
    let mut violations = rust_layout_violations(root)?;
    violations.extend(missing_integration_directories(root)?);
    violations.extend(whistle::violations(root)?);
    fail_if_any(CheckKind::Layout, violations)?;
    Ok("Rust test layout and pinned Whistle evidence are valid.".to_owned())
}

fn rust_layout_violations(root: &Path) -> Result<Vec<Violation>, CheckError> {
    let files = fs_walk::files(root, &["crates"], &[])?;
    let mut violations = Vec::new();
    for relative in files
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
    {
        let source = fs_walk::read_text(root, &relative)?;
        let syntax = syn::parse_file(&source).map_err(|source| CheckError::RustSyntax {
            path: relative.clone(),
            source,
        })?;
        let mut visitor = LayoutVisitor::default();
        visitor.visit_file(&syntax);
        if visitor.inline_test_module {
            violations.push(Violation::new(
                &relative,
                "inline `mod tests { ... }` is forbidden; use a dedicated test file",
            ));
        }
        if visitor.test_attribute && !is_dedicated_test_path(&relative) {
            violations.push(Violation::new(
                relative,
                "test function is outside `tests.rs` or a `tests/` directory",
            ));
        }
    }
    Ok(violations)
}

fn missing_integration_directories(root: &Path) -> Result<Vec<Violation>, CheckError> {
    let crates = root.join("crates");
    let entries = fs::read_dir(&crates).map_err(|source| io_error("list", &crates, source))?;
    let mut directories = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| io_error("list", &crates, source))
        })
        .collect::<Result<Vec<_>, _>>()?;
    directories.sort();

    let mut violations = Vec::new();
    for directory in directories {
        if directory.join("Cargo.toml").is_file() && !directory.join("tests").is_dir() {
            let relative = directory.strip_prefix(root).unwrap_or(&directory);
            violations.push(Violation::new(
                relative.join("tests"),
                "crate is missing a public integration-test directory",
            ));
        }
    }
    Ok(violations)
}

fn is_dedicated_test_path(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "tests.rs")
        || path
            .parent()
            .is_some_and(|parent| parent.components().any(|part| part.as_os_str() == "tests"))
}

#[derive(Default)]
struct LayoutVisitor {
    inline_test_module: bool,
    test_attribute: bool,
}

impl<'ast> Visit<'ast> for LayoutVisitor {
    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        if node.ident == "tests" && node.content.is_some() {
            self.inline_test_module = true;
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        let path = attribute.path();
        if path.is_ident("test")
            || (path.segments.len() == 2
                && path.segments[0].ident == "tokio"
                && path.segments[1].ident == "test")
        {
            self.test_attribute = true;
        }
        visit::visit_attribute(self, attribute);
    }
}

#[cfg(test)]
pub(super) fn rust_violations_for_test(root: &Path) -> Result<Vec<Violation>, CheckError> {
    let mut violations = rust_layout_violations(root)?;
    violations.extend(missing_integration_directories(root)?);
    Ok(violations)
}

#[cfg(test)]
pub(super) fn dedicated_test_path_for_test(path: &Path) -> bool {
    is_dedicated_test_path(path)
}
