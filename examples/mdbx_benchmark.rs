//! Example: MDBX database benchmark
//!
//! This example demonstrates using the MDBX-backed sequential executor
//! for persistent storage benchmarking.
//!
//! Run with: cargo run --example mdbx_benchmark --features mdbx

use db_test::{Workload, WorkloadConfig};
use std::time::Instant;
use tempfile::tempdir;

#[cfg(feature = "mdbx")]
use db_test::executor::MdbxSequentialExecutor;

fn main() {
    #[cfg(not(feature = "mdbx"))]
    {
        eprintln!("This example requires the 'mdbx' feature to be enabled.");
        eprintln!("Run with: cargo run --example mdbx_benchmark --features mdbx");
        std::process::exit(1);
    }

    #[cfg(feature = "mdbx")]
    {
        println!("=== MDBX Database Benchmark ===\n");

        let configs = [
            ("No conflicts", 0.0),
            ("25% conflicts", 0.25),
            ("50% conflicts", 0.5),
            ("75% conflicts", 0.75),
            ("Full conflicts", 1.0),
        ];

        let num_accounts = 100;
        let num_transactions = 100;

        println!(
            "Configuration: {} accounts, {} transactions per run\n",
            num_accounts, num_transactions
        );

        for (name, conflict_factor) in configs {
            let config = WorkloadConfig {
                num_accounts,
                num_transactions,
                conflict_factor,
                seed: 42,
                chain_id: 1,
            };

            let workload = Workload::generate(config);

            // Create temporary directory for database
            let dir = tempdir().unwrap();

            // Create MDBX executor with signature verification
            let executor = MdbxSequentialExecutor::new(dir.path(), true)
                .expect("Failed to create MDBX executor");

            let start = Instant::now();
            let (result, _) = executor
                .execute_workload(&workload)
                .expect("Execution failed");
            let elapsed = start.elapsed();

            let tps = num_transactions as f64 / elapsed.as_secs_f64();

            println!(
                "{:20} | {:5} successful | {:8.2} ms | {:8.0} tx/s | ordering: {}",
                name,
                result.successful,
                elapsed.as_secs_f64() * 1000.0,
                tps,
                if executor.preserves_order() {
                    "strict"
                } else {
                    "loose"
                }
            );
        }
    }
}

