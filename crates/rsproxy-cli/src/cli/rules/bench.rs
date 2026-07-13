use super::groups::load_rule_set;
use super::request::request_meta;
use crate::cli::command::RulesBenchArgs;
use crate::{CliError, CliResult};
use std::path::Path;
use std::time::Instant;

pub(super) fn run_rules_bench(
    args: RulesBenchArgs,
    json: bool,
    api: &str,
    storage: &Path,
) -> CliResult<()> {
    let rules = load_rule_set(args.source.file.as_deref(), api, storage)?;
    let url = args
        .url
        .or(args.positional_url)
        .ok_or_else(|| CliError::Usage("rules bench requires --url URL".to_string()))?;
    let request = request_meta(&args.request, url)?;
    let iterations = args.iterations.unwrap_or(10_000).max(1);
    let warmup = args.warmup.unwrap_or(100);
    let mut matched_actions = 0usize;
    for _ in 0..warmup {
        matched_actions = matched_actions.wrapping_add(rules.resolve(&request).actions.len());
    }
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        let result = rules.resolve(&request);
        matched_actions = matched_actions.wrapping_add(result.actions.len());
        samples.push(started.elapsed().as_nanos());
    }
    samples.sort_unstable();
    let stats = rules.stats();
    let p50_ns = percentile(&samples, 50);
    let p99_ns = percentile(&samples, 99);
    let max_ns = samples.last().copied().unwrap_or(0);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "iterations": iterations,
                "warmup": warmup,
                "rules": stats.rules,
                "indexed_rules": stats.indexed_rules,
                "global_rules": stats.global_rules,
                "prefilter_literals": stats.prefilter_literals,
                "prefilter_rules": stats.prefilter_rules,
                "matched_actions": matched_actions,
                "p50_ns": p50_ns,
                "p99_ns": p99_ns,
                "max_ns": max_ns,
            })
        );
    } else {
        println!("iterations={iterations}");
        println!("warmup={warmup}");
        println!("rules={}", stats.rules);
        println!("indexed_rules={}", stats.indexed_rules);
        println!("global_rules={}", stats.global_rules);
        println!("prefilter_literals={}", stats.prefilter_literals);
        println!("prefilter_rules={}", stats.prefilter_rules);
        println!("matched_actions={matched_actions}");
        println!("p50_ns={p50_ns}");
        println!("p99_ns={p99_ns}");
        println!("max_ns={max_ns}");
    }
    Ok(())
}

fn percentile(samples: &[u128], percentile: usize) -> u128 {
    if samples.is_empty() {
        return 0;
    }
    let percentile = percentile.min(100);
    let rank = (samples.len() * percentile).div_ceil(100).saturating_sub(1);
    samples[rank.min(samples.len() - 1)]
}
