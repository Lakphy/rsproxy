use crate::{CliResult, LoggingError};
use std::env;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::util::SubscriberInitExt;

const DEFAULT_FILTER: &str = "rsproxy=info,rsproxy_cli=info,rsproxy_control=info,rsproxy_engine=info,rsproxy_net=info,rsproxy_platform=info,rsproxy_rules=info,rsproxy_trace=info";
const INTERNAL_TARGETS: &[&str] = &[
    "rsproxy_control",
    "rsproxy_engine",
    "rsproxy_net",
    "rsproxy_platform",
    "rsproxy_rules",
    "rsproxy_trace",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LogFormat {
    Text,
    Json,
}

#[derive(Debug, PartialEq, Eq)]
struct LogSettings {
    filter: String,
    format: LogFormat,
}

pub(crate) fn init() -> CliResult<()> {
    let settings = LogSettings::from_environment()?;
    let filter =
        EnvFilter::try_new(&settings.filter).map_err(|source| LoggingError::InvalidFilter {
            filter: settings.filter.clone(),
            source,
        })?;
    match settings.format {
        LogFormat::Text => {
            let subscriber = tracing_subscriber::registry().with(filter).with(
                fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_ansi(false)
                    .with_target(true)
                    .with_thread_names(true),
            );
            let _ = subscriber.try_init();
        }
        LogFormat::Json => {
            let subscriber = tracing_subscriber::registry().with(filter).with(
                fmt::layer()
                    .json()
                    .with_writer(std::io::stderr)
                    .with_ansi(false)
                    .with_target(true)
                    .with_thread_names(true)
                    .with_current_span(false)
                    .with_span_list(false),
            );
            let _ = subscriber.try_init();
        }
    }
    Ok(())
}

impl LogSettings {
    fn from_environment() -> Result<Self, LoggingError> {
        Self::from_values(
            env::var("RSPROXY_LOG").ok().as_deref(),
            env::var("RUST_LOG").ok().as_deref(),
            env::var("RSPROXY_LOG_FORMAT").ok().as_deref(),
        )
    }

    fn from_values(
        rsproxy_filter: Option<&str>,
        rust_filter: Option<&str>,
        format: Option<&str>,
    ) -> Result<Self, LoggingError> {
        let filter = rsproxy_filter
            .filter(|value| !value.trim().is_empty())
            .or_else(|| rust_filter.filter(|value| !value.trim().is_empty()))
            .unwrap_or(DEFAULT_FILTER)
            .to_string();
        let filter = expand_cli_target(&filter);
        let format = match format
            .unwrap_or("text")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "text" | "compact" => LogFormat::Text,
            "json" => LogFormat::Json,
            value => {
                return Err(LoggingError::InvalidFormat {
                    value: value.to_string(),
                });
            }
        };
        Ok(Self { filter, format })
    }
}

fn expand_cli_target(filter: &str) -> String {
    let directives = filter
        .split(',')
        .map(str::trim)
        .filter(|directive| !directive.is_empty())
        .collect::<Vec<_>>();
    let suffix = directives.iter().find_map(|directive| {
        directive
            .strip_prefix("rsproxy_cli=")
            .map(|level| format!("={level}"))
            .or_else(|| (*directive == "rsproxy_cli").then(String::new))
    });
    let Some(suffix) = suffix else {
        return filter.to_string();
    };
    let mut expanded = filter.to_string();
    for target in INTERNAL_TARGETS {
        let already_configured = directives
            .iter()
            .any(|directive| *directive == *target || directive.starts_with(&format!("{target}=")));
        if !already_configured {
            expanded.push(',');
            expanded.push_str(target);
            expanded.push_str(&suffix);
        }
    }
    expanded
}

#[cfg(test)]
#[path = "logging/tests.rs"]
mod tests;
