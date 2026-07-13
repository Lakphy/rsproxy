use super::*;

#[test]
fn parses_dns_server_lists_and_default_ports() {
    let servers = parse_dns_servers(&[
        "1.1.1.1,[2606:4700:4700::1111]:5353".to_string(),
        "1.1.1.1:53".to_string(),
    ])
    .unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[0], "1.1.1.1:53".parse().unwrap());
    assert_eq!(servers[1], "[2606:4700:4700::1111]:5353".parse().unwrap());
    assert!(
        parse_dns_servers(&["dns.example:53".to_string()])
            .unwrap_err()
            .to_string()
            .contains(
                "invalid --dns-server `dns.example:53`; expected an IP address with optional port"
            )
    );
    assert!(
        parse_dns_servers(&["1.1.1.1,".to_string()])
            .unwrap_err()
            .to_string()
            .contains("--dns-server contains an empty server")
    );
}
