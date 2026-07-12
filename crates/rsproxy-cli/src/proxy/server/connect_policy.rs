use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PassthroughReason {
    Disabled,
    RuleBypass,
    MissingCa,
    RememberedFailure,
}

pub(super) enum ConnectDecision {
    Inspect { host: String, flags: Vec<String> },
    Passthrough { flags: Vec<String> },
}

pub(super) fn decide(
    state: &SharedState,
    target: &str,
    actions: &[ResolvedAction],
) -> ConnectDecision {
    let (host, _) = split_addr(target, 443);
    let rule_bypass = connect_bypass(actions);
    let ca_initialized = if state.config.no_mitm || rule_bypass {
        false
    } else {
        ca_initialized(state)
    };
    let remembered_failure = !state.config.no_mitm
        && !rule_bypass
        && ca_initialized
        && !state.config.strict_mitm
        && state.mitm_failures.lock().unwrap().is_active(&host);
    let reason = passthrough_reason(
        state.config.no_mitm,
        rule_bypass,
        ca_initialized,
        remembered_failure,
    );
    if let Some(reason) = reason {
        return ConnectDecision::Passthrough {
            flags: vec![reason.flag().to_string()],
        };
    }

    let flags = state
        .config
        .strict_mitm
        .then(|| "strict-mitm".to_string())
        .into_iter()
        .collect();
    ConnectDecision::Inspect { host, flags }
}

fn passthrough_reason(
    no_mitm: bool,
    rule_bypass: bool,
    ca_initialized: bool,
    remembered_failure: bool,
) -> Option<PassthroughReason> {
    if no_mitm {
        Some(PassthroughReason::Disabled)
    } else if rule_bypass {
        Some(PassthroughReason::RuleBypass)
    } else if !ca_initialized {
        Some(PassthroughReason::MissingCa)
    } else if remembered_failure {
        Some(PassthroughReason::RememberedFailure)
    } else {
        None
    }
}

impl PassthroughReason {
    fn flag(self) -> &'static str {
        match self {
            Self::Disabled => "no-mitm",
            Self::RuleBypass => "bypass",
            Self::MissingCa => "no-ca",
            Self::RememberedFailure => "mitm-fallback-cache-hit",
        }
    }
}

#[cfg(test)]
#[path = "tests/connect_policy.rs"]
mod tests;
