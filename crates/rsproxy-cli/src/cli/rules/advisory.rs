use rsproxy_rules::{Action, RequestMeta, RuleSet, UrlParts};
use serde_json::{Value as JsonValue, json};
use std::path::Path;

pub(super) const HTTPS_MITM_UNAVAILABLE: &str = "https-mitm-unavailable";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EnvironmentAdvisory {
    pub(super) kind: &'static str,
    pub(super) message: String,
    pub(super) hint: &'static str,
    pub(super) group: Option<String>,
    pub(super) line: Option<usize>,
}

impl EnvironmentAdvisory {
    pub(super) fn to_json(&self) -> JsonValue {
        json!({
            "kind": self.kind,
            "message": self.message,
            "hint": self.hint,
            "group": self.group,
            "line": self.line,
        })
    }
}

pub(super) fn request_advisories(
    rules: &RuleSet,
    request: &RequestMeta,
    storage: &Path,
    no_mitm: bool,
) -> Vec<EnvironmentAdvisory> {
    let Ok(url) = UrlParts::parse(&request.url) else {
        return Vec::new();
    };
    if !matches!(url.scheme.as_str(), "https" | "wss") {
        return Vec::new();
    }
    let resolved = rules.resolve(request);
    let Some(action) = resolved
        .actions
        .iter()
        .find(|item| matches!(item.action, Action::MapRemote(_)))
    else {
        return Vec::new();
    };
    unavailable_advisory(storage, no_mitm).map_or_else(Vec::new, |mut advisory| {
        advisory.group = Some(action.rule.group.to_string());
        advisory.line = Some(action.rule.line);
        vec![advisory]
    })
}

pub(super) fn lint_advisories(
    rules: &RuleSet,
    storage: &Path,
    no_mitm: bool,
) -> Vec<EnvironmentAdvisory> {
    let Some(rule) = rules.rules().iter().find(|rule| {
        !rule.disabled
            && rule
                .actions
                .iter()
                .any(|action| matches!(action, Action::MapRemote(_)))
    }) else {
        return Vec::new();
    };
    unavailable_advisory(storage, no_mitm).map_or_else(Vec::new, |mut advisory| {
        advisory.group = Some(rule.group.to_string());
        advisory.line = Some(rule.line);
        vec![advisory]
    })
}

pub(super) fn print_advisories(advisories: &[EnvironmentAdvisory]) {
    for advisory in advisories {
        let source = match (&advisory.group, advisory.line) {
            (Some(group), Some(line)) => format!(" {group}:{line}"),
            _ => String::new(),
        };
        println!(
            "warning[{}]{}: {}\nhint: {}",
            advisory.kind, source, advisory.message, advisory.hint
        );
    }
}

fn unavailable_advisory(storage: &Path, no_mitm: bool) -> Option<EnvironmentAdvisory> {
    let hint =
        "run `rsproxy ca init && rsproxy ca install`, ensure MITM is enabled, then restart rsproxy";
    let message = if no_mitm {
        "map.remote cannot inspect HTTPS/WSS requests because MITM is disabled by configuration"
            .to_string()
    } else {
        let ca_directory = storage.join("ca");
        match rsproxy_platform::ca::root_ca_status(&ca_directory) {
            Ok(status) if status.initialized => return None,
            Ok(_) => format!(
                "map.remote on HTTPS/WSS requires interception, but the CA in {} is not initialized",
                ca_directory.display()
            ),
            Err(error) => format!(
                "map.remote on HTTPS/WSS requires interception, but the CA in {} could not be inspected: {error}",
                ca_directory.display()
            ),
        }
    };
    Some(EnvironmentAdvisory {
        kind: HTTPS_MITM_UNAVAILABLE,
        message,
        hint,
        group: None,
        line: None,
    })
}

#[cfg(test)]
#[path = "advisory/tests.rs"]
mod tests;
