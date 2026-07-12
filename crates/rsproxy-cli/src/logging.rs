use std::env;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::util::SubscriberInitExt;

const DEFAULT_FILTER: &str = "rsproxy=info";

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

pub(crate) fn init() -> Result<(), String> {
    let settings = LogSettings::from_environment()?;
    let filter = EnvFilter::try_new(&settings.filter)
        .map_err(|error| format!("invalid log filter `{}`: {error}", settings.filter))?;
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
    fn from_environment() -> Result<Self, String> {
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
    ) -> Result<Self, String> {
        let filter = rsproxy_filter
            .filter(|value| !value.trim().is_empty())
            .or_else(|| rust_filter.filter(|value| !value.trim().is_empty()))
            .unwrap_or(DEFAULT_FILTER)
            .to_string();
        let format = match format
            .unwrap_or("text")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "text" | "compact" => LogFormat::Text,
            "json" => LogFormat::Json,
            value => {
                return Err(format!(
                    "invalid RSPROXY_LOG_FORMAT `{value}`; expected text or json"
                ));
            }
        };
        Ok(Self { filter, format })
    }
}

#[cfg(test)]
#[path = "logging/tests.rs"]
mod tests;
