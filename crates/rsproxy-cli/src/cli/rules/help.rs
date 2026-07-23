use super::super::command::RulesHelpArgs;
use crate::{CliError, CliResult};
use rsproxy_rules::{
    ACTION_SYNTAX, ActionFamily, CONDITION_SYNTAX, MATCHER_SYNTAX, MAX_HTTP_STATUS,
    MAX_RULE_ACTIONS_PER_RULE, MAX_RULE_ACTIONS_PER_SNAPSHOT,
    MAX_RULE_BODY_CONDITIONS_PER_SNAPSHOT, MAX_RULE_CALL_ARGUMENTS,
    MAX_RULE_CONDITION_NODES_PER_RULE, MAX_RULE_CONDITION_NODES_PER_SNAPSHOT, MAX_RULE_DIAGNOSTICS,
    MAX_RULE_EXPLAIN_BYTES, MAX_RULE_EXPLAIN_VALUE_BYTES, MAX_RULE_EXTERNAL_PATH_BYTES,
    MAX_RULE_EXTERNAL_VALUE_BYTES, MAX_RULE_GLOB_CAPTURES, MAX_RULE_GROUP_NAME_BYTES,
    MAX_RULE_GROUPS_PER_SNAPSHOT, MAX_RULE_LINT_COMPARISON_BYTES, MAX_RULE_LINT_COMPARISONS,
    MAX_RULE_LINT_FINDINGS, MAX_RULE_LINT_REPORT_BYTES, MAX_RULE_MOCK_FILE_CANDIDATES,
    MAX_RULE_PARSE_NESTING, MAX_RULE_PROPERTIES_PER_RULE, MAX_RULE_RENDERED_TAG_BYTES,
    MAX_RULE_RENDERED_VALUE_BYTES, MAX_RULE_SNAPSHOT_SOURCE_BYTES, MAX_RULE_SOURCE_LINE_BYTES,
    MAX_RULE_TAGS_PER_REQUEST, MAX_RULE_TLS_PEM_BYTES, MAX_RULE_UPSTREAM_HOPS,
    MAX_RULES_PER_SNAPSHOT, MIN_FINAL_HTTP_STATUS, MIN_HTTP_STATUS, PROPERTY_SYNTAX,
    REDIRECT_STATUSES, RULE_LANGUAGE_VERSION, RuleSyntaxSpelling,
};
use serde_json::{Value as JsonValue, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Topic {
    id: &'static str,
    category: &'static str,
    summary: &'static str,
    syntax: &'static [&'static str],
    examples: &'static [&'static str],
    notes: &'static [&'static str],
    aliases: &'static [&'static str],
    related: &'static [&'static str],
}

macro_rules! topic {
    ($id:literal, $category:literal, $summary:literal,
     [$($syntax:literal),* $(,)?],
     [$($example:literal),* $(,)?],
     [$($note:literal),* $(,)?],
     [$($alias:literal),* $(,)?],
     [$($related:literal),* $(,)?]) => {
        Topic {
            id: $id,
            category: $category,
            summary: $summary,
            syntax: &[$($syntax),*],
            examples: &[$($example),*],
            notes: &[$($note),*],
            aliases: &[$($alias),*],
            related: &[$($related),*],
        }
    };
}

const CATEGORIES: &[(&str, &str)] = &[
    ("concepts", "Language concepts"),
    ("matchers", "URL matchers"),
    ("conditions", "Conditions"),
    ("actions", "Actions"),
    ("properties", "Rule properties"),
];

mod catalog;

use catalog::TOPIC_GROUPS;

fn topics() -> impl Iterator<Item = &'static Topic> {
    TOPIC_GROUPS.iter().flat_map(|group| group.iter())
}

#[cfg(test)]
fn topic_count() -> usize {
    TOPIC_GROUPS.iter().map(|group| group.len()).sum()
}

pub(super) fn run_rules_help(args: RulesHelpArgs, json_output: bool) -> CliResult<()> {
    let (kind, query, topics) = select_topics(args)?;
    if json_output {
        println!(
            "{}",
            json!({
                "schema": "rsproxy.rules.help/v1",
                "language_version": RULE_LANGUAGE_VERSION,
                "limits": {
                    "source_line_bytes": MAX_RULE_SOURCE_LINE_BYTES,
                    "snapshot_source_bytes": MAX_RULE_SNAPSHOT_SOURCE_BYTES,
                    "group_name_bytes": MAX_RULE_GROUP_NAME_BYTES,
                    "groups_per_snapshot": MAX_RULE_GROUPS_PER_SNAPSHOT,
                    "rules_per_snapshot": MAX_RULES_PER_SNAPSHOT,
                    "diagnostics": MAX_RULE_DIAGNOSTICS,
                    "actions_per_rule": MAX_RULE_ACTIONS_PER_RULE,
                    "actions_per_snapshot": MAX_RULE_ACTIONS_PER_SNAPSHOT,
                    "condition_nodes_per_rule": MAX_RULE_CONDITION_NODES_PER_RULE,
                    "condition_nodes_per_snapshot": MAX_RULE_CONDITION_NODES_PER_SNAPSHOT,
                    "body_conditions_per_snapshot": MAX_RULE_BODY_CONDITIONS_PER_SNAPSHOT,
                    "properties_per_rule": MAX_RULE_PROPERTIES_PER_RULE,
                    "call_arguments": MAX_RULE_CALL_ARGUMENTS,
                    "external_value_bytes": MAX_RULE_EXTERNAL_VALUE_BYTES,
                    "rendered_value_bytes": MAX_RULE_RENDERED_VALUE_BYTES,
                    "external_path_bytes": MAX_RULE_EXTERNAL_PATH_BYTES,
                    "rendered_tag_bytes": MAX_RULE_RENDERED_TAG_BYTES,
                    "tags_per_request": MAX_RULE_TAGS_PER_REQUEST,
                    "explain_value_bytes": MAX_RULE_EXPLAIN_VALUE_BYTES,
                    "explain_bytes": MAX_RULE_EXPLAIN_BYTES,
                    "upstream_hops": MAX_RULE_UPSTREAM_HOPS,
                    "mock_file_candidates": MAX_RULE_MOCK_FILE_CANDIDATES,
                    "lint_comparisons": MAX_RULE_LINT_COMPARISONS,
                    "lint_comparison_bytes": MAX_RULE_LINT_COMPARISON_BYTES,
                    "lint_findings_per_report": MAX_RULE_LINT_FINDINGS,
                    "lint_report_bytes": MAX_RULE_LINT_REPORT_BYTES,
                    "tls_pem_bytes": MAX_RULE_TLS_PEM_BYTES,
                    "parse_nesting": MAX_RULE_PARSE_NESTING,
                    "glob_captures": MAX_RULE_GLOB_CAPTURES,
                    "condition_http_status": [MIN_HTTP_STATUS, MAX_HTTP_STATUS],
                    "final_http_status": [MIN_FINAL_HTTP_STATUS, MAX_HTTP_STATUS],
                    "redirect_status": REDIRECT_STATUSES,
                },
                "kind": kind,
                "query": query,
                "topics": topics.iter().map(|topic| topic_json(topic)).collect::<Vec<_>>(),
            })
        );
        return Ok(());
    }

    if kind == "topic" {
        render_topic(topics[0]);
    } else {
        render_index(kind, query.as_deref(), &topics);
    }
    Ok(())
}

fn select_topics(
    args: RulesHelpArgs,
) -> CliResult<(&'static str, Option<String>, Vec<&'static Topic>)> {
    if let Some(search) = args.search {
        let terms = normalized_terms(&search);
        if terms.is_empty() {
            return Err(CliError::Usage(
                "rules help --search requires at least one non-whitespace term".to_string(),
            ));
        }
        let topics = topics()
            .filter(|topic| topic_matches_terms(topic, &terms))
            .collect::<Vec<_>>();
        if topics.is_empty() {
            return Err(CliError::Usage(format!(
                "no rule help topics match `{}`; run `rsproxy rules help` for the complete index",
                search.trim()
            )));
        }
        return Ok(("search", Some(search.trim().to_string()), topics));
    }

    let Some(query) = args.topic else {
        return Ok(("index", None, topics().collect()));
    };
    let normalized = normalize(&query);
    if let Some(category) = category_name(&normalized) {
        let topics = topics()
            .filter(|topic| topic.category == category)
            .collect();
        return Ok(("category", Some(category.to_string()), topics));
    }

    let exact = topics()
        .filter(|topic| {
            normalize(topic.id) == normalized
                || topic
                    .aliases
                    .iter()
                    .any(|alias| normalize(alias) == normalized)
                || topic_dsl_spellings(topic).any(|spelling| {
                    normalize(spelling.canonical) == normalized
                        || spelling
                            .aliases
                            .iter()
                            .any(|alias| normalize(alias) == normalized)
                })
        })
        .collect::<Vec<_>>();
    if exact.len() == 1 {
        return Ok(("topic", Some(query), exact));
    }
    if exact.len() > 1 {
        return Err(ambiguous_topic(&query, &exact));
    }

    let shorthand = topics()
        .filter(|topic| {
            topic
                .id
                .split_once('.')
                .is_some_and(|(_, suffix)| normalize(suffix) == normalized)
        })
        .collect::<Vec<_>>();
    if shorthand.len() == 1 {
        return Ok(("topic", Some(query), shorthand));
    }
    if shorthand.len() > 1 {
        return Err(ambiguous_topic(&query, &shorthand));
    }

    let terms = normalized_terms(&query);
    let suggestions = topics()
        .filter(|topic| topic_matches_terms(topic, &terms))
        .take(6)
        .map(|topic| topic.id)
        .collect::<Vec<_>>();
    let suffix = if suggestions.is_empty() {
        "run `rsproxy rules help` for the complete index".to_string()
    } else {
        format!("possible topics: {}", suggestions.join(", "))
    };
    Err(CliError::Usage(format!(
        "unknown rule help topic `{}`; {suffix}",
        query.trim()
    )))
}

fn ambiguous_topic(query: &str, topics: &[&Topic]) -> CliError {
    CliError::Usage(format!(
        "ambiguous rule help topic `{}`; use one of: {}",
        query.trim(),
        topics
            .iter()
            .map(|topic| topic.id)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn category_name(input: &str) -> Option<&'static str> {
    match input {
        "concept" | "concepts" => Some("concepts"),
        "matcher" | "matchers" => Some("matchers"),
        "condition" | "conditions" => Some("conditions"),
        "action" | "actions" => Some("actions"),
        "property" | "properties" => Some("properties"),
        _ => None,
    }
}

fn normalize(input: &str) -> String {
    input.trim().to_ascii_lowercase().replace('_', "-")
}

fn normalized_terms(input: &str) -> Vec<String> {
    input.split_whitespace().map(normalize).collect()
}

fn topic_matches_terms(topic: &Topic, terms: &[String]) -> bool {
    let mut haystack = std::iter::once(topic.id)
        .chain(std::iter::once(topic.category))
        .chain(std::iter::once(topic.summary))
        .chain(topic.syntax.iter().copied())
        .chain(topic.examples.iter().copied())
        .chain(topic.notes.iter().copied())
        .chain(topic.aliases.iter().copied())
        .chain(topic.related.iter().copied())
        .chain(topic_dsl_spellings(topic).flat_map(|spelling| {
            std::iter::once(spelling.canonical).chain(spelling.aliases.iter().copied())
        }))
        .map(normalize)
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(phases) = action_phases(topic) {
        for phase in phases {
            haystack.push('\n');
            haystack.push_str(phase.as_str());
        }
    }
    terms.iter().all(|term| haystack.contains(term))
}

fn topic_json(topic: &Topic) -> JsonValue {
    let resolution = action_resolution(topic);
    json!({
        "id": topic.id,
        "category": topic.category,
        "summary": topic.summary,
        "syntax": topic.syntax,
        "examples": topic.examples,
        "notes": topic.notes,
        "aliases": topic.aliases,
        "related": topic.related,
        "dsl_spellings": topic_dsl_spellings(topic)
            .map(|spelling| json!({
                "canonical": spelling.canonical,
                "aliases": spelling.aliases,
                "prefix": spelling.prefix,
            }))
            .collect::<Vec<_>>(),
        "resolution": resolution,
        "phases": action_phases(topic).map(|phases| {
            phases.iter().map(|phase| phase.as_str()).collect::<Vec<_>>()
        }),
    })
}

fn render_topic(topic: &Topic) {
    println!("{}\n", topic.id);
    println!("{}\n", topic.summary);
    print_values("SYNTAX", topic.syntax);
    print_values("EXAMPLES", topic.examples);
    print_values("NOTES", topic.notes);
    if let Some(resolution) = action_resolution(topic) {
        println!("RESOLUTION\n  {resolution}\n");
    }
    if let Some(phases) = action_phases(topic) {
        println!(
            "PHASES\n  {}\n",
            phases
                .iter()
                .map(|phase| phase.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let spellings = topic_dsl_spellings(topic).collect::<Vec<_>>();
    if !spellings.is_empty() {
        println!("ACCEPTED DSL NAMES");
        for spelling in spellings {
            if spelling.aliases.is_empty() {
                println!("  {}", spelling.canonical);
            } else {
                println!(
                    "  {} (aliases: {})",
                    spelling.canonical,
                    spelling.aliases.join(", ")
                );
            }
        }
        println!();
    }
    if !topic.aliases.is_empty() {
        println!("ALIASES\n  {}\n", topic.aliases.join(", "));
    }
    if !topic.related.is_empty() {
        println!("RELATED\n  {}", topic.related.join(", "));
    }
}

fn action_resolution(topic: &Topic) -> Option<&'static str> {
    let family = ActionFamily::from_name(topic.id.strip_prefix("action.")?)?;
    Some(
        if family.resolution() == rsproxy_rules::ResolutionPolicy::Stackable {
            "stackable"
        } else {
            "single: first applicable action wins"
        },
    )
}

fn action_phases(topic: &Topic) -> Option<&'static [rsproxy_rules::Phase]> {
    let family = ActionFamily::from_name(topic.id.strip_prefix("action.")?)?;
    Some(family.phases())
}

fn topic_dsl_spellings(topic: &Topic) -> impl Iterator<Item = &'static RuleSyntaxSpelling> {
    ACTION_SYNTAX
        .iter()
        .chain(CONDITION_SYNTAX)
        .chain(MATCHER_SYNTAX)
        .chain(PROPERTY_SYNTAX)
        .filter(|spelling| spelling.topics.contains(&topic.id))
}

fn print_values(heading: &str, values: &[&str]) {
    if values.is_empty() {
        return;
    }
    println!("{heading}");
    for value in values {
        for line in value.lines() {
            println!("  {line}");
        }
    }
    println!();
}

fn render_index(kind: &str, query: Option<&str>, topics: &[&Topic]) {
    match (kind, query) {
        ("search", Some(query)) => println!("Rule help search: {query}\n"),
        ("category", Some(category)) => println!("Rule help category: {category}\n"),
        _ => {
            println!("rsproxy rule language reference\n");
            println!("Rule: MATCHER ACTION [ACTION ...] [when CONDITION ...] [@PROPERTY ...]");
            println!(
                "Resolution: enabled group order, then line order; @important first; first match wins each single-action family.\n"
            );
        }
    }

    for (category, title) in CATEGORIES {
        let category_topics = topics
            .iter()
            .filter(|topic| topic.category == *category)
            .copied()
            .collect::<Vec<_>>();
        if category_topics.is_empty() {
            continue;
        }
        println!("{} ({})", title.to_ascii_uppercase(), category_topics.len());
        for topic in category_topics {
            println!("  {:<28} {}", topic.id, topic.summary);
        }
        println!();
    }
    println!("Query:  rsproxy rules help TOPIC");
    println!("Search: rsproxy rules help --search 'TERMS'");
    println!("JSON:   rsproxy rules help [TOPIC] --json");
}

#[cfg(test)]
mod tests;
