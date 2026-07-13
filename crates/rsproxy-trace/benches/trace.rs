//! Criterion benchmarks for trace ingestion and bounded storage.

use criterion::{Criterion, Throughput, black_box};
use rsproxy_trace::{TraceEvent, TraceStore, TraceStoreConfig};
use std::time::Duration;

fn enqueue_benchmark(criterion: &mut Criterion) {
    let store = TraceStore::new_with_config(TraceStoreConfig {
        max_sessions: 4_096,
        queue_capacity: 65_536,
        memory_budget_bytes: 256 * 1024 * 1024,
        queue_memory_budget_bytes: Some(64 * 1024 * 1024),
        body_limit: 0,
        spill: None,
    });
    let mut id = 1u64;
    let mut group = criterion.benchmark_group("trace_enqueue");
    group.throughput(Throughput::Elements(1));
    group.bench_function("abort_event", |bencher| {
        bencher.iter(|| {
            id = id.wrapping_add(1);
            black_box(store.emit(TraceEvent::Abort { id }))
        });
    });
    group.finish();
}

fn benches() {
    let mut criterion = Criterion::default()
        .measurement_time(Duration::from_secs(3))
        .warm_up_time(Duration::from_secs(1))
        .configure_from_args();
    enqueue_benchmark(&mut criterion);
}

fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
}
