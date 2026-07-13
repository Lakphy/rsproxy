use crate::cli::util::read_stdin;
use crate::{CliError, CliResult, RuleDiagnostics};
use rsproxy_control::api_request;
use rsproxy_engine::RuleStore;
use rsproxy_rules::RuleSet;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Deserialize)]
struct GroupListEntry {
    name: String,
    enabled: bool,
    order: usize,
    rules: usize,
}

#[derive(Deserialize)]
struct GroupExportEntry {
    name: String,
    enabled: bool,
    text: String,
}

pub(super) fn run_rules_set(
    group: &str,
    file: Option<&Path>,
    api: &str,
    storage: &Path,
) -> CliResult<()> {
    validate_group(group)?;
    let text = if let Some(file) = file {
        fs::read_to_string(file)
            .map_err(|source| CliError::io(format!("read rules file {}", file.display()), source))?
    } else {
        read_stdin()?
    };
    save_group(group, text, api, storage)
}

pub(super) fn run_rules_cat(group: &str, json: bool, api: &str, storage: &Path) -> CliResult<()> {
    validate_group(group)?;
    let path = group_api_path(group);
    let text = match api_request("GET", api, &path, "") {
        Ok(text) => text,
        Err(_) => RuleStore::load(storage)?
            .snapshot()
            .group(group)
            .map(|group| group.text.clone())
            .ok_or_else(|| CliError::Usage(format!("rule group `{group}` not found")))?,
    };
    if json {
        println!("{}", serde_json::json!({"name": group, "text": text}));
    } else {
        print!("{text}");
    }
    Ok(())
}

pub(super) fn run_rules_list(json_output: bool, api: &str, storage: &Path) -> CliResult<()> {
    let json = match api_request("GET", api, "/api/rules", "") {
        Ok(json) => json,
        Err(_) => local_group_list_json(storage)?,
    };
    if json_output {
        println!("{json}");
        return Ok(());
    }
    let mut groups: Vec<GroupListEntry> =
        serde_json::from_str(&json).map_err(|source| CliError::Json {
            context: "parse rules list",
            source,
        })?;
    groups.sort_by_key(|group| group.order);
    println!("ORDER  GROUP  ENABLED  RULES");
    for group in groups {
        println!(
            "{:<5}  {}  {:<7}  {}",
            group.order,
            group.name,
            if group.enabled { "yes" } else { "no" },
            group.rules
        );
    }
    Ok(())
}

pub(super) fn run_rules_remove(group: &str, api: &str, storage: &Path) -> CliResult<()> {
    validate_group(group)?;
    change_group(group, "DELETE", None, api, storage)
}

pub(super) fn run_rules_toggle(
    group: &str,
    api: &str,
    storage: &Path,
    enabled: bool,
) -> CliResult<()> {
    validate_group(group)?;
    change_group(
        group,
        "POST",
        Some(if enabled { "enable" } else { "disable" }),
        api,
        storage,
    )
}

pub(super) fn run_rules_edit(group: &str, api: &str, storage: &Path) -> CliResult<()> {
    validate_group(group)?;
    let existing = match api_request("GET", api, &group_api_path(group), "") {
        Ok(text) => text,
        Err(_) => RuleStore::load(storage)?
            .snapshot()
            .group(group)
            .map(|group| group.text.clone())
            .unwrap_or_default(),
    };
    let run_dir = storage.join("run");
    fs::create_dir_all(&run_dir).map_err(|source| {
        CliError::io(
            format!("create rules editor directory {}", run_dir.display()),
            source,
        )
    })?;
    let edit_path = run_dir.join(format!(
        ".rules-edit-{group}-{}-{}.rules",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    fs::write(&edit_path, existing).map_err(|source| {
        CliError::io(
            format!("write rules editor file {}", edit_path.display()),
            source,
        )
    })?;
    let edit_result = run_editor(&edit_path);
    let text_result = fs::read_to_string(&edit_path).map_err(|source| {
        CliError::io(
            format!("read rules editor file {}", edit_path.display()),
            source,
        )
    });
    let _ = fs::remove_file(&edit_path);
    edit_result?;
    save_group(group, text_result?, api, storage)
}

pub(super) fn load_rule_set(file: Option<&Path>, api: &str, storage: &Path) -> CliResult<RuleSet> {
    if let Some(file) = file {
        let text = fs::read_to_string(file).map_err(|source| {
            CliError::io(format!("read rules file {}", file.display()), source)
        })?;
        return RuleSet::parse("default", &text)
            .map_err(RuleDiagnostics)
            .map_err(Into::into);
    }
    if let Ok(body) = api_request("GET", api, "/api/rules/export", "") {
        let groups: Vec<GroupExportEntry> =
            serde_json::from_str(&body).map_err(|source| CliError::Json {
                context: "parse rules export",
                source,
            })?;
        return RuleSet::parse_groups(
            groups
                .iter()
                .filter(|group| group.enabled)
                .map(|group| (group.name.as_str(), group.text.as_str())),
        )
        .map_err(RuleDiagnostics)
        .map_err(Into::into);
    }
    Ok(RuleStore::load(storage)?.snapshot().compiled.clone())
}

fn save_group(group: &str, text: String, api: &str, storage: &Path) -> CliResult<()> {
    RuleSet::parse(group, &text).map_err(RuleDiagnostics)?;
    let path = group_api_path(group);
    match api_request("POST", api, &path, &text) {
        Ok(body) => println!("{body}"),
        Err(_) => {
            RuleStore::load(storage)?.set_group(group, text)?;
            println!("saved rule group {group} to {}", storage.display());
        }
    }
    Ok(())
}

fn change_group(
    group: &str,
    method: &str,
    action: Option<&str>,
    api: &str,
    storage: &Path,
) -> CliResult<()> {
    let mut path = group_api_path(group);
    if let Some(action) = action {
        path.push('/');
        path.push_str(action);
    }
    match api_request(method, api, &path, "") {
        Ok(body) => println!("{body}"),
        Err(_) => {
            let store = RuleStore::load(storage)?;
            match (method, action) {
                ("DELETE", None) => store.remove_group(group),
                ("POST", Some("enable")) => store.set_enabled(group, true),
                ("POST", Some("disable")) => store.set_enabled(group, false),
                _ => return Err(CliError::InvalidRuleOperation),
            }?;
            println!("updated rule group {group} in {}", storage.display());
        }
    }
    Ok(())
}

fn local_group_list_json(storage: &Path) -> CliResult<String> {
    let snapshot = RuleStore::load(storage)?.snapshot();
    let groups = snapshot
        .groups
        .iter()
        .enumerate()
        .map(|(order, group)| {
            let rules = RuleSet::parse(&group.name, &group.text)
                .map(|rules| rules.rules.len())
                .unwrap_or_default();
            serde_json::json!({
                "name": group.name,
                "enabled": group.enabled,
                "order": order,
                "rules": rules,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&groups).map_err(|source| CliError::Json {
        context: "serialize rules list",
        source,
    })
}

fn run_editor(path: &Path) -> CliResult<()> {
    let editor = env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    let mut parts = editor.split_whitespace();
    let program = parts
        .next()
        .filter(|program| !program.is_empty())
        .ok_or_else(|| CliError::Usage("VISUAL/EDITOR is empty".to_string()))?;
    let status = Command::new(program)
        .args(parts)
        .arg(path)
        .status()
        .map_err(|source| CliError::io(format!("launch editor `{program}`"), source))?;
    if !status.success() {
        return Err(CliError::ExternalCommand {
            command: program.to_string(),
            status,
        });
    }
    Ok(())
}

fn validate_group(group: &str) -> CliResult<()> {
    RuleStore::validate_name(group)?;
    Ok(())
}

fn group_api_path(group: &str) -> String {
    format!("/api/rules/{group}")
}
