//! Rule DSL integration and corpus tests sharing one test binary.
// Test failures should stop at the assertion site; production code remains denied.
#![allow(clippy::unwrap_used)]

#[path = "../support/fuzz_harness.rs"]
mod fuzz_harness;
#[path = "../support/whistle_fixture.rs"]
mod whistle_fixture;

mod complexity;
mod corpus;
mod fuzz_seeds;
mod properties;
mod value_matrix;
mod value_sources;
mod whistle_migration;
mod whistle_options;
