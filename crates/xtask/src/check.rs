mod fs_walk;
mod layout;
mod lines;
mod typed_errors;
mod whistle;
mod workflow_contracts;
mod workflows;

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CheckKind {
    Lines,
    Layout,
    TypedErrors,
    Workflows,
    All,
}

impl fmt::Display for CheckKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Lines => "lines",
            Self::Layout => "layout",
            Self::TypedErrors => "typed-errors",
            Self::Workflows => "workflows",
            Self::All => "all",
        };
        formatter.write_str(name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckPass {
    pub kind: CheckKind,
    pub summary: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CheckReport {
    pub checks: Vec<CheckPass>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Violation {
    pub path: PathBuf,
    pub message: String,
}

impl Violation {
    fn new(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckFailures {
    pub kind: CheckKind,
    pub violations: Vec<Violation>,
}

impl fmt::Display for CheckFailures {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "{} check failed:", self.kind)?;
        for violation in &self.violations {
            writeln!(
                formatter,
                "  {}: {}",
                violation.path.display(),
                violation.message
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for CheckFailures {}

#[derive(Debug, Error)]
pub enum CheckError {
    #[error("failed to {action} {path}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to parse check configuration {path}")]
    Config {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("failed to parse Rust source {path}")]
    RustSyntax {
        path: PathBuf,
        #[source]
        source: syn::Error,
    },

    #[error("failed to parse JSON contract {path}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("{0}")]
    Violations(#[from] CheckFailures),
}

pub fn run(workspace_root: &Path, kind: CheckKind) -> Result<CheckReport, CheckError> {
    let requested: &[CheckKind] = match kind {
        CheckKind::All => &[
            CheckKind::Lines,
            CheckKind::Layout,
            CheckKind::TypedErrors,
            CheckKind::Workflows,
        ],
        CheckKind::Lines => &[CheckKind::Lines],
        CheckKind::Layout => &[CheckKind::Layout],
        CheckKind::TypedErrors => &[CheckKind::TypedErrors],
        CheckKind::Workflows => &[CheckKind::Workflows],
    };
    let mut checks = Vec::with_capacity(requested.len());
    for &check in requested {
        let summary = match check {
            CheckKind::Lines => lines::check(workspace_root)?,
            CheckKind::Layout => layout::check(workspace_root)?,
            CheckKind::TypedErrors => typed_errors::check(workspace_root)?,
            CheckKind::Workflows => workflows::check(workspace_root)?,
            CheckKind::All => unreachable!("all expands into individual checks"),
        };
        checks.push(CheckPass {
            kind: check,
            summary,
        });
    }
    Ok(CheckReport { checks })
}

fn fail_if_any(kind: CheckKind, violations: Vec<Violation>) -> Result<(), CheckError> {
    if violations.is_empty() {
        Ok(())
    } else {
        Err(CheckFailures { kind, violations }.into())
    }
}

fn io_error(action: &'static str, path: &Path, source: io::Error) -> CheckError {
    CheckError::Io {
        action,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests;
