//! Criterion benchmarks for MITM leaf issuance and certificate-cache hot paths.
//!
//! Fixtures use an isolated process-specific directory and remove it when the
//! benchmark ends, keeping certificate generation costs separate from setup I/O.

use criterion::{Criterion, Throughput, black_box};
use rsproxy_engine::benchmark_support::{CertificateFixture, fixture_path};
use std::time::Duration;

fn certificate_benchmarks(criterion: &mut Criterion) {
    let fixture = CertificateFixture::create(fixture_path("certificates"))
        .expect("create isolated certificate benchmark fixture");
    let cached_host = "cached.example.test";
    fixture
        .ensure_leaf(cached_host)
        .expect("seed cached benchmark leaf certificate");
    let mut server_cache = fixture
        .cached_server_config(cached_host)
        .expect("build cached benchmark TLS configuration");
    let mut serial = 0u64;

    let mut group = criterion.benchmark_group("mitm_certificate");
    group.sample_size(20);
    group.throughput(Throughput::Elements(1));
    group.bench_function("issue_leaf", |bencher| {
        bencher.iter(|| {
            serial = serial.wrapping_add(1);
            fixture
                .issue_leaf(black_box(&format!("issued-{serial}.example.test")))
                .expect("issue benchmark leaf certificate")
        });
    });
    group.bench_function("disk_cache_hit", |bencher| {
        bencher.iter(|| {
            fixture
                .ensure_leaf(black_box(cached_host))
                .expect("read cached benchmark leaf certificate")
        });
    });
    group.bench_function("server_config_cache_hit", |bencher| {
        bencher.iter(|| assert!(black_box(server_cache.lookup())));
    });
    group.bench_function("cached_tls_handshake", |bencher| {
        bencher.iter(|| {
            server_cache
                .handshake()
                .expect("cached in-memory TLS handshake must succeed")
        });
    });
    group.finish();
}

fn benches() {
    let mut criterion: Criterion<_> = Criterion::default()
        .measurement_time(Duration::from_secs(3))
        .warm_up_time(Duration::from_secs(1))
        .configure_from_args();
    certificate_benchmarks(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
