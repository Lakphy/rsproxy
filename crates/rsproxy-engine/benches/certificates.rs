use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use rsproxy_engine::benchmark_support::{CertificateFixture, fixture_path};
use std::time::Duration;

fn certificate_benchmarks(criterion: &mut Criterion) {
    let fixture = CertificateFixture::create(fixture_path("certificates")).unwrap();
    let cached_host = "cached.example.test";
    fixture.ensure_leaf(cached_host).unwrap();
    let mut server_cache = fixture.cached_server_config(cached_host).unwrap();
    let mut serial = 0u64;

    let mut group = criterion.benchmark_group("mitm_certificate");
    group.sample_size(20);
    group.throughput(Throughput::Elements(1));
    group.bench_function("issue_leaf", |bencher| {
        bencher.iter(|| {
            serial = serial.wrapping_add(1);
            fixture
                .issue_leaf(black_box(&format!("issued-{serial}.example.test")))
                .unwrap()
        });
    });
    group.bench_function("disk_cache_hit", |bencher| {
        bencher.iter(|| fixture.ensure_leaf(black_box(cached_host)).unwrap());
    });
    group.bench_function("server_config_cache_hit", |bencher| {
        bencher.iter(|| assert!(black_box(server_cache.lookup())));
    });
    group.bench_function("cached_tls_handshake", |bencher| {
        bencher.iter(|| server_cache.handshake().unwrap());
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(3))
        .warm_up_time(Duration::from_secs(1));
    targets = certificate_benchmarks
}
criterion_main!(benches);
