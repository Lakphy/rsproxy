mod advisory;
mod bench;
pub(crate) mod groups;
mod help;
pub(super) mod request;

use super::command::{RulesArgs, RulesCommand, RulesMigrateArgs, RuntimeArgs};
use super::config::runtime_config;
use super::util::{read_stdin_bounded, read_utf8_file_bounded};
use crate::{CliError, CliResult, RuleDiagnostics};
use advisory::{lint_advisories, print_advisories};
use groups::{
    load_rule_set, run_rules_cat, run_rules_edit, run_rules_list, run_rules_remove, run_rules_set,
    run_rules_toggle,
};
use rsproxy_rules::{
    MAX_RULE_LINT_COMPARISON_BYTES, MAX_RULE_LINT_COMPARISONS, MAX_RULE_LINT_FINDINGS,
    MAX_RULE_LINT_REPORT_BYTES, MAX_RULE_SNAPSHOT_SOURCE_BYTES, RuleSet, migrate_rule_source_v3,
};
use std::fs;

#[cfg(test)]
pub(super) use request::{parse_header_arg, response_meta, rules_test_api_path};

pub(super) fn rules_cmd(args: RulesArgs, json: bool) -> CliResult<()> {
    let command = match args.command {
        Some(RulesCommand::Help(args)) => return help::run_rules_help(args, json),
        Some(RulesCommand::Migrate(args)) => return run_rules_migrate(args, json),
        command => command,
    };
    let config = runtime_config(&RuntimeArgs::from_client(args.client))?;
    let api = config.api.clone();
    let storage = config.engine().storage.clone();
    let no_mitm = config.engine().no_mitm;
    // `rsproxy rules` with no subcommand defaults to listing groups, matching
    // the default-status behavior of `ca` and `proxy`.
    let Some(command) = command else {
        return run_rules_list(json, &api, &storage);
    };
    match command {
        RulesCommand::Help(_) => unreachable!("rules help returned before runtime configuration"),
        RulesCommand::Migrate(_) => {
            unreachable!("rules migrate returned before runtime configuration")
        }
        RulesCommand::Check(args) => {
            let text = if let Some(file) = args.file {
                read_utf8_file_bounded(&file, MAX_RULE_SNAPSHOT_SOURCE_BYTES, "rules file")?
            } else {
                read_stdin_bounded(MAX_RULE_SNAPSHOT_SOURCE_BYTES, "rules stdin")?
            };
            match RuleSet::parse_versioned("default", &text) {
                Ok(set) if json => println!(
                    "{}",
                    serde_json::json!({"ok": true, "rules": set.rules().len()})
                ),
                Ok(set) => println!("ok: {} rule(s)", set.rules().len()),
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
        RulesCommand::Lint(args) => {
            let rules = load_rule_set(args.file.as_deref(), &api, &storage)?;
            let shadow_report = rules.lint_report();
            let semantic_report = rules.semantic_lint_report();
            let advisories = lint_advisories(&rules, &storage, no_mitm);
            let shadow_findings = &shadow_report.findings;
            let semantic_findings = &semantic_report.findings;
            let complete = shadow_report.complete && semantic_report.complete;
            let finding_count = shadow_findings.len() + semantic_findings.len();
            if json {
                let findings = shadow_findings
                    .iter()
                    .map(|finding| {
                        serde_json::json!({
                            "kind": "shadowed-rule",
                            "group": finding.group.as_ref(),
                            "line": finding.line,
                            "rule": finding.raw.as_ref(),
                            "message": format!(
                                "never wins {} because an earlier broader rule matches first",
                                finding.families.join(", ")
                            ),
                            "shadowed_by_group": finding.shadowed_by_group.as_ref(),
                            "shadowed_by_line": finding.shadowed_by_line,
                            "shadowed_by_rule": finding.shadowed_by_raw.as_ref(),
                            "families": finding.families,
                        })
                    })
                    .chain(semantic_findings.iter().map(|finding| {
                        serde_json::json!({
                            "kind": finding.kind.as_str(),
                            "group": finding.group.as_ref(),
                            "line": finding.line,
                            "rule": finding.raw.as_ref(),
                            "message": finding.message,
                            "families": finding.families,
                        })
                    }))
                    .collect::<Vec<_>>();
                println!(
                    "{}",
                    serde_json::json!({
                        "schema": "rsproxy.rules.lint/v1",
                        "ok": finding_count == 0 && complete,
                        "complete": complete,
                        "shadow_comparisons": shadow_report.comparisons,
                        "shadow_comparison_bytes": shadow_report.comparison_bytes,
                        "limits": {
                            "comparisons": MAX_RULE_LINT_COMPARISONS,
                            "comparison_bytes": MAX_RULE_LINT_COMPARISON_BYTES,
                            "findings_per_report": MAX_RULE_LINT_FINDINGS,
                            "report_bytes": MAX_RULE_LINT_REPORT_BYTES,
                        },
                        "findings": findings,
                        "warnings": advisories
                            .iter()
                            .map(advisory::EnvironmentAdvisory::to_json)
                            .collect::<Vec<_>>(),
                    })
                );
            } else if finding_count == 0 && complete {
                println!("ok: no rule lint findings");
            } else {
                for finding in shadow_findings {
                    println!(
                        "{}:{} `{}` never wins {} — shadowed by earlier broader rule {}:{} `{}`",
                        finding.group,
                        finding.line,
                        finding.raw,
                        finding.families.join(", "),
                        finding.shadowed_by_group,
                        finding.shadowed_by_line,
                        finding.shadowed_by_raw,
                    );
                }
                for finding in semantic_findings {
                    let families = if finding.families.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", finding.families.join(", "))
                    };
                    println!(
                        "{}:{} `{}` {} — {}{}",
                        finding.group,
                        finding.line,
                        finding.raw,
                        finding.kind.as_str(),
                        finding.message,
                        families,
                    );
                }
                if !shadow_findings.is_empty() {
                    println!(
                        "\nRules resolve first-match-wins per action family (group order, then line order; `@important` first). Move the specific rule above the broader one."
                    );
                }
                if !semantic_findings.is_empty() {
                    println!(
                        "\nKeep one action per single-action family, remove contradictory guards, and review skip, phase, local-response, and direct/upstream combinations."
                    );
                }
                if !complete {
                    println!(
                        "\nLint report is incomplete because a published comparison, finding, or byte limit was reached; narrow or split the ruleset."
                    );
                }
            }
            if !json && !advisories.is_empty() {
                println!();
                print_advisories(&advisories);
            }
            if finding_count == 0 && complete {
                Ok(())
            } else if !complete {
                Err(CliError::LintIncomplete {
                    findings: finding_count,
                })
            } else {
                Err(CliError::LintFindings(finding_count))
            }
        }
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
                        "compiled_globs": stats.compiled_globs,
                        "compiled_body_literals": stats.compiled_body_literals,
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
                println!("compiled_globs={}", stats.compiled_globs);
                println!("compiled_body_literals={}", stats.compiled_body_literals);
            }
            Ok(())
        }
        RulesCommand::Bench(args) => bench::run_rules_bench(args, json, &api, &storage),
        RulesCommand::Test(args) => request::run_rules_test(args, json, &api, &storage, no_mitm),
    }
}

fn run_rules_migrate(args: RulesMigrateArgs, json: bool) -> CliResult<()> {
    let source = if let Some(file) = &args.file {
        read_utf8_file_bounded(file, MAX_RULE_SNAPSHOT_SOURCE_BYTES, "rules file")?
    } else {
        read_stdin_bounded(MAX_RULE_SNAPSHOT_SOURCE_BYTES, "rules stdin")?
    };
    let migrated = migrate_rule_source_v3(&source);
    let rules = RuleSet::parse_versioned("default", &migrated).map_err(RuleDiagnostics)?;

    if args.write {
        let file = args
            .file
            .as_ref()
            .expect("clap requires FILE when --write is supplied");
        let parent = file.parent().unwrap_or_else(|| std::path::Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|source| CliError::io(format!("create {}", parent.display()), source))?;
        let temporary = parent.join(format!(
            ".{}.migrate-{}-{}",
            file.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("rules"),
            std::process::id(),
            rsproxy_trace::now_millis()
        ));
        fs::write(&temporary, migrated.as_bytes()).map_err(|source| {
            CliError::io(
                format!("write migrated rules {}", temporary.display()),
                source,
            )
        })?;
        if let Err(source) = fs::rename(&temporary, file) {
            let _ = fs::remove_file(&temporary);
            return Err(CliError::io(
                format!("replace rules file {}", file.display()),
                source,
            ));
        }
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "language": rules.language_version(),
                    "rules": rules.rules().len(),
                    "file": file,
                    "written": true,
                })
            );
        } else {
            println!(
                "migrated {} rule(s) to v{} in {}",
                rules.rules().len(),
                rules.language_version(),
                file.display()
            );
        }
    } else if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "language": rules.language_version(),
                "rules": rules.rules().len(),
                "source": migrated,
            })
        );
    } else {
        print!("{migrated}");
    }
    Ok(())
}
