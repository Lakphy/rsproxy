use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(super) struct CoverageReport {
    pub schema: String,
    pub workspace: CoverageMetric,
    pub rules: CoverageMetric,
}

#[derive(Debug, Deserialize)]
pub(super) struct CoverageMetric {
    pub lines: f64,
    pub covered: f64,
    pub percent: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct CriterionTargetReport {
    pub schema: String,
    pub unit: String,
    pub metrics: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct CriterionMetric {
    pub mean_ns: f64,
    pub lower_ns: f64,
    pub upper_ns: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct CriterionRegressionReport {
    pub schema: String,
    pub unit: String,
    pub metrics: BTreeMap<String, CriterionMetric>,
}

#[derive(Debug, Deserialize)]
pub(super) struct E2eReport {
    pub schema: String,
    pub driver: String,
    pub requests: f64,
    pub concurrency: f64,
    pub direct: RequestMetrics,
    pub proxy: RequestMetrics,
    pub added_latency: AddedLatency,
    pub memory: E2eMemory,
    #[serde(default)]
    pub whistle: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RequestMetrics {
    pub requests_per_second: f64,
    pub p50_us: f64,
    pub p99_us: f64,
    pub response_bytes: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct AddedLatency {
    pub p50_us: f64,
    pub p99_us: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct E2eMemory {
    pub empty_rss_kib: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct WhistleMetrics {
    pub speedup: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct SoakReport {
    pub schema: String,
    pub driver: String,
    pub duration: String,
    pub elapsed_seconds: f64,
    pub configured: SoakConfiguration,
    pub load: SoakLoad,
    pub process: SoakProcess,
    pub rules: SoakRules,
    pub trace: SoakTrace,
}

#[derive(Debug, Deserialize)]
pub(super) struct SoakConfiguration {
    pub qps: f64,
    pub concurrency: f64,
    pub rules: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct SoakLoad {
    pub requests: f64,
    pub requests_per_second: f64,
    pub success_rate: f64,
    pub response_bytes: f64,
    pub status_200: f64,
    pub errors: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct SoakProcess {
    pub samples: f64,
    pub rss_kib: RssMetric,
    pub fds: GrowthMetric,
}

#[derive(Debug, Deserialize)]
pub(super) struct GrowthMetric {
    pub start: f64,
    pub end: f64,
    #[serde(rename = "max")]
    pub maximum: f64,
    pub end_growth: f64,
    pub peak_growth: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct RssMetric {
    #[serde(flatten)]
    pub growth: GrowthMetric,
    pub slope_kib_per_hour: f64,
    pub last_half_slope_kib_per_hour: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct SoakRules {
    pub loaded: f64,
}

#[derive(Debug, Deserialize)]
pub(super) struct SoakTrace {
    pub sessions: f64,
    pub max_sessions: f64,
    pub queue_dropped: f64,
    pub queue_memory_dropped: f64,
    pub queue_bytes: f64,
    pub pending_sessions: f64,
    pub incomplete_sessions: f64,
    pub orphan_events: f64,
    pub total_memory_bytes: f64,
    pub memory_budget_bytes: f64,
    pub spill_errors: f64,
}
