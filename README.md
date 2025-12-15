# REVM Database Benchmark

Benchmarking framework for comparing different `revm::Database` implementations with realistic transaction workloads including signature verification.

## Overview

This project benchmarks ETH transfer transactions using the [revm](https://docs.rs/revm/latest/revm/) EVM implementation. It measures transaction throughput under various conditions:

- **Conflict levels**: How transaction contention affects performance
- **Batch sizes**: Performance with different transaction batch sizes
- **Account pools**: Impact of account pool size on database operations
- **Signature verification**: Overhead of ECDSA signature recovery

## Architecture

### Core Components

```
┌─────────────────┐     ┌──────────────────┐     ┌────────────────┐
│  WorkloadConfig │ ──► │     Workload     │ ──► │    Executor    │
│  - num_accounts │     │  - accounts[]    │     │  - execute()   │
│  - num_txs      │     │  - transactions[]│     └────────────────┘
│  - conflict_fac │     │  - create_db()   │              │
│  - seed         │     └──────────────────┘              ▼
└─────────────────┘                               ┌────────────────┐
                                                  │ExecutionResult │
                                                  │  - successful  │
                                                  │  - failed      │
                                                  └────────────────┘
```

### Executor Trait

The `Executor` trait allows plugging in different execution strategies:

```rust
pub trait Executor {
    type Database: revm::Database + revm::DatabaseCommit;

    fn execute(
        &self,
        db: Self::Database,
        workload: &Workload,
    ) -> (Self::Database, ExecutionResult);
}
```

Currently implemented:
- **SequentialExecutor**: In-memory (CacheDB) sequential execution
- **MdbxSequentialExecutor**: MDBX-backed persistent storage (requires `mdbx` feature)

Planned implementations:
- Parallel execution
- Optimistic execution with conflict detection
- Batched execution

#### Executor Capabilities

All executors implement methods to query their behavior:
- `preserves_order()` - Returns `true` if transactions execute in strict order
- `name()` - Returns a human-readable identifier for benchmarking

### Signature Verification

Transactions are pre-signed during workload generation. During execution:

1. The transaction hash is computed from (from, to, value, nonce, chain_id)
2. The signature is created using secp256k1 ECDSA
3. During execution, `recover_signer()` recovers the address from the signature
4. The recovered address is compared against the expected sender

## Quick Start

```bash
# Run comprehensive benchmark suite (in-memory only)
cargo run --release

# Run with MDBX persistent storage benchmarks included
cargo run --release --features mdbx

# Run MDBX-only example
cargo run --example mdbx_benchmark --features mdbx --release

# Run the full criterion benchmarks
cargo bench

# Run tests (including MDBX if feature enabled)
cargo test --all-features
```

### Benchmark Output

The main executable runs all available executors with different conflict ratios and provides:
- Individual benchmark results for each configuration
- Summary statistics per executor (avg/min/max throughput)
- Ordering information (strict vs loose)
- Clear indication of which features are enabled

Example output:
```
════════════════════════════════════════════════════════════════════════════════
  In-Memory Executor (CacheDB)
════════════════════════════════════════════════════════════════════════════════

Config               | Executor             | Ordering | Successful | Failed | Time (ms) | TPS         
No conflicts         | sequential_in_memory | strict   | 1000       | 0      | 191.53    | 5221        
25% conflicts        | sequential_in_memory | strict   | 1000       | 0      | 191.26    | 5228        
...

  Summary Statistics

sequential_in_memory:
  • Average throughput: 5218 tx/s
  • Min throughput: 5212 tx/s
  • Max throughput: 5228 tx/s
  • Preserves order: true

mdbx_sequential:
  • Average throughput: 606 tx/s
  • Preserves order: true
```

## Features

### MDBX Persistent Storage

Enable the `mdbx` feature to use MDBX for persistent storage:

```toml
[dependencies]
db-test = { version = "*", features = ["mdbx"] }
```

The MDBX backend uses Reth's database implementation with two tables:
- **HashedAccounts**: Stores account state indexed by `keccak256(address)`
- **HashedStorages**: DupSort table storing storage values by hashed account and key

```rust
use db_test::executor::MdbxSequentialExecutor;
use tempfile::tempdir;

let dir = tempdir()?;
let executor = MdbxSequentialExecutor::new(dir.path(), true)?;
let (result, _) = executor.execute_workload(&workload)?;
```

## Configuration

```rust
WorkloadConfig {
    num_accounts: 1000,      // Total accounts in the system
    num_transactions: 100,   // Transactions per batch
    conflict_factor: 0.5,    // 0.0 = no conflicts, 1.0 = max conflicts
    seed: 42,                // Random seed for reproducibility
    chain_id: 1,             // Chain ID for EIP-155 signing
}
```

### Conflict Factor

The `conflict_factor` parameter controls how many transactions compete for the same accounts:

- `0.0`: Transactions uniformly distributed across all accounts (minimal conflicts)
- `0.5`: Half the transactions target a small "hot" set of accounts
- `1.0`: All transactions compete for the same small set of accounts (maximum conflicts)

## Benchmark Results

Example output showing the impact of signature verification:

```
--- With Signature Verification ---
No conflicts         |  1000 successful |   196.83 ms |     5080 tx/s

--- Without Signature Verification ---
No conflicts         |  1000 successful |     1.04 ms |   965559 tx/s
```

Signature verification adds ~190x overhead due to ECDSA recovery.

HTML reports are generated at:
- `target/criterion/eth_transfer/conflict_levels/report/index.html`
- `target/criterion/eth_transfer/batch_sizes/report/index.html`
- `target/criterion/eth_transfer/account_pools/report/index.html`
- `target/criterion/eth_transfer/signature_verification/report/index.html`

## Project Structure

```
db-test/
├── src/
│   ├── lib.rs          # Core library
│   │   ├── Account     # Keypair + address
│   │   ├── SignedTransaction
│   │   ├── Workload    # Pre-generated benchmark data
│   │   ├── Executor    # Trait for execution strategies
│   │   └── SequentialExecutor
│   └── main.rs         # CLI runner
├── benches/
│   └── eth_transfer.rs # Criterion benchmarks
└── Cargo.toml
```

## Extending with New Executors

To implement a new execution strategy:

```rust
use db_test::{Executor, ExecutionResult, Workload};
use revm::database::{CacheDB, EmptyDB};

pub struct MyExecutor;

impl Executor for MyExecutor {
    type Database = CacheDB<EmptyDB>;

    fn execute(
        &self,
        db: Self::Database,
        workload: &Workload,
    ) -> (Self::Database, ExecutionResult) {
        // Your implementation here
        todo!()
    }
}
```
