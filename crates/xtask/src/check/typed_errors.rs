use std::path::Path;

use syn::visit::{self, Visit};
use syn::{GenericArgument, PathArguments, Type, TypePath};

use super::fs_walk;
use super::{CheckError, CheckKind, Violation, fail_if_any};

pub(super) fn check(root: &Path) -> Result<String, CheckError> {
    let violations = violations(root)?;
    fail_if_any(CheckKind::TypedErrors, violations)?;
    Ok("Rust error channels use typed errors.".to_owned())
}

fn violations(root: &Path) -> Result<Vec<Violation>, CheckError> {
    let excluded = [Path::new("fuzz/target").to_path_buf()];
    let files = fs_walk::files(root, &["crates", "fuzz"], &excluded)?;
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
        let mut visitor = StringErrorVisitor::default();
        visitor.visit_file(&syntax);
        if visitor.owned_string_results > 0 {
            violations.push(Violation::new(
                &relative,
                format!(
                    "contains {} `Result<_, String>` error channel(s)",
                    visitor.owned_string_results
                ),
            ));
        }
        if visitor.static_str_results > 0 {
            violations.push(Violation::new(
                relative,
                format!(
                    "contains {} `Result<_, &'static str>` error channel(s)",
                    visitor.static_str_results
                ),
            ));
        }
    }
    Ok(violations)
}

#[derive(Default)]
struct StringErrorVisitor {
    owned_string_results: usize,
    static_str_results: usize,
}

impl<'ast> Visit<'ast> for StringErrorVisitor {
    fn visit_type_path(&mut self, node: &'ast TypePath) {
        if let Some(error) = result_error_type(node) {
            if is_string(error) {
                self.owned_string_results += 1;
            } else if is_static_str(error) {
                self.static_str_results += 1;
            }
        }
        visit::visit_type_path(self, node);
    }
}

fn result_error_type(path: &TypePath) -> Option<&Type> {
    let result = path.path.segments.last()?;
    if result.ident != "Result" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &result.arguments else {
        return None;
    };
    let mut types = arguments.args.iter().filter_map(|argument| match argument {
        GenericArgument::Type(value) => Some(value),
        _ => None,
    });
    types.next()?;
    let error = types.next()?;
    types.next().is_none().then_some(error)
}

fn is_string(value: &Type) -> bool {
    match ungroup(value) {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "String"),
        _ => false,
    }
}

fn is_static_str(value: &Type) -> bool {
    let Type::Reference(reference) = ungroup(value) else {
        return false;
    };
    let lifetime_is_static = reference
        .lifetime
        .as_ref()
        .is_some_and(|lifetime| lifetime.ident == "static");
    let primitive_is_str = match ungroup(&reference.elem) {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "str"),
        _ => false,
    };
    lifetime_is_static && primitive_is_str
}

fn ungroup(mut value: &Type) -> &Type {
    loop {
        value = match value {
            Type::Group(group) => &group.elem,
            Type::Paren(parenthesized) => &parenthesized.elem,
            _ => return value,
        };
    }
}

#[cfg(test)]
pub(super) fn violations_for_test(root: &Path) -> Result<Vec<Violation>, CheckError> {
    violations(root)
}
