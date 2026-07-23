use super::super::Topic;

pub(super) const TOPICS: &[Topic] = &[
    topic!(
        "action.delay.req",
        "actions",
        "Delay before upstream request forwarding.",
        ["delay(req, DURATION)"],
        ["example.test delay(req, 50ms)"],
        ["Duration accepts milliseconds, ms, or fractional seconds with s."],
        ["request-delay", "req-delay"],
        ["action.delay.res"]
    ),
    topic!(
        "action.delay.res",
        "actions",
        "Delay before downstream response forwarding.",
        ["delay(res, DURATION)"],
        ["example.test delay(res, 1.5s)"],
        ["Duration accepts milliseconds, ms, or fractional seconds with s."],
        ["response-delay", "res-delay"],
        ["action.delay.req"]
    ),
    topic!(
        "action.throttle.req",
        "actions",
        "Pace request-body writes under the absolute request deadline.",
        ["throttle(req, SPEED)"],
        ["example.test throttle(req, 64KB/s)"],
        ["Speed accepts bytes, KB/K, or MB/M per second and must be greater than zero."],
        ["request-throttle", "req-speed"],
        ["action.throttle.res"]
    ),
    topic!(
        "action.throttle.res",
        "actions",
        "Pace response-body writes under the absolute request deadline.",
        ["throttle(res, SPEED)"],
        ["example.test throttle(res, 1MB/s)"],
        ["Pacing is preserved across frames; speed must be greater than zero."],
        ["response-throttle", "res-speed"],
        ["action.throttle.req"]
    ),
    topic!(
        "action.bypass",
        "actions",
        "Keep matching CONNECT tunnels in passthrough mode.",
        ["bypass"],
        ["example.test bypass"],
        ["This disables MITM for matching traffic without changing global proxy settings."],
        ["passthrough", "no-mitm"],
        ["action.direct"]
    ),
    topic!(
        "action.hide",
        "actions",
        "Suppress trace recording while retaining all other matched actions.",
        ["hide"],
        ["example.test hide"],
        ["Use for sensitive or noisy traffic."],
        ["hide-trace", "no-trace"],
        ["action.tag"]
    ),
    topic!(
        "action.tag",
        "actions",
        "Add a rendered tag flag to the captured session.",
        ["tag(VALUE)"],
        ["example.test tag(api:${path})"],
        [
            "Tag actions stack; runtime flags use the tag: prefix.",
            "This differs from source metadata @tag:NAME."
        ],
        ["trace-tag", "runtime-tag"],
        ["property.tag", "concept.templates"]
    ),
    topic!(
        "action.skip",
        "actions",
        "Suppress later action families or all later actions.",
        ["skip(FAMILY[, FAMILY...])", "skip() | skip(all) | skip(*)"],
        ["example.test skip(res.header, res.body)"],
        [
            "Family names normalize case, underscores, and hyphens.",
            "Skipping a parent such as res.body covers its child families."
        ],
        ["skip-family", "stop-actions"],
        ["concept.ordering", "action.direct"]
    ),
    topic!(
        "property.important",
        "properties",
        "Move a rule ahead of every ordinary rule across enabled groups.",
        ["@important"],
        ["example.test status(204) @important"],
        [
            "Relative order remains stable within the important partition.",
            "Prefer explicit source ordering when practical."
        ],
        ["important", "priority"],
        ["concept.ordering"]
    ),
    topic!(
        "property.disabled",
        "properties",
        "Retain a source rule for inspection while excluding it from resolution.",
        ["@disabled"],
        ["example.test status(204) @disabled"],
        ["The rule remains in snapshot statistics."],
        ["disabled", "disable-rule"],
        ["concept.groups"]
    ),
    topic!(
        "property.tag",
        "properties",
        "Attach source metadata to a parsed rule.",
        ["@tag:NAME"],
        ["example.test status(204) @tag:health"],
        [
            "This metadata is retained on Rule.tags and is distinct from the runtime tag(VALUE) action."
        ],
        ["source-tag", "metadata-tag"],
        ["action.tag"]
    ),
];
