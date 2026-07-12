use super::fetch_snapshot;
use super::format::json_u64;
use crate::cli::api::api_request;
use serde_json::Value as JsonValue;
use std::time::Instant;

#[derive(Clone, Debug)]
pub(super) struct TuiSnapshot {
    pub(super) status: JsonValue,
    pub(super) sessions: Vec<JsonValue>,
    pub(super) selected_detail: Option<JsonValue>,
    pub(super) error: Option<String>,
}

pub(super) struct TuiApp {
    pub(super) api: String,
    pub(super) limit: usize,
    pub(super) selected: usize,
    pub(super) filter: String,
    pub(super) editing_filter: bool,
    pub(super) detail_tab: DetailTab,
    pub(super) replay_status: Option<String>,
    pub(super) snapshot: TuiSnapshot,
    pub(super) last_refresh: Instant,
}

impl TuiApp {
    pub(super) fn refresh(&mut self) {
        let selected_id = self
            .snapshot
            .sessions
            .get(self.selected)
            .and_then(|session| json_u64(session, "id"));
        self.snapshot = match fetch_snapshot(&self.api, self.limit, selected_id, &self.filter) {
            Ok(snapshot) => snapshot,
            Err(error) => TuiSnapshot {
                status: JsonValue::Null,
                sessions: Vec::new(),
                selected_detail: None,
                error: Some(error),
            },
        };
        if self.selected >= self.snapshot.sessions.len() {
            self.selected = self.snapshot.sessions.len().saturating_sub(1);
        }
        self.last_refresh = Instant::now();
    }

    pub(super) fn replay_selected(&mut self) {
        let Some(id) = self
            .snapshot
            .sessions
            .get(self.selected)
            .and_then(|session| json_u64(session, "id"))
        else {
            self.replay_status = Some("no session selected".to_string());
            return;
        };
        self.replay_status = match api_request("POST", &self.api, &format!("/api/replay/{id}"), "")
        {
            Ok(body) => {
                let status = serde_json::from_str::<JsonValue>(&body)
                    .ok()
                    .and_then(|value| json_u64(&value, "status"))
                    .map_or("?".to_string(), |status| status.to_string());
                Some(format!("replayed id={id} status={status}"))
            }
            Err(error) => Some(format!("replay id={id} failed: {error}")),
        };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DetailTab {
    Overview,
    Headers,
    Body,
    Rules,
}

impl DetailTab {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "overview" | "meta" => Ok(Self::Overview),
            "headers" | "header" => Ok(Self::Headers),
            "body" => Ok(Self::Body),
            "rules" | "rule" => Ok(Self::Rules),
            _ => Err("--tab must be overview, headers, body, or rules".to_string()),
        }
    }

    pub(super) fn next(self) -> Self {
        match self {
            Self::Overview => Self::Headers,
            Self::Headers => Self::Body,
            Self::Body => Self::Rules,
            Self::Rules => Self::Overview,
        }
    }

    pub(super) fn previous(self) -> Self {
        match self {
            Self::Overview => Self::Rules,
            Self::Headers => Self::Overview,
            Self::Body => Self::Headers,
            Self::Rules => Self::Body,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Headers => "headers",
            Self::Body => "body",
            Self::Rules => "rules",
        }
    }
}
