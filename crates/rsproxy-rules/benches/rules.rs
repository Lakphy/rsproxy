use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use rsproxy_rules::{RequestMeta, RuleSet};
use std::fmt::Write as _;
use std::time::Duration;

const SIZES: [usize; 3] = [100, 1_000, 10_000];

fn source(rule_count: usize) -> String {
    let mut rules = String::with_capacity(rule_count * 72);
    for index in 0..rule_count {
        if index % 5 == 0 {
            writeln!(
                rules,
                r"/^http:\/\/bench-{index}\.example\.test\/api\/[0-9]+$/ status(200)"
            )
            .unwrap();
        } else {
            writeln!(
                rules,
                "bench-{index}.example.test/api status(200) when method(GET)"
            )
            .unwrap();
        }
    }
    rules
}

fn request(rule_count: usize) -> RequestMeta {
    let index = rule_count.saturating_sub(5).div_ceil(5) * 5;
    RequestMeta {
        method: "GET".to_string(),
        url: format!("http://bench-{index}.example.test/api/42"),
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: Default::default(),
    }
}

fn parse_benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("rules_parse");
    group.sample_size(20);
    for size in SIZES {
        let rules = source(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &rules,
            |bencher, rules| {
                bencher.iter(|| RuleSet::parse("criterion", black_box(rules)).unwrap());
            },
        );
    }
    group.finish();
}

fn resolve_benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("rules_resolve_mixed");
    for size in SIZES {
        let rules = RuleSet::parse("criterion", &source(size)).unwrap();
        let request = request(size);
        assert_eq!(rules.stats().rules, size);
        assert_eq!(rules.resolve(&request).actions.len(), 1);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |bencher, _| {
            bencher.iter(|| rules.resolve(black_box(&request)));
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(3))
        .warm_up_time(Duration::from_secs(1));
    targets = parse_benchmarks, resolve_benchmarks
}
criterion_main!(benches);
