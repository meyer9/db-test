//! Block-STM parallel executor for blockchain transactions.
//!
//! This crate implements a simplified version of the Block-STM algorithm for
//! parallel execution of blockchain transactions. It uses optimistic concurrency
//! control with push-based invalidation to achieve high throughput while
//! maintaining strict transaction ordering guarantees.
//!
//! # Core Components
//!
//! - **MVHashMap**: Multi-version data structure storing versioned account states
//! - **Scheduler**: Coordinates parallel execution and handles push-based invalidation
//! - **ParallelExecutor**: Orchestrates worker threads and transaction execution
//!
//! # Algorithm Overview
//!
//! 1. Transactions are executed speculatively in parallel
//! 2. Each write records which transactions have read from the previous version
//! 3. When a transaction writes, readers with higher indices are immediately aborted
//! 4. Aborted transactions are re-executed with incremented incarnation numbers
//! 5. Transactions commit in order once all lower-indexed transactions are done
//!
//! # Example
//!
//! ```rust,ignore
//! use block_stm_executor::{ParallelExecutor, ExecutorConfig, Transaction};
//! use alloy_primitives::{Address, U256};
//! use std::collections::HashMap;
//!
//! let config = ExecutorConfig {
//!     num_threads: 4,
//!     verify_signatures: true,
//!     initial_states: HashMap::new(),
//! };
//!
//! let executor = ParallelExecutor::new(config);
//! let transactions = vec![/* ... */];
//! let result = executor.execute_block(transactions);
//!
//! println!("Successful: {}, Failed: {}", result.successful, result.failed);
//! ```

pub mod executor;
pub mod mvhashmap;
pub mod scheduler;
pub mod types;

pub use executor::{BlockExecutionResult, ExecutorConfig, ParallelExecutor, Transaction};
pub use types::{AccountState, Incarnation, TxnIndex, Version};
