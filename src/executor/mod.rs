//! Transaction execution strategies.
//!
//! This module provides the [`Executor`] trait and implementations for
//! different transaction execution strategies.

mod sequential;

#[cfg(feature = "mdbx")]
mod mdbx;

#[cfg(feature = "mdbx")]
mod mdbx_batched;

#[cfg(feature = "fdb")]
mod fdb;

pub use sequential::SequentialExecutor;

#[cfg(feature = "mdbx")]
pub use mdbx::{MdbxDatabase, MdbxSequentialExecutor};

#[cfg(feature = "mdbx")]
pub use mdbx_batched::{BlockResult, MdbxBatchedExecutor, MultiBlockResult};

#[cfg(feature = "fdb")]
pub use fdb::{FdbParallelExecutor, ParallelExecutionResult, ThreadResult};

use crate::Workload;

/// Transaction ordering requirements.
///
/// This enum controls whether transactions must be executed in the exact order
/// they appear in the workload, or if some reordering is acceptable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrderingMode {
    /// Strict ordering: transactions must be executed in the exact order specified.
    ///
    /// Use this when:
    /// - Transaction order is semantically important
    /// - You need deterministic, reproducible execution
    /// - Debugging or comparing different executors
    #[default]
    Strict,

    /// Loose ordering: transactions may be reordered for performance.
    ///
    /// Guarantees:
    /// - Transactions from the same sender are never reordered relative to each other
    /// - The final state will be equivalent to some valid ordering of all transactions
    ///
    /// Use this when:
    /// - Maximum throughput is desired
    /// - Transaction order is not semantically important
    /// - Enabling parallel or optimistic execution strategies
    ///
    /// Note: Sequential executors ignore this flag as they naturally maintain strict ordering.
    Loose,
}

impl OrderingMode {
    /// Returns true if strict ordering is required.
    pub fn is_strict(&self) -> bool {
        matches!(self, OrderingMode::Strict)
    }

    /// Returns true if loose ordering is allowed.
    pub fn is_loose(&self) -> bool {
        matches!(self, OrderingMode::Loose)
    }
}

/// Result of executing a workload.
#[derive(Debug, Clone, Default)]
pub struct ExecutionResult {
    /// Number of successfully executed transactions.
    pub successful: usize,
    /// Number of failed transactions (reverted or validation error).
    pub failed: usize,
}

impl ExecutionResult {
    /// Creates a new execution result.
    pub fn new(successful: usize, failed: usize) -> Self {
        Self { successful, failed }
    }

    /// Total number of transactions processed.
    pub fn total(&self) -> usize {
        self.successful + self.failed
    }
}

/// Trait for different transaction execution strategies.
///
/// This allows benchmarking different approaches to executing transactions,
/// such as sequential execution, parallel execution, or optimistic execution.
///
/// # Implementing a New Executor
///
/// ```ignore
/// use db_test::executor::{Executor, ExecutionResult, OrderingMode};
/// use db_test::Workload;
/// use revm::database::{CacheDB, EmptyDB};
///
/// pub struct ParallelExecutor {
///     pub verify_signatures: bool,
///     pub num_threads: usize,
/// }
///
/// impl Executor for ParallelExecutor {
///     type Database = CacheDB<EmptyDB>;
///
///     fn execute(
///         &self,
///         db: Self::Database,
///         workload: &Workload,
///     ) -> (Self::Database, ExecutionResult) {
///         // Your implementation here
///         todo!()
///     }
///
///     fn preserves_order(&self) -> bool {
///         false // Parallel executor may reorder transactions
///     }
/// }
/// ```
pub trait Executor {
    /// The database type this executor operates on.
    type Database: revm::Database + revm::DatabaseCommit;

    /// Executes the workload on the given database.
    ///
    /// # Arguments
    /// * `db` - The database to execute transactions on.
    /// * `workload` - The workload containing signed transactions to execute.
    ///
    /// # Returns
    /// A tuple of (final database state, execution result).
    fn execute(
        &self,
        db: Self::Database,
        workload: &Workload,
    ) -> (Self::Database, ExecutionResult);

    /// Returns whether this executor preserves strict transaction ordering.
    ///
    /// If true, transactions are guaranteed to execute in the exact order they
    /// appear in the workload. If false, transactions may be reordered for
    /// performance (though per-sender ordering is always preserved).
    fn preserves_order(&self) -> bool {
        true // Default to strict ordering for safety
    }

    /// Returns a human-readable name for this executor.
    fn name(&self) -> &'static str {
        "unknown"
    }
}
