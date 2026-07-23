use super::Topic;

mod actions_control;
mod actions_metadata;
mod actions_routing;
mod actions_transforms;
mod concepts;
mod matchers_conditions;

pub(super) const TOPIC_GROUPS: &[&[Topic]] = &[
    concepts::TOPICS,
    matchers_conditions::TOPICS,
    actions_routing::TOPICS,
    actions_metadata::TOPICS,
    actions_transforms::TOPICS,
    actions_control::TOPICS,
];
