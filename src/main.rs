//! CLI for running database benchmarks.

use db_test::{Executor, SequentialExecutor, Workload, WorkloadConfig};
use std::time::Instant;

fn main() {
    println!("=== REVM Database Benchmark ===\n");

    let configs = [
        ("No conflicts", 0.0),
        ("25% conflicts", 0.25),
        ("50% conflicts", 0.5),
        ("75% conflicts", 0.75),
        ("Full conflicts", 1.0),
    ];

    let num_accounts = 1000;
    let num_transactions = 1000;

    println!(
        "Configuration: {} accounts, {} transactions per run\n",
        num_accounts, num_transactions
    );

    // Test with signature verification.
    println!("--- With Signature Verification ---");
    let executor_with_sig = SequentialExecutor::with_verification(true);
    run_benchmarks(&configs, num_accounts, num_transactions, &executor_with_sig);

    println!("\n--- Without Signature Verification ---");
    let executor_no_sig = SequentialExecutor::with_verification(false);
    run_benchmarks(&configs, num_accounts, num_transactions, &executor_no_sig);
}

fn run_benchmarks(
    configs: &[(&str, f64)],
    num_accounts: usize,
    num_transactions: usize,
    executor: &SequentialExecutor,
) {
    for &(name, conflict_factor) in configs {
        let config = WorkloadConfig {
            num_accounts,
            num_transactions,
            conflict_factor,
            seed: 42,
            chain_id: 1,
        };

        // Generate workload (includes signing).
        let workload = Workload::generate(config);
        let db = workload.create_db();

        // Execute and measure.
        let start = Instant::now();
        let (_, result) = executor.execute(db, &workload);
        let elapsed = start.elapsed();

        let tps = num_transactions as f64 / elapsed.as_secs_f64();

        println!(
            "{:20} | {:5} successful | {:8.2} ms | {:8.0} tx/s",
            name,
            result.successful,
            elapsed.as_secs_f64() * 1000.0,
            tps
        );
    }
}
