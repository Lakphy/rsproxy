use super::*;

fn req(url: &str) -> RequestMeta {
    RequestMeta {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: vec![("accept".to_string(), "text/plain".to_string())],
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: TemplateMetadata::default(),
    }
}

mod actions;
mod body_planning;
mod conditions;
mod errors;
mod explain_matrix;
mod groups;
mod index;
mod language;
mod lint;
mod matching_edges;
mod migration;
mod model_edges;
mod parser_edges;
mod regex;
mod semantic_lint;
mod snapshot_limits;
mod template_edges;
mod templates;
