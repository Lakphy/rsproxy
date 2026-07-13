use super::*;

fn test_upstream_route(
    url: &UrlParts,
    actions: &[ResolvedAction],
    meta: &RequestMeta,
) -> UpstreamRoute {
    upstream_route(url, actions, meta, &test_state()).unwrap()
}

fn test_planned_upstream_addr(
    full_url: &str,
    actions: &[ResolvedAction],
    meta: &RequestMeta,
) -> Option<String> {
    planned_upstream_addr(full_url, actions, meta, &test_state())
}

mod chains;
mod host_pool;
mod single_hop;
