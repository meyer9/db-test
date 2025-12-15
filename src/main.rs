//! Comprehensive benchmark runner for all executor backends.

use clap::Parser;
use db_test::{Executor, SequentialExecutor, Workload, WorkloadConfig};
use std::time::Instant;

#[cfg(feature = "mdbx")]
use db_test::executor::{MdbxBatchedExecutor, MdbxSequentialExecutor};
#[cfg(feature = "mdbx")]
use tempfile::tempdir;

#[cfg(feature = "fdb")]
use db_test::executor::FdbParallelExecutor;

#[cfg(feature = "block-stm")]
use db_test::BlockStmExecutor;

/// Benchmark runner for REVM database implementations
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Number of accounts in the system
    #[arg(short = 'a', long, default_value_t = 50_000)]
    num_accounts: usize,

    /// Total number of transactions to execute
    #[arg(short = 't', long, default_value_t = 2_500)]
    num_transactions: usize,

    /// Number of transactions per block
    #[arg(short = 'b', long, default_value_t = 625)]
    transactions_per_block: usize,

    /// Conflict factors to test (comma-separated, e.g., "0.0,0.25,0.5,0.75,1.0")
    #[arg(short = 'c', long, value_delimiter = ',', default_values_t = vec![0.0, 0.25, 0.5, 0.75, 1.0])]
    conflicts: Vec<f64>,

    /// Thread counts to test for parallel executors (comma-separated)
    #[arg(long, value_delimiter = ',', default_values_t = vec![1, 2, 4, 8])]
    threads: Vec<usize>,

    /// Enable sequential in-memory executor
    #[arg(long, default_value_t = true)]
    sequential: bool,

    /// Enable MDBX sequential executor (requires --features mdbx)
    #[arg(long, default_value_t = false)]
    mdbx_sequential: bool,

    /// Enable MDBX batched executor (requires --features mdbx)
    #[arg(long, default_value_t = false)]
    mdbx_batched: bool,

    /// Enable FoundationDB parallel executor (requires --features fdb)
    #[arg(long, default_value_t = false)]
    fdb: bool,

    /// Enable Block-STM parallel executor (requires --features block-stm)
    #[arg(long, default_value_t = false)]
    block_stm: bool,

    /// Enable all available executors
    #[arg(long, default_value_t = false)]
    all: bool,

    /// Disable signature verification (faster but less realistic)
    #[arg(long, default_value_t = false)]
    no_verify: bool,
}

/// Results from a single benchmark run.
#[derive(Debug, Clone)]
struct BenchmarkResult {
    conflict_name: String,
    executor_name: String,
    preserves_order: bool,
    successful: usize,
    failed: usize,
    duration_ms: f64,
    throughput_tps: f64,
}

impl BenchmarkResult {
    fn print_header() {
        println!(
            "{:<20} | {:<25} | {:<8} | {:<10} | {:<10} | {:<12} | {:<12}",
            "Conflict", "Executor", "Ordering", "Successful", "Failed", "Time (ms)", "TPS"
        );
        println!("{}", "-".repeat(120));
    }

    fn print(&self) {
        println!(
            "{:<20} | {:<25} | {:<8} | {:<10} | {:<10} | {:<12.2} | {:<12.0}",
            self.conflict_name,
            self.executor_name,
            if self.preserves_order { "strict" } else { "loose" },
            self.successful,
            self.failed,
            self.duration_ms,
            self.throughput_tps,
        );
    }
}

/// Generic benchmark runner for in-memory executors
fn run_in_memory_benchmark<E>(
    executor: &E,
    workload: &Workload,
    conflict_name: &str,
    num_transactions: usize,
) -> BenchmarkResult
where
    E: Executor<Database = revm::database::CacheDB<revm::database::EmptyDB>>,
{
    let db = workload.create_db();
    
    let start = Instant::now();
    let (_, result) = executor.execute(db, workload);
    let elapsed = start.elapsed();

    BenchmarkResult {
        conflict_name: conflict_name.to_string(),
        executor_name: executor.name().to_string(),
        preserves_order: executor.preserves_order(),
        successful: result.successful,
        failed: result.failed,
        duration_ms: elapsed.as_secs_f64() * 1000.0,
        throughput_tps: num_transactions as f64 / elapsed.as_secs_f64(),
    }
}

fn print_section_header(title: &str) {
    println!("═══════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!("  {}", title);
    println!("═══════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!();
}

fn print_summary(results: &[BenchmarkResult]) {
    if results.is_empty() {
        return;
    }

    println!();
    println!("═══════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!("  Summary Statistics");
    println!("═══════════════════════════════════════════════════════════════════════════════════════════════════════");
    println!();

    // Group by executor
    let mut executor_groups: std::collections::HashMap<String, Vec<&BenchmarkResult>> =
        std::collections::HashMap::new();

    for result in results {
        executor_groups
            .entry(result.executor_name.clone())
            .or_default()
            .push(result);
    }

    println!("{:<30} | {:<15} | {:<15} | {:<15}", "Executor", "Avg TPS", "Min TPS", "Max TPS");
    println!("{}", "-".repeat(80));

    for (executor_name, results) in executor_groups.iter() {
        let avg_tps: f64 = results.iter().map(|r| r.throughput_tps).sum::<f64>() / results.len() as f64;
        let min_tps = results.iter().map(|r| r.throughput_tps).fold(f64::INFINITY, f64::min);
        let max_tps = results.iter().map(|r| r.throughput_tps).fold(f64::NEG_INFINITY, f64::max);

        println!("{:<30} | {:<15.0} | {:<15.0} | {:<15.0}", executor_name, avg_tps, min_tps, max_tps);
    }

    println!();
}

fn main() {
    let args = Args::parse();

    println!("╔══════════════════════════════════════════════════════════════════════════════════════════════════════╗");
    println!("║                              REVM Database Benchmark Suite                                           ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════════════════════════════╝");
    println!();

    let num_blocks = args.num_transactions / args.transactions_per_block;
    let verify_signatures = !args.no_verify;

    println!("Benchmark Configuration:");
    println!("  • Accounts: {}", args.num_accounts);
    println!("  • Transactions per run: {}", args.num_transactions);
    println!("  • Transactions per block: {}", args.transactions_per_block);
    println!("  • Number of blocks: {}", num_blocks);
    println!("  • Signature verification: {}", if verify_signatures { "enabled" } else { "disabled" });
    println!("  • Conflict factors: {:?}", args.conflicts);
    println!("  • Thread counts (parallel): {:?}", args.threads);
    println!();

    let mut all_results: Vec<BenchmarkResult> = Vec::new();

    // Determine which executors to run
    let run_sequential = args.all || args.sequential;
    let run_mdbx_sequential = args.all || args.mdbx_sequential;
    let run_mdbx_batched = args.all || args.mdbx_batched;
    let run_fdb = args.all || args.fdb;
    let run_block_stm = args.all || args.block_stm;

    // Run sequential in-memory executor
    if run_sequential {
        print_section_header("Sequential In-Memory Executor (CacheDB)");
        BenchmarkResult::print_header();

        for &conflict_factor in &args.conflicts {
            let conflict_name = format!("{:.0}% conflicts", conflict_factor * 100.0);
            
            let workload_config = WorkloadConfig {
                num_accounts: args.num_accounts,
                num_transactions: args.num_transactions,
                transactions_per_block: args.transactions_per_block,
                conflict_factor,
                seed: 42,
                chain_id: 1,
            };

            let workload = Workload::generate(workload_config);
            let executor = SequentialExecutor::with_verification(verify_signatures);

            let result = run_in_memory_benchmark(&executor, &workload, &conflict_name, args.num_transactions);
            result.print();
            all_results.push(result);
        }

        println!();
    }

    // Run MDBX sequential executor
    #[cfg(feature = "mdbx")]
    if run_mdbx_sequential {
        print_section_header("MDBX Sequential Executor (Persistent storage)");
        BenchmarkResult::print_header();

        for &conflict_factor in &args.conflicts {
            let conflict_name = format!("{:.0}% conflicts", conflict_factor * 100.0);
            
            let workload_config = WorkloadConfig {
                num_accounts: args.num_accounts,
                num_transactions: args.num_transactions,
                transactions_per_block: args.transactions_per_block,
                conflict_factor,
                seed: 42,
                chain_id: 1,
            };

            let workload = Workload::generate(workload_config);

            let dir = tempdir().expect("Failed to create temp directory");
            let executor = MdbxSequentialExecutor::new(dir.path(), verify_signatures)
                .expect("Failed to create MDBX sequential executor");

            let start = Instant::now();
            let (result, _) = executor
                .execute_workload(&workload)
                .expect("Execution failed");
            let elapsed = start.elapsed();

            let bench_result = BenchmarkResult {
                conflict_name,
                executor_name: executor.name().to_string(),
                preserves_order: executor.preserves_order(),
                successful: result.total_successful,
                failed: result.total_failed,
                duration_ms: elapsed.as_secs_f64() * 1000.0,
                throughput_tps: args.num_transactions as f64 / elapsed.as_secs_f64(),
            };

            bench_result.print();
            all_results.push(bench_result);
        }

        println!();
    }

    // Run MDBX batched executor
    #[cfg(feature = "mdbx")]
    if run_mdbx_batched {
        print_section_header("MDBX Batched Executor (Block-level caching and commit)");
        BenchmarkResult::print_header();

        for &conflict_factor in &args.conflicts {
            let conflict_name = format!("{:.0}% conflicts", conflict_factor * 100.0);
            
            let workload_config = WorkloadConfig {
                num_accounts: args.num_accounts,
                num_transactions: args.num_transactions,
                transactions_per_block: args.transactions_per_block,
                conflict_factor,
                seed: 42,
                chain_id: 1,
            };

            let workload = Workload::generate(workload_config);

            let dir = tempdir().expect("Failed to create temp directory");
            let executor = MdbxBatchedExecutor::new(dir.path(), verify_signatures)
                .expect("Failed to create MDBX batched executor");

            let start = Instant::now();
            let (result, _) = executor
                .execute_workload(&workload)
                .expect("Execution failed");
            let elapsed = start.elapsed();

            let bench_result = BenchmarkResult {
                conflict_name,
                executor_name: executor.name().to_string(),
                preserves_order: executor.preserves_order(),
                successful: result.total_successful,
                failed: result.total_failed,
                duration_ms: elapsed.as_secs_f64() * 1000.0,
                throughput_tps: args.num_transactions as f64 / elapsed.as_secs_f64(),
            };

            bench_result.print();
            all_results.push(bench_result);
        }

        println!();
    }

    // Run Block-STM parallel executor
    #[cfg(feature = "block-stm")]
    if run_block_stm {
        print_section_header("Block-STM Parallel Executor (Optimistic concurrency)");

        for &num_threads in &args.threads {
            println!("--- {} threads ---", num_threads);
            BenchmarkResult::print_header();

            for &conflict_factor in &args.conflicts {
                let conflict_name = format!("{:.0}% conflicts", conflict_factor * 100.0);
                
                let workload_config = WorkloadConfig {
                    num_accounts: args.num_accounts,
                    num_transactions: args.num_transactions,
                    transactions_per_block: args.transactions_per_block,
                    conflict_factor,
                    seed: 42,
                    chain_id: 1,
                };

                let workload = Workload::generate(workload_config);
                let executor = BlockStmExecutor::new(num_threads, verify_signatures);

                let result = run_in_memory_benchmark(&executor, &workload, &conflict_name, args.num_transactions);
                result.print();
                all_results.push(result);
            }

            println!();
        }
    }

    // Run FoundationDB parallel executor
    #[cfg(feature = "fdb")]
    if run_fdb {
        print_section_header("FoundationDB Parallel Executor (Distributed transactional)");

        // Initialize FDB network once
        let _fdb_network = unsafe { foundationdb::boot() };
        std::thread::sleep(std::time::Duration::from_millis(100));

        for &num_threads in &args.threads {
            println!("--- {} threads ---", num_threads);
            BenchmarkResult::print_header();

            for &conflict_factor in &args.conflicts {
                let conflict_name = format!("{:.0}% conflicts", conflict_factor * 100.0);
                
                let workload_config = WorkloadConfig {
                    num_accounts: args.num_accounts,
                    num_transactions: args.num_transactions,
                    transactions_per_block: args.transactions_per_block,
                    conflict_factor,
                    seed: 42,
                    chain_id: 1,
                };

                let workload = Workload::generate(workload_config);

                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                let (result, elapsed) = rt.block_on(async {
                    let executor = FdbParallelExecutor::new(num_threads, verify_signatures)
                        .await
                        .expect("Failed to create FDB executor");

                    let start = Instant::now();
                    let result = executor
                        .execute_workload(&workload)
                        .await
                        .expect("Execution failed");
                    let elapsed = start.elapsed();

                    (result, elapsed)
                });

                let bench_result = BenchmarkResult {
                    conflict_name,
                    executor_name: format!("fdb_parallel_{}t", num_threads),
                    preserves_order: false,
                    successful: result.total_successful,
                    failed: result.total_failed,
                    duration_ms: elapsed.as_secs_f64() * 1000.0,
                    throughput_tps: args.num_transactions as f64 / elapsed.as_secs_f64(),
                };

                bench_result.print();
                all_results.push(bench_result);
            }

            println!();
        }
    }

    // Print warnings for unavailable executors
    #[cfg(not(feature = "mdbx"))]
    if run_mdbx_sequential || run_mdbx_batched {
        println!("⚠️  MDBX executors not available (rebuild with --features mdbx)");
        println!();
    }

    #[cfg(not(feature = "fdb"))]
    if run_fdb {
        println!("⚠️  FoundationDB executor not available (rebuild with --features fdb)");
        println!();
    }

    #[cfg(not(feature = "block-stm"))]
    if run_block_stm {
        println!("⚠️  Block-STM executor not available (rebuild with --features block-stm)");
        println!();
    }

    // Print summary
    print_summary(&all_results);
}
