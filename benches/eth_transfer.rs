//! Benchmark for ETH transfer transactions with varying conflict levels.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use db_test::{Executor, SequentialExecutor, Workload, WorkloadConfig};

/// Benchmarks ETH transfers with different conflict factors.
fn bench_conflict_levels(c: &mut Criterion) {
    let mut group = c.benchmark_group("eth_transfer/conflict_levels");

    let conflict_factors = [0.0, 0.25, 0.5, 0.75, 1.0];
    let num_transactions = 1000;
    let executor = SequentialExecutor::new(true); // With signature verification

    for &conflict_factor in &conflict_factors {
        let config = WorkloadConfig {
            num_accounts: 1000,
            num_transactions,
            conflict_factor,
            seed: 42,
            chain_id: 1,
        };

        // Pre-generate workload (including signing) outside the benchmark loop.
        let workload = Workload::generate(config.clone());

        group.throughput(Throughput::Elements(num_transactions as u64));
        group.bench_with_input(
            BenchmarkId::new("sequential", format!("conflict_{:.0}%", conflict_factor * 100.0)),
            &workload,
            |b, workload| {
                b.iter(|| {
                    let db = workload.create_db();
                    let (_, result) = executor.execute(db, black_box(workload));
                    result.successful
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks ETH transfers with different transaction batch sizes.
fn bench_batch_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("eth_transfer/batch_sizes");

    let batch_sizes = [100, 500, 1000, 5000];
    let executor = SequentialExecutor::new(true);

    for &batch_size in &batch_sizes {
        let config = WorkloadConfig {
            num_accounts: 10_000,
            num_transactions: batch_size,
            conflict_factor: 0.0,
            seed: 42,
            chain_id: 1,
        };

        let workload = Workload::generate(config);

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("sequential", batch_size),
            &workload,
            |b, workload| {
                b.iter(|| {
                    let db = workload.create_db();
                    let (_, result) = executor.execute(db, black_box(workload));
                    result.successful
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks ETH transfers with different account pool sizes.
fn bench_account_pools(c: &mut Criterion) {
    let mut group = c.benchmark_group("eth_transfer/account_pools");

    let account_counts = [100, 1000, 10_000];
    let num_transactions = 1000;
    let executor = SequentialExecutor::new(true);

    for &num_accounts in &account_counts {
        let config = WorkloadConfig {
            num_accounts,
            num_transactions,
            conflict_factor: 0.0,
            seed: 42,
            chain_id: 1,
        };

        let workload = Workload::generate(config);

        group.throughput(Throughput::Elements(num_transactions as u64));
        group.bench_with_input(
            BenchmarkId::new("sequential", num_accounts),
            &workload,
            |b, workload| {
                b.iter(|| {
                    let db = workload.create_db();
                    let (_, result) = executor.execute(db, black_box(workload));
                    result.successful
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks signature verification overhead.
fn bench_signature_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("eth_transfer/signature_verification");

    let num_transactions = 1000;
    let config = WorkloadConfig {
        num_accounts: 1000,
        num_transactions,
        conflict_factor: 0.0,
        seed: 42,
        chain_id: 1,
    };

    let workload = Workload::generate(config);

    // Without signature verification.
    let executor_no_sig = SequentialExecutor::new(false);
    group.throughput(Throughput::Elements(num_transactions as u64));
    group.bench_with_input(
        BenchmarkId::new("sequential", "no_verification"),
        &workload,
        |b, workload| {
            b.iter(|| {
                let db = workload.create_db();
                let (_, result) = executor_no_sig.execute(db, black_box(workload));
                result.successful
            });
        },
    );

    // With signature verification.
    let executor_with_sig = SequentialExecutor::new(true);
    group.bench_with_input(
        BenchmarkId::new("sequential", "with_verification"),
        &workload,
        |b, workload| {
            b.iter(|| {
                let db = workload.create_db();
                let (_, result) = executor_with_sig.execute(db, black_box(workload));
                result.successful
            });
        },
    );

    group.finish();
}

criterion_group!(
    benches,
    bench_conflict_levels,
    bench_batch_sizes,
    bench_account_pools,
    bench_signature_verification
);
criterion_main!(benches);
