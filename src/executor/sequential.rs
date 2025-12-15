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

use super::{ExecutionResult, Executor};
use crate::Workload;

/// Sequential executor that processes transactions one at a time.
///
/// This is the baseline executor that processes transactions in order,
/// verifying signatures and executing each transaction before moving to the next.
///
/// # Example
///
/// ```
/// use db_test::{Executor, SequentialExecutor, Workload, WorkloadConfig};
///
/// let config = WorkloadConfig::default();
/// let workload = Workload::generate(config);
/// let db = workload.create_db();
///
/// let executor = SequentialExecutor::new(true); // with signature verification
/// let (final_db, result) = executor.execute(db, &workload);
///
/// println!("Successful: {}, Failed: {}", result.successful, result.failed);
/// ```
#[derive(Debug, Clone, Default)]
pub struct SequentialExecutor {
    /// Whether to verify signatures during execution.
    pub verify_signatures: bool,
}

impl SequentialExecutor {
    /// Creates a new sequential executor.
    ///
    /// # Arguments
    /// * `verify_signatures` - If true, recovers and verifies the signer address
    ///   from each transaction's signature before execution.
    pub fn new(verify_signatures: bool) -> Self {
        Self { verify_signatures }
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

        let executor = SequentialExecutor::new(true);
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

        let executor = SequentialExecutor::new(false);
        let (_, result) = executor.execute(db, &workload);

        assert_eq!(result.successful, 5);
        assert_eq!(result.failed, 0);
    }
}


