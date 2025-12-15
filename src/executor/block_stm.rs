//! Block-STM parallel executor wrapper.
//!
//! This module provides a wrapper around the block-stm-executor crate,
//! implementing the Executor trait for use in benchmarks.

use crate::executor::{ExecutionResult, Executor};
use crate::Workload;
use alloy_primitives::U256;
use block_stm_executor::{AccountState, ExecutorConfig, ParallelExecutor, Transaction};
use revm::database::{CacheDB, EmptyDB};
use std::collections::HashMap;

/// Block-STM parallel executor.
///
/// This executor uses optimistic concurrency control with push-based invalidation
/// to execute transactions in parallel while preserving strict ordering.
#[derive(Debug)]
pub struct BlockStmExecutor {
    pub num_threads: usize,
    pub verify_signatures: bool,
}

impl BlockStmExecutor {
    /// Creates a new Block-STM executor with the specified number of threads.
    pub fn new(num_threads: usize, verify_signatures: bool) -> Self {
        Self {
            num_threads,
            verify_signatures,
        }
    }
}

impl Executor for BlockStmExecutor {
    type Database = CacheDB<EmptyDB>;

    fn execute(
        &self,
        _db: Self::Database,
        workload: &Workload,
    ) -> (Self::Database, ExecutionResult) {
        // Extract initial account states from the workload
        // All accounts start with the same initial balance (1000 ETH)
        let mut initial_states = HashMap::new();
        let initial_balance = U256::from(1_000_000_000_000_000_000_000u128); // 1000 ETH
        
        for account in &workload.accounts {
            initial_states.insert(
                account.address,
                AccountState::new(0, initial_balance),
            );
        }
        
        // Convert all transactions across all blocks to Block-STM format
        let mut block_stm_txs = Vec::new();
        for block in &workload.blocks {
            for tx in block {
                block_stm_txs.push(Transaction {
                    from: tx.from,
                    to: tx.to,
                    value: tx.value,
                    nonce: tx.nonce,
                    signature_valid: tx.recover_signer().is_some(),
                });
            }
        }
        
        // Execute with Block-STM
        let config = ExecutorConfig {
            num_threads: self.num_threads,
            verify_signatures: self.verify_signatures,
            initial_states,
        };
        
        let executor = ParallelExecutor::new(config);
        let result = executor.execute_block(block_stm_txs);
        
        // Create a fresh database with final states
        let mut final_db = CacheDB::new(EmptyDB::default());
        for (address, state) in result.final_states {
            use revm::state::AccountInfo;
            use revm::primitives::KECCAK_EMPTY;
            let info = AccountInfo {
                balance: state.balance,
                nonce: state.nonce,
                code_hash: KECCAK_EMPTY,
                code: None,
            };
            final_db.insert_account_info(address, info);
        }
        
        let exec_result = ExecutionResult {
            successful: result.successful,
            failed: result.failed,
        };
        
        (final_db, exec_result)
    }

    fn preserves_order(&self) -> bool {
        true // Block-STM maintains strict transaction ordering
    }

    fn name(&self) -> &'static str {
        "block_stm_parallel"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Account, WorkloadConfig};

    #[test]
    fn test_block_stm_executor() {
        let executor = BlockStmExecutor::new(2, false);
        
        let config = WorkloadConfig {
            num_accounts: 10,
            num_transactions: 20,
            transactions_per_block: 10,
            conflict_factor: 0.0,
            seed: 42,
            chain_id: 1,
        };
        
        let workload = Workload::generate(config);
        let db = workload.create_db();
        
        let (_, result) = executor.execute(db, &workload);
        
        // All transactions should succeed with no conflicts
        assert_eq!(result.total_successful, 20);
        assert_eq!(result.total_failed, 0);
    }
}

