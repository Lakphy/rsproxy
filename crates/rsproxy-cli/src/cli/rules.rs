mod bench;
pub(crate) mod groups;
pub(super) mod request;

use super::command::{RulesArgs, RulesCommand, RuntimeArgs};
use super::config::runtime_config;
use super::util::read_stdin;
use crate::{CliError, CliResult, RuleDiagnostics};
use groups::{
    load_rule_set, run_rules_cat, run_rules_edit, run_rules_list, run_rules_remove, run_rules_set,
    run_rules_toggle,
};
use rsproxy_rules::RuleSet;
use std::fs;

#[cfg(test)]
pub(super) use request::{parse_header_arg, response_meta, rules_test_api_path};

pub(super) fn rules_cmd(args: RulesArgs, json: bool) -> CliResult<()> {
    let config = runtime_config(&RuntimeArgs::from_client(args.client))?;
    let api = config.api.clone();
    let storage = config.engine().storage.clone();
    match args.command {
        RulesCommand::Check(args) => {
            let text = if let Some(file) = args.file {
                fs::read_to_string(&file).map_err(|source| {
                    CliError::io(format!("read rules file {}", file.display()), source)
                })?
            } else {
                read_stdin()?
            };
            match RuleSet::parse("default", &text) {
                Ok(set) if json => println!(
                    "{}",
                    serde_json::json!({"ok": true, "rules": set.rules.len()})
                ),
                Ok(set) => println!("ok: {} rule(s)", set.rules.len()),
                Err(errors) => return Err(RuleDiagnostics(errors).into()),
            }
            Ok(())
        }
        RulesCommand::Set(args) => {
            run_rules_set(&args.group, args.file.as_deref(), &api, &storage, json)
        }
        RulesCommand::Cat(args) => run_rules_cat(&args.group, json, &api, &storage),
        RulesCommand::Edit(args) => run_rules_edit(&args.group, &api, &storage, json),
        RulesCommand::Remove(args) => run_rules_remove(&args.group, &api, &storage, json),
        RulesCommand::Enable(args) => run_rules_toggle(&args.group, &api, &storage, true, json),
        RulesCommand::Disable(args) => run_rules_toggle(&args.group, &api, &storage, false, json),
        RulesCommand::List(_) => run_rules_list(json, &api, &storage),
        RulesCommand::Stats(args) => {
            let rules = load_rule_set(args.file.as_deref(), &api, &storage)?;
            let stats = rules.stats();
            if json {
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
        RulesCommand::Bench(args) => bench::run_rules_bench(args, json, &api, &storage),
        RulesCommand::Test(args) => request::run_rules_test(args, json, &api, &storage),
    }
}
