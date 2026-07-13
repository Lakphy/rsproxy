use std::env;
use std::ffi::OsString;
use std::fmt::{self, Display, Write as _};
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use serde::de::DeserializeOwned;
use thiserror::Error;

mod coverage;
mod criterion;
mod e2e;
mod model;
mod soak;

pub const DEFAULT_REGRESSION_TOLERANCE_PERCENT: f64 = 10.0;

#[derive(Debug, Args)]
pub struct TargetsArgs {
    #[command(subcommand)]
    pub command: TargetCommand,
}

#[derive(Debug, Subcommand)]
pub enum TargetCommand {
    /// Check line-coverage targets.
    Coverage { report: PathBuf },
    /// Check the cached TLS-handshake Criterion target.
    Criterion { report: PathBuf },
    /// Check end-to-end throughput, latency, and memory targets.
    E2e { report: PathBuf },
    /// Check long-running stability targets.
    Soak { report: PathBuf },
    /// Compare current Criterion means with a baseline.
    Regression {
        baseline: PathBuf,
        current: PathBuf,
        #[arg(default_value_t = DEFAULT_REGRESSION_TOLERANCE_PERCENT)]
        tolerance_percent: f64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetCheck {
    pub label: String,
    pub observed: String,
    pub target: String,
}

impl TargetCheck {
    pub(crate) fn new(
        label: impl Into<String>,
        observed: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            observed: observed.into(),
            target: target.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetOutcome {
    checks: Vec<TargetCheck>,
    summary: Option<String>,
}

impl TargetOutcome {
    pub fn checks(&self) -> &[TargetCheck] {
        &self.checks
    }

    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    pub(crate) fn new(checks: Vec<TargetCheck>, summary: Option<String>) -> Self {
        Self { checks, summary }
    }
}

impl Display for TargetOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, check) in self.checks.iter().enumerate() {
            if index > 0 {
                formatter.write_char('\n')?;
            }
            write!(
                formatter,
                "PASS {} observed={} target={}",
                check.label, check.observed, check.target
            )?;
        }
        if let Some(summary) = &self.summary {
            if !self.checks.is_empty() {
                formatter.write_char('\n')?;
            }
            formatter.write_str(summary)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedChecks(Vec<TargetCheck>);

impl FailedChecks {
    pub fn checks(&self) -> &[TargetCheck] {
        &self.0
    }
}

impl Display for FailedChecks {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, check) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str("; ")?;
            }
            write!(
                formatter,
                "{} observed={} target={}",
                check.label, check.observed, check.target
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegressionFailure {
    pub metric: String,
    pub baseline_ns: f64,
    pub current_ns: f64,
    pub change_percent: f64,
    pub tolerance_percent: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegressionFailures(Vec<RegressionFailure>);

impl RegressionFailures {
    pub fn failures(&self) -> &[RegressionFailure] {
        &self.0
    }
}

impl Display for RegressionFailures {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, failure) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str("; ")?;
            }
            write!(
                formatter,
                "{} baseline={}ns current={}ns change={:.2}% tolerance={}%",
                failure.metric,
                failure.baseline_ns,
                failure.current_ns,
                failure.change_percent,
                failure.tolerance_percent
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TargetError {
    #[error("failed to read target report {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse target report {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid target report {path}: field `{field}` {message}")]
    InvalidReport {
        path: PathBuf,
        field: String,
        message: String,
    },
    #[error("invalid environment variable {name}={value:?}: expected {expected}")]
    InvalidEnvironment {
        name: &'static str,
        value: String,
        expected: &'static str,
    },
    #[error("invalid argument `{field}`={value}: expected {expected}")]
    InvalidArgument {
        field: &'static str,
        value: f64,
        expected: &'static str,
    },
    #[error("target checks failed for {path}: {failures}")]
    ChecksFailed {
        path: PathBuf,
        failures: FailedChecks,
    },
    #[error("current Criterion report {path} is missing baseline metrics: {metrics}")]
    MissingMetrics { path: PathBuf, metrics: String },
    #[error("Criterion regressions in {path} exceeded tolerance: {failures}")]
    Regressions {
        path: PathBuf,
        failures: RegressionFailures,
    },
}

pub fn run(args: &TargetsArgs) -> Result<TargetOutcome, TargetError> {
    run_with_environment(args, &|name| env::var_os(name))
}

fn run_with_environment(
    args: &TargetsArgs,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<TargetOutcome, TargetError> {
    match &args.command {
        TargetCommand::Coverage { report } => coverage::check(report, lookup),
        TargetCommand::Criterion { report } => criterion::check_target(report, lookup),
        TargetCommand::E2e { report } => e2e::check(report, lookup),
        TargetCommand::Soak { report } => soak::check(report, lookup),
        TargetCommand::Regression {
            baseline,
            current,
            tolerance_percent,
        } => criterion::check_regression(
            baseline,
            current,
            finite_argument("tolerance_percent", *tolerance_percent)?,
        ),
    }
}

fn read_report<T: DeserializeOwned>(path: &Path) -> Result<T, TargetError> {
    let source = fs::read_to_string(path).map_err(|source| TargetError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&source).map_err(|source| TargetError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn invalid_report(
    path: &Path,
    field: impl Into<String>,
    message: impl Into<String>,
) -> TargetError {
    TargetError::InvalidReport {
        path: path.to_path_buf(),
        field: field.into(),
        message: message.into(),
    }
}

fn failed_checks(path: &Path, failures: Vec<TargetCheck>) -> TargetError {
    TargetError::ChecksFailed {
        path: path.to_path_buf(),
        failures: FailedChecks(failures),
    }
}

fn environment_text(
    name: &'static str,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Option<String>, TargetError> {
    let Some(value) = lookup(name) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    value
        .into_string()
        .map(Some)
        .map_err(|value| TargetError::InvalidEnvironment {
            name,
            value: value.to_string_lossy().into_owned(),
            expected: "Unicode text",
        })
}

fn number_environment(
    name: &'static str,
    default: f64,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<f64, TargetError> {
    let Some(value) = environment_text(name, lookup)? else {
        return Ok(default);
    };
    value
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
        .ok_or(TargetError::InvalidEnvironment {
            name,
            value,
            expected: "a finite JSON number",
        })
}

fn integer_environment(
    name: &'static str,
    default: u64,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<u64, TargetError> {
    let Some(value) = environment_text(name, lookup)? else {
        return Ok(default);
    };
    non_negative_integer(name, value)
}

fn non_negative_integer(name: &'static str, value: String) -> Result<u64, TargetError> {
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(TargetError::InvalidEnvironment {
            name,
            value,
            expected: "a non-negative integer",
        });
    }
    value.parse().map_err(|_| TargetError::InvalidEnvironment {
        name,
        value,
        expected: "a non-negative integer within the u64 range",
    })
}

fn finite_argument(field: &'static str, value: f64) -> Result<f64, TargetError> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(TargetError::InvalidArgument {
            field,
            value,
            expected: "a finite non-negative number",
        })
    }
}

#[cfg(test)]
#[path = "targets/tests/mod.rs"]
mod tests;
