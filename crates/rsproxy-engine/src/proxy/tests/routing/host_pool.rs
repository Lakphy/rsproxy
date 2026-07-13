use super::*;
use std::collections::BTreeMap;

#[test]
fn host_pool_routes_requests_in_round_robin_order() {
    let request = meta("http://origin.test/path");
    let rules = RuleSet::parse(
        "default",
        "origin.test host(127.0.0.1:18081, 127.0.0.1:18082, 127.0.0.1:18083)",
    )
    .unwrap();
    let url = UrlParts::parse(&request.url).unwrap();

    for expected_port in [18081, 18082, 18083, 18081] {
        let actions = rules.resolve(&request).actions;
        let expected = UpstreamRoute::Direct {
            host: "127.0.0.1".to_string(),
            port: expected_port,
        };
        assert_eq!(
            test_planned_upstream_addr(&request.url, &actions, &request),
            Some(format!("127.0.0.1:{expected_port}"))
        );
        assert_eq!(test_upstream_route(&url, &actions, &request), expected);
        assert_eq!(test_upstream_route(&url, &actions, &request), expected);
    }
}

#[test]
fn host_pool_is_balanced_under_concurrent_selection() {
    let rules = RuleSet::parse(
        "default",
        "origin.test host(a.test:80, b.test:80, c.test:80)",
    )
    .unwrap();
    let Action::Host(pool) = &rules.rules[0].actions[0] else {
        panic!("expected host action");
    };
    let pool = pool.clone();

    let selected = thread::scope(|scope| {
        let handles = (0..12)
            .map(|_| {
                let pool = pool.clone();
                scope.spawn(move || {
                    (0..30)
                        .map(|_| {
                            pool.clone()
                                .selected_address()
                                .as_inline()
                                .unwrap()
                                .to_string()
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    let counts = selected
        .into_iter()
        .fold(BTreeMap::new(), |mut counts, address| {
            *counts.entry(address).or_insert(0usize) += 1;
            counts
        });

    assert_eq!(counts.get("a.test:80"), Some(&120));
    assert_eq!(counts.get("b.test:80"), Some(&120));
    assert_eq!(counts.get("c.test:80"), Some(&120));
}
