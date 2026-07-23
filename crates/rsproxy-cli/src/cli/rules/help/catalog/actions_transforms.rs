use super::super::Topic;

pub(super) const TOPICS: &[Topic] = &[
    topic!(
        "action.url.rewrite",
        "actions",
        "Rewrite the request path/query using a literal or regex pattern.",
        [
            "url.rewrite(FROM, TO)",
            "url.rewrite(/REGEX/[i], REPLACEMENT)"
        ],
        ["example.test url.rewrite(/old, /new)"],
        [
            "Regex replacements preserve $1 and ${name} for the replacement engine.",
            "Only path and query are rewritten; use map.remote to replace the origin."
        ],
        ["path-rewrite", "url-rewrite"],
        ["action.map.remote", "action.url.query"]
    ),
    topic!(
        "action.url.query",
        "actions",
        "Apply ordered query additions, updates, and removals.",
        ["url.query(NAME=VALUE[, -NAME ...])"],
        ["example.test url.query(debug=1, source=${host}, -token)"],
        [
            "Every duplicate occurrence of a removed name is deleted.",
            "This family stacks in source order."
        ],
        ["query", "query-params", "url-params"],
        ["action.delete", "concept.values"]
    ),
    topic!(
        "action.delete",
        "actions",
        "Delete typed URL, header, cookie, body, Content-Type, or trailer properties.",
        [
            "delete(PROPERTY[, PROPERTY...])",
            "pathname | pathname.INDEX | pathname.first | pathname.last",
            "urlParams[.NAME]",
            "reqHeaders.NAME | resHeaders.NAME | headers.NAME",
            "reqCookies[.NAME] | resCookies[.NAME] | cookies[.NAME]",
            "reqBody[.PATH] | resBody[.PATH] | body",
            "reqType | resType | reqCharset | resCharset",
            "trailer[.NAME] | trailers"
        ],
        [
            "example.test delete(urlParams.token, reqHeaders.x-secret, reqBody.profile.secret, resBody.meta.debug)"
        ],
        [
            "Nested request deletion supports JSON and forms; response deletion supports JSON and JSONP.",
            "Body deletion is conservative for incompatible content, encodings, missing paths, or size overflow.",
            "Backslash escapes literal path separators and special characters.",
            "Compatibility aliases include url.params/params/query, req.header/reqH, res.header/resH, req.cookie/reqC, res.cookie/resC, req.body/res.body, and combined headers/cookies/body targets."
        ],
        ["delete-property", "remove-property"],
        ["concept.limits", "action.url.query"]
    ),
    topic!(
        "action.req.body.set",
        "actions",
        "Replace the complete buffered request body.",
        ["req.body.set(VALUE)"],
        ["example.test req.body.set({\"debug\":true})"],
        ["Body framing is updated; over-limit bodies follow conservative planner behavior."],
        ["request-body-set"],
        ["concept.values", "concept.limits"]
    ),
    topic!(
        "action.req.body.prepend",
        "actions",
        "Prepend bytes to the buffered request body.",
        ["req.body.prepend(VALUE)"],
        ["example.test req.body.prepend(prefix-)"],
        ["Binary file/reference values are preserved."],
        ["request-body-prepend"],
        ["concept.values"]
    ),
    topic!(
        "action.req.body.append",
        "actions",
        "Append bytes to the buffered request body.",
        ["req.body.append(VALUE)"],
        ["example.test req.body.append(-suffix)"],
        ["Binary file/reference values are preserved."],
        ["request-body-append"],
        ["concept.values"]
    ),
    topic!(
        "action.req.body.replace",
        "actions",
        "Regex-replace UTF-8 request-body content.",
        ["req.body.replace(/REGEX/[i], REPLACEMENT)"],
        ["example.test req.body.replace(/item-(\\d+)/, item=$1)"],
        ["Invalid UTF-8, non-identity encoding, or over-limit bodies are preserved."],
        ["request-body-replace"],
        ["matcher.regex", "concept.limits"]
    ),
    topic!(
        "action.res.body.set",
        "actions",
        "Replace the complete buffered response body.",
        ["res.body.set(VALUE)"],
        ["example.test res.body.set({\"mocked\":true})"],
        ["Body framing is updated; over-limit bodies follow conservative planner behavior."],
        ["response-body-set"],
        ["concept.values", "concept.limits"]
    ),
    topic!(
        "action.res.body.prepend",
        "actions",
        "Prepend bytes to the buffered response body.",
        ["res.body.prepend(VALUE)"],
        ["example.test res.body.prepend(prefix-)"],
        ["Binary file/reference values are preserved."],
        ["response-body-prepend"],
        ["concept.values"]
    ),
    topic!(
        "action.res.body.append",
        "actions",
        "Append bytes to the buffered response body.",
        ["res.body.append(VALUE)"],
        ["example.test res.body.append(-suffix)"],
        ["Binary file/reference values are preserved."],
        ["response-body-append"],
        ["concept.values"]
    ),
    topic!(
        "action.res.body.replace",
        "actions",
        "Regex-replace UTF-8 response-body content.",
        ["res.body.replace(/REGEX/[i], REPLACEMENT)"],
        ["example.test res.body.replace(/raw/i, rendered)"],
        ["Invalid UTF-8, non-identity encoding, or over-limit bodies are preserved."],
        ["response-body-replace"],
        ["matcher.regex", "concept.limits"]
    ),
    topic!(
        "action.inject",
        "actions",
        "Content-Type-gated HTML, JavaScript, or CSS response injection.",
        ["inject(html|js|css, VALUE[, append|prepend|replace])"],
        ["example.test inject(html, \"<!-- debug -->\", append)"],
        [
            "Mode defaults to append.",
            "Incompatible, encoded, or over-limit bodies are preserved."
        ],
        ["html-inject", "js-inject", "css-inject"],
        ["concept.values", "concept.limits"]
    ),
];
