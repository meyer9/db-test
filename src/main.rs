//! Comprehensive benchmark runner for all executor backends.

use db_test::{Executor, SequentialExecutor, Workload, WorkloadConfig};
use std::time::Instant;

#[cfg(feature = "mdbx")]
use db_test::executor::{MdbxBatchedExecutor, MdbxSequentialExecutor};
#[cfg(feature = "mdbx")]
use tempfile::tempdir;

#[cfg(feature = "fdb")]
use db_test::executor::FdbParallelExecutor;
#[cfg(feature = "fdb")]
use tokio;
#[cfg(feature = "fdb")]
use foundationdb;

/// Configuration for a single benchmark run.
struct BenchmarkConfig {
    name: &'static str,
    conflict_factor: f64,
}

/// Results from a single benchmark run.
#[derive(Debug)]
struct BenchmarkResult {
    config_name: &'static str,
    executor_name: &'static str,
    preserves_order: bool,
    successful: usize,
    failed: usize,
    duration_ms: f64,
    throughput_tps: f64,
}

impl BenchmarkResult {
    fn print_header() {
        println!(
            "{:<20} | {:<20} | {:<8} | {:<10} | {:<10} | {:<12} | {:<12}",
            "Config", "Executor", "Ordering", "Successful", "Failed", "Time (ms)", "TPS"
        );
        println!("{}", "-".repeat(115));
    }

    fn print(&self) {
        println!(
            "{:<20} | {:<20} | {:<8} | {:<10} | {:<10} | {:<12.2} | {:<12.0}",
            self.config_name,
            self.executor_name,
            if self.preserves_order { "strict" } else { "loose" },
            self.successful,
            self.failed,
            self.duration_ms,
            self.throughput_tps,
        );
    }
}

fn main() {
    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                              REVM Database Benchmark Suite                                               ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════════════════════════════════╝");
    println!();

    let configs = vec![
        BenchmarkConfig {
            name: "No conflicts",
            conflict_factor: 0.0,
        },
        BenchmarkConfig {
            name: "25% conflicts",
            conflict_factor: 0.25,
        },
        BenchmarkConfig {
            name: "50% conflicts",
            conflict_factor: 0.5,
        },
        BenchmarkConfig {
            name: "75% conflicts",
            conflict_factor: 0.75,
        },
        BenchmarkConfig {
            name: "Full conflicts",
            conflict_factor: 1.0,
        },
    ];

    // Realistic blockchain parameters (scaled down for faster benchmarks)
    let num_accounts = 50_000;           // Large account pool
    let num_transactions = 2_500;        // 4 blocks at 625 tx/block
    let transactions_per_block = 625;    // Mid-range of 2k-20k typical in real blockchains (scaled)

    println!("Benchmark Configuration:");
    println!("  • Accounts: {}", num_accounts);
    println!("  • Transactions per run: {}", num_transactions);
    println!("  • Transactions per block: {}", transactions_per_block);
    println!("  • Number of blocks: {}", num_transactions / transactions_per_block);
    println!("  • Signature verification: enabled");
    println!();

    let mut all_results = Vec::new();

    // Run benchmarks for in-memory executor
    println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!("  In-Memory Executor (CacheDB)");
    println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!();
    BenchmarkResult::print_header();

    for config in &configs {
        let workload_config = WorkloadConfig {
            num_accounts,
            num_transactions,
            conflict_factor: config.conflict_factor,
            seed: 42,
            chain_id: 1,
            transactions_per_block,
        };

        let workload = Workload::generate(workload_config);
        let db = workload.create_db();
        let executor = SequentialExecutor::with_verification(true);

        let start = Instant::now();
        let (_, result) = executor.execute(db, &workload);
        let elapsed = start.elapsed();

        let bench_result = BenchmarkResult {
            config_name: config.name,
            executor_name: executor.name(),
            preserves_order: executor.preserves_order(),
            successful: result.successful,
            failed: result.failed,
            duration_ms: elapsed.as_secs_f64() * 1000.0,
            throughput_tps: num_transactions as f64 / elapsed.as_secs_f64(),
        };

        bench_result.print();
        all_results.push(bench_result);
    }

    println!();

    // Run benchmarks for MDBX executor if feature is enabled
    #[cfg(feature = "mdbx")]
    {
        println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!("  MDBX Persistent Storage Executor");
        println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!();
        BenchmarkResult::print_header();

        for config in &configs {
            let workload_config = WorkloadConfig {
                num_accounts,
                num_transactions,
                conflict_factor: config.conflict_factor,
                seed: 42,
                chain_id: 1,
                transactions_per_block,
            };

            let workload = Workload::generate(workload_config);

            // Create temporary directory for each run
            let dir = tempdir().expect("Failed to create temp directory");
            let executor = MdbxSequentialExecutor::new(dir.path(), true)
                .expect("Failed to create MDBX executor");

            let start = Instant::now();
            let (result, _) = executor
                .execute_workload(&workload)
                .expect("Execution failed");
            let elapsed = start.elapsed();

            let bench_result = BenchmarkResult {
                config_name: config.name,
                executor_name: executor.name(),
                preserves_order: executor.preserves_order(),
                successful: result.successful,
                failed: result.failed,
                duration_ms: elapsed.as_secs_f64() * 1000.0,
                throughput_tps: num_transactions as f64 / elapsed.as_secs_f64(),
            };

            bench_result.print();
            all_results.push(bench_result);
        }

        println!();

        // Run benchmarks for MDBX batched executor
        println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!("  MDBX Batched Executor (Block-level caching and commit)");
        println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!();
        BenchmarkResult::print_header();

        for config in &configs {
            let workload_config = WorkloadConfig {
                num_accounts,
                num_transactions,
                conflict_factor: config.conflict_factor,
                seed: 42,
                chain_id: 1,
                transactions_per_block,
            };

            let workload = Workload::generate(workload_config);

            // Create temporary directory for each run
            let dir = tempdir().expect("Failed to create temp directory");
            let executor = MdbxBatchedExecutor::new(dir.path(), true)
                .expect("Failed to create MDBX batched executor");

            let start = Instant::now();
            let (result, _) = executor
                .execute_workload(&workload)
                .expect("Execution failed");
            let elapsed = start.elapsed();

            let bench_result = BenchmarkResult {
                config_name: config.name,
                executor_name: executor.name(),
                preserves_order: executor.preserves_order(),
                successful: result.total_successful,
                failed: result.total_failed,
                duration_ms: elapsed.as_secs_f64() * 1000.0,
                throughput_tps: num_transactions as f64 / elapsed.as_secs_f64(),
            };

            bench_result.print();
            all_results.push(bench_result);
        }

        println!();
    }

    #[cfg(not(feature = "mdbx"))]
    {
        println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!("  MDBX executor not available (enable with --features mdbx)");
        println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!();
    }

    // Run benchmarks for FoundationDB executor if feature is enabled
    #[cfg(feature = "fdb")]
    {
        println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!("  FoundationDB Parallel Executor (Multi-threaded with automatic retry)");
        println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!();

        // Initialize FDB network once (required before any FDB operations)
        let _fdb_network = unsafe { foundationdb::boot() };
        
        // Give network time to start
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Test with different thread counts
        let thread_counts = [1, 2, 4, 8];
        
        for &num_threads in &thread_counts {
            println!("--- {} threads ---", num_threads);
            BenchmarkResult::print_header();

            for config in &configs {
                let workload_config = WorkloadConfig {
                    num_accounts,
                    num_transactions,
                    conflict_factor: config.conflict_factor,
                    seed: 42,
                    chain_id: 1,
                    transactions_per_block,
                };

                let workload = Workload::generate(workload_config);

                // Create FDB executor - needs async
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                let result = rt.block_on(async {
                    let executor = FdbParallelExecutor::new(num_threads, true)
                        .await
                        .expect("Failed to create FDB executor");

                    let start = Instant::now();
                    let result = executor
                        .execute_workload(&workload)
                        .await
                        .expect("Execution failed");
                    let elapsed = start.elapsed();

                    BenchmarkResult {
                        config_name: config.name,
                        executor_name: executor.name(),
                        preserves_order: executor.preserves_order(),
                        successful: result.total_successful,
                        failed: result.total_failed,
                        duration_ms: elapsed.as_secs_f64() * 1000.0,
                        throughput_tps: num_transactions as f64 / elapsed.as_secs_f64(),
                    }
                });

                result.print();
                all_results.push(result);
            }

            println!();
        }
    }

    #[cfg(not(feature = "fdb"))]
    {
        println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!("  FoundationDB executor not available (enable with --features fdb)");
        println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
        println!();
    }

    // Print summary statistics
    println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!("  Summary Statistics");
    println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!();

    // Group by executor
    let mut by_executor: std::collections::HashMap<&str, Vec<&BenchmarkResult>> =
        std::collections::HashMap::new();

    for result in &all_results {
        by_executor
            .entry(result.executor_name)
            .or_insert_with(Vec::new)
            .push(result);
    }

    for (executor_name, results) in by_executor.iter() {
        let avg_tps: f64 = results.iter().map(|r| r.throughput_tps).sum::<f64>() / results.len() as f64;
        let min_tps = results
            .iter()
            .map(|r| r.throughput_tps)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0);
        let max_tps = results
            .iter()
            .map(|r| r.throughput_tps)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0);
        let total_success: usize = results.iter().map(|r| r.successful).sum();
        let total_failed: usize = results.iter().map(|r| r.failed).sum();

        println!("{}:", executor_name);
        println!("  • Average throughput: {:.0} tx/s", avg_tps);
        println!("  • Min throughput: {:.0} tx/s", min_tps);
        println!("  • Max throughput: {:.0} tx/s", max_tps);
        println!("  • Total successful: {}", total_success);
        println!("  • Total failed: {}", total_failed);
        println!("  • Preserves order: {}", results[0].preserves_order);
        println!();
    }

    println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!("Benchmark complete!");
    println!("  • Run with --features mdbx to include MDBX benchmarks");
    println!("  • Run with --features fdb to include FoundationDB benchmarks");
    println!("  • Run with --all-features to include all backends");
    println!("════════════════════════════════════════════════════════════════════════════════════════════════════════════");
}
