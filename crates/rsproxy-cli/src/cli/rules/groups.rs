use super::*;
use crate::rule_store::RuleStore;
use serde::Deserialize;

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

pub(super) fn run_rules_set(args: &[String], api: &str, storage: &Path) -> Result<(), String> {
    let group = group_arg(args, false)?;
    let text = if let Some(file) = option_value(args, "--file") {
        fs::read_to_string(file).map_err(|error| error.to_string())?
    } else {
        read_stdin()?
    };
    save_group(&group, text, api, storage)
}

pub(super) fn run_rules_cat(args: &[String], api: &str, storage: &Path) -> Result<(), String> {
    let group = group_arg(args, false)?;
    let path = group_api_path(&group);
    let text = match api_request("GET", api, &path, "") {
        Ok(text) => text,
        Err(_) => RuleStore::load(storage)
            .map_err(|error| error.to_string())?
            .snapshot()
            .group(&group)
            .map(|group| group.text.clone())
            .ok_or_else(|| format!("rule group `{group}` not found"))?,
    };
    if has_flag(args, "--json") {
        println!("{}", serde_json::json!({"name": group, "text": text}));
    } else {
        print!("{text}");
    }
    Ok(())
}

pub(super) fn run_rules_list(args: &[String], api: &str, storage: &Path) -> Result<(), String> {
    let json = match api_request("GET", api, "/api/rules", "") {
        Ok(json) => json,
        Err(_) => local_group_list_json(storage)?,
    };
    if has_flag(args, "--json") {
        println!("{json}");
        return Ok(());
    }
    let mut groups: Vec<GroupListEntry> =
        serde_json::from_str(&json).map_err(|error| format!("invalid rules list: {error}"))?;
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

pub(super) fn run_rules_remove(args: &[String], api: &str, storage: &Path) -> Result<(), String> {
    let group = group_arg(args, true)?;
    change_group(&group, "DELETE", None, api, storage)
}

pub(super) fn run_rules_toggle(
    args: &[String],
    api: &str,
    storage: &Path,
    enabled: bool,
) -> Result<(), String> {
    let group = group_arg(args, true)?;
    change_group(
        &group,
        "POST",
        Some(if enabled { "enable" } else { "disable" }),
        api,
        storage,
    )
}

pub(super) fn run_rules_edit(args: &[String], api: &str, storage: &Path) -> Result<(), String> {
    let group = group_arg(args, false)?;
    let existing = match api_request("GET", api, &group_api_path(&group), "") {
        Ok(text) => text,
        Err(_) => RuleStore::load(storage)
            .map_err(|error| error.to_string())?
            .snapshot()
            .group(&group)
            .map(|group| group.text.clone())
            .unwrap_or_default(),
    };
    let run_dir = storage.join("run");
    fs::create_dir_all(&run_dir).map_err(|error| error.to_string())?;
    let edit_path = run_dir.join(format!(
        ".rules-edit-{group}-{}-{}.rules",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    fs::write(&edit_path, existing).map_err(|error| error.to_string())?;
    let edit_result = run_editor(&edit_path);
    let text_result = fs::read_to_string(&edit_path).map_err(|error| error.to_string());
    let _ = fs::remove_file(&edit_path);
    edit_result?;
    save_group(&group, text_result?, api, storage)
}

pub(super) fn load_rule_set(args: &[String], api: &str, storage: &Path) -> Result<RuleSet, String> {
    if let Some(file) = option_value(args, "--file") {
        let text = fs::read_to_string(file).map_err(|error| error.to_string())?;
        return RuleSet::parse("default", &text).map_err(format_rule_errors);
    }
    if let Ok(body) = api_request("GET", api, "/api/rules/export", "") {
        let groups: Vec<GroupExportEntry> = serde_json::from_str(&body)
            .map_err(|error| format!("invalid rules export: {error}"))?;
        return RuleSet::parse_groups(
            groups
                .iter()
                .filter(|group| group.enabled)
                .map(|group| (group.name.as_str(), group.text.as_str())),
        )
        .map_err(format_rule_errors);
    }
    Ok(RuleStore::load(storage)
        .map_err(|error| error.to_string())?
        .snapshot()
        .compiled
        .clone())
}

fn save_group(group: &str, text: String, api: &str, storage: &Path) -> Result<(), String> {
    RuleSet::parse(group, &text).map_err(format_rule_errors)?;
    let path = group_api_path(group);
    match api_request("POST", api, &path, &text) {
        Ok(body) => println!("{body}"),
        Err(_) => {
            RuleStore::load(storage)
                .and_then(|store| store.set_group(group, text))
                .map_err(|error| error.to_string())?;
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
) -> Result<(), String> {
    let mut path = group_api_path(group);
    if let Some(action) = action {
        path.push('/');
        path.push_str(action);
    }
    match api_request(method, api, &path, "") {
        Ok(body) => println!("{body}"),
        Err(_) => {
            let store = RuleStore::load(storage).map_err(|error| error.to_string())?;
            match (method, action) {
                ("DELETE", None) => store.remove_group(group),
                ("POST", Some("enable")) => store.set_enabled(group, true),
                ("POST", Some("disable")) => store.set_enabled(group, false),
                _ => return Err("invalid rule group operation".to_string()),
            }
            .map_err(|error| error.to_string())?;
            println!("updated rule group {group} in {}", storage.display());
        }
    }
    Ok(())
}

fn local_group_list_json(storage: &Path) -> Result<String, String> {
    let snapshot = RuleStore::load(storage)
        .map_err(|error| error.to_string())?
        .snapshot();
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
    serde_json::to_string(&groups).map_err(|error| error.to_string())
}

fn run_editor(path: &Path) -> Result<(), String> {
    let editor = env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    let mut parts = editor.split_whitespace();
    let program = parts
        .next()
        .filter(|program| !program.is_empty())
        .ok_or_else(|| "VISUAL/EDITOR is empty".to_string())?;
    let status = Command::new(program)
        .args(parts)
        .arg(path)
        .status()
        .map_err(|error| format!("launch editor: {error}"))?;
    if !status.success() {
        return Err(format!("editor exited with {status}"));
    }
    Ok(())
}

fn group_arg(args: &[String], required: bool) -> Result<String, String> {
    let group = match rules_primary_positional(args) {
        Some(group) => Ok(group),
        None if !required => Ok("default".to_string()),
        None => Err("rule group name required".to_string()),
    }?;
    RuleStore::validate_name(&group).map_err(|error| error.to_string())?;
    Ok(group)
}

fn group_api_path(group: &str) -> String {
    format!("/api/rules/{group}")
}
