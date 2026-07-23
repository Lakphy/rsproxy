use super::*;

#[test]
fn migration_adds_header_and_rewrites_only_call_aliases() {
    let source = concat!(
        "# retained\n",
        "/clientIp\\(/ mockRaw(\"clientIp(value)\") ",
        "when any(ip(127.*), resHeader(x-id)) # keep clientIp(value)\n"
    );
    assert_eq!(
        migrate_rule_source_v3(source),
        concat!(
            "# retained\n",
            "@language 3\n",
            "/clientIp\\(/ mock.raw(\"clientIp(value)\") ",
            "when any(client.ip(127.*), res.header(x-id)) # keep clientIp(value)\n"
        )
    );
}

#[test]
fn migration_replaces_an_existing_language_directive() {
    assert_eq!(
        migrate_rule_source_v3("  @language 2  # retain this note\nexample.test direct\n"),
        "  @language 3  # retain this note\nexample.test direct\n"
    );
}
