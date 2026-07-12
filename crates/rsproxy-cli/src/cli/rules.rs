use super::*;

mod bench;
mod groups;
mod request;

use bench::run_rules_bench;
use groups::{
    load_rule_set, run_rules_cat, run_rules_edit, run_rules_list, run_rules_remove, run_rules_set,
    run_rules_toggle,
};
use request::run_rules_test;
#[cfg(test)]
pub(super) use request::{parse_header_arg, response_meta, rules_test_api_path};
pub(super) use request::{
    request_body, request_client_ip, request_headers, request_method, request_server_ip,
    request_url,
};

pub(super) fn rules_cmd(mut args: Vec<String>) -> Result<(), String> {
    if args.is_empty() {
        return Err("rules command required".to_string());
    }
    let sub = args.remove(0);
    let config = runtime_config(&args)?;
    let api = config.api;
    let storage = config.storage;
    match sub.as_str() {
        "check" => {
            let text = if let Some(file) = rules_primary_positional(&args) {
                fs::read_to_string(file).map_err(|e| e.to_string())?
            } else {
                read_stdin()?
            };
            match RuleSet::parse("default", &text) {
                Ok(set) if has_flag(&args, "--json") => println!(
                    "{}",
                    serde_json::json!({"ok": true, "rules": set.rules.len()})
                ),
                Ok(set) => println!("ok: {} rule(s)", set.rules.len()),
                Err(errors) => return Err(format_rule_errors(errors)),
            }
            Ok(())
        }
        "set" => run_rules_set(&args, &api, &storage),
        "cat" => run_rules_cat(&args, &api, &storage),
        "edit" => run_rules_edit(&args, &api, &storage),
        "rm" => run_rules_remove(&args, &api, &storage),
        "enable" => run_rules_toggle(&args, &api, &storage, true),
        "disable" => run_rules_toggle(&args, &api, &storage, false),
        "ls" => run_rules_list(&args, &api, &storage),
        "stats" => {
            let rules = load_rule_set(&args, &api, &storage)?;
            let stats = rules.stats();
            if has_flag(&args, "--json") {
                println!(
                    "{}",
                    serde_json::json!({
                        "rules": stats.rules,
                        "disabled": stats.disabled,
                        "domain_exact_entries": stats.domain_exact_entries,
                        "domain_suffix_entries": stats.domain_suffix_entries,
                        "indexed_rules": stats.indexed_rules,
                        "global_rules": stats.global_rules,
                        "prefilter_literals": stats.prefilter_literals,
                        "prefilter_rules": stats.prefilter_rules,
                    })
                );
            } else {
                println!("rules={}", stats.rules);
                println!("disabled={}", stats.disabled);
                println!("domain_exact_entries={}", stats.domain_exact_entries);
                println!("domain_suffix_entries={}", stats.domain_suffix_entries);
                println!("indexed_rules={}", stats.indexed_rules);
                println!("global_rules={}", stats.global_rules);
                println!("prefilter_literals={}", stats.prefilter_literals);
                println!("prefilter_rules={}", stats.prefilter_rules);
            }
            Ok(())
        }
        "bench" => run_rules_bench(&args, &api, &storage),
        "test" => run_rules_test(&args, &api, &storage),
        _ => Err(format!("unknown rules command `{sub}`")),
    }
}

fn rules_primary_positional(args: &[String]) -> Option<String> {
    positional_skipping_values(
        args,
        &["--api", "--api-token", "--config", "--storage", "--file"],
    )
}
