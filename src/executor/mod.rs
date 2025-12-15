//! Transaction execution strategies.
//!
//! This module provides the [`Executor`] trait and implementations for
//! different transaction execution strategies.

mod sequential;

pub use sequential::SequentialExecutor;

use crate::Workload;

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
/// use db_test::{Executor, ExecutionResult, Workload};
/// use revm::database::{CacheDB, EmptyDB};
///
/// pub struct MyExecutor;
///
/// impl Executor for MyExecutor {
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
}


