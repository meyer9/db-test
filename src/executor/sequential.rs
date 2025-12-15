//! Sequential transaction executor.
//!
//! This module provides a baseline sequential executor that processes
//! transactions one at a time with optional signature verification.

use revm::{
    context::TxEnv,
    database::{CacheDB, EmptyDB},
    primitives::TxKind,
    Context, ExecuteCommitEvm, MainBuilder, MainContext,
};

use super::{ExecutionResult, Executor, OrderingMode};
use crate::Workload;

/// Sequential executor that processes transactions one at a time.
///
/// This is the baseline executor that processes transactions in order,
/// verifying signatures and executing each transaction before moving to the next.
///
/// # Ordering Behavior
///
/// The sequential executor always maintains strict ordering regardless of the
/// `ordering` configuration, since it processes transactions one at a time.
/// The `ordering` field is provided for consistency with other executors.
///
/// # Example
///
/// ```
/// use db_test::{Executor, SequentialExecutor, Workload, WorkloadConfig};
/// use db_test::executor::OrderingMode;
///
/// let config = WorkloadConfig::default();
/// let workload = Workload::generate(config);
/// let db = workload.create_db();
///
/// // With signature verification and strict ordering (default)
/// let executor = SequentialExecutor::new(true, OrderingMode::Strict);
/// let (final_db, result) = executor.execute(db, &workload);
///
/// println!("Successful: {}, Failed: {}", result.successful, result.failed);
/// ```
#[derive(Debug, Clone)]
pub struct SequentialExecutor {
    /// Whether to verify signatures during execution.
    pub verify_signatures: bool,
    /// Ordering mode (ignored for sequential execution).
    pub ordering: OrderingMode,
}

impl SequentialExecutor {
    /// Creates a new sequential executor.
    ///
    /// # Arguments
    /// * `verify_signatures` - If true, recovers and verifies the signer address
    ///   from each transaction's signature before execution.
    /// * `ordering` - Ordering mode (ignored for sequential execution, as it
    ///   naturally maintains strict ordering).
    pub fn new(verify_signatures: bool, ordering: OrderingMode) -> Self {
        Self {
            verify_signatures,
            ordering,
        }
    }

    /// Creates a new sequential executor with default ordering (strict).
    ///
    /// This is a convenience constructor for the common case where you just
    /// want to control signature verification.
    pub fn with_verification(verify_signatures: bool) -> Self {
        Self::new(verify_signatures, OrderingMode::default())
    }
}

impl Default for SequentialExecutor {
    fn default() -> Self {
        Self::new(true, OrderingMode::default())
    }
}

impl Executor for SequentialExecutor {
    type Database = CacheDB<EmptyDB>;

    fn execute(
        &self,
        db: Self::Database,
        workload: &Workload,
    ) -> (Self::Database, ExecutionResult) {
        let mut successful = 0;
        let mut failed = 0;

        // Create the EVM context with mainnet configuration.
        let mut evm = Context::mainnet().with_db(db).build_mainnet();

        // Note: Sequential execution always maintains strict ordering,
        // regardless of self.ordering configuration.
        for tx in &workload.transactions {
            // Verify signature if enabled.
            if self.verify_signatures {
                let recovered = match tx.recover_signer() {
                    Some(addr) => addr,
                    None => {
                        failed += 1;
                        continue;
                    }
                };

                if recovered != tx.from {
                    failed += 1;
                    continue;
                }
            }

            // Build the transaction environment.
            let tx_env = TxEnv {
                caller: tx.from,
                kind: TxKind::Call(tx.to),
                value: tx.value,
                gas_limit: 21_000,
                gas_price: 1,
                nonce: tx.nonce,
                chain_id: Some(workload.config.chain_id),
                ..Default::default()
            };

            // Execute and commit the transaction.
            match evm.transact_commit(tx_env) {
                Ok(result) => {
                    if result.is_success() {
                        successful += 1;
                    } else {
                        failed += 1;
                    }
                }
                Err(_) => {
                    failed += 1;
                }
            }
        }

        (
            evm.ctx.journaled_state.database,
            ExecutionResult::new(successful, failed),
        )
    }

    fn preserves_order(&self) -> bool {
        true // Sequential execution always preserves order
    }

    fn name(&self) -> &'static str {
        "sequential_in_memory"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkloadConfig;

    #[test]
    fn test_sequential_executor_with_verification() {
        let config = WorkloadConfig {
            num_accounts: 10,
            num_transactions: 5,
            conflict_factor: 0.0,
            seed: 42,
            chain_id: 1,
        };

        let workload = Workload::generate(config);
        let db = workload.create_db();

        let executor = SequentialExecutor::new(true, OrderingMode::Strict);
        let (_, result) = executor.execute(db, &workload);

        assert_eq!(result.successful, 5);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn test_sequential_executor_without_verification() {
        let config = WorkloadConfig {
            num_accounts: 10,
            num_transactions: 5,
            conflict_factor: 0.0,
            seed: 42,
            chain_id: 1,
        };

        let workload = Workload::generate(config);
        let db = workload.create_db();

        let executor = SequentialExecutor::new(false, OrderingMode::Loose);
        let (_, result) = executor.execute(db, &workload);

        assert_eq!(result.successful, 5);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn test_sequential_executor_convenience_constructor() {
        let config = WorkloadConfig {
            num_accounts: 10,
            num_transactions: 5,
            conflict_factor: 0.0,
            seed: 42,
            chain_id: 1,
        };

        let workload = Workload::generate(config);
        let db = workload.create_db();

        let executor = SequentialExecutor::with_verification(true);
        let (_, result) = executor.execute(db, &workload);

        assert_eq!(result.successful, 5);
        assert_eq!(result.failed, 0);
        assert_eq!(executor.ordering, OrderingMode::Strict);
    }

    #[test]
    fn test_ordering_mode_methods() {
        assert!(OrderingMode::Strict.is_strict());
        assert!(!OrderingMode::Strict.is_loose());

        assert!(!OrderingMode::Loose.is_strict());
        assert!(OrderingMode::Loose.is_loose());

        assert_eq!(OrderingMode::default(), OrderingMode::Strict);
    }
}
