//! Persistence and typed-error integration tests for `rsproxy-engine`.
//!
//! The modules exercise disk-backed rule-group transactions and error source
//! chains through the same facade used by the control plane.

mod errors;
mod rule_store;
