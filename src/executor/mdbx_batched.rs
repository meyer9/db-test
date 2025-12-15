//! MDBX batched executor with block-level caching.
//!
//! This executor simulates realistic blockchain execution by:
//! - Grouping transactions into blocks
//! - Caching all state changes in memory during block execution
//! - Committing once at the end of each block
//! - Running multiple blocks sequentially

use alloy_primitives::{keccak256, Address, U256};
use eyre::Result;
use reth_primitives_traits::Account;
use std::collections::HashMap;
use std::path::Path;

use super::{ExecutionResult, mdbx::MdbxDatabase};
use crate::Workload;

/// Block execution result with per-block statistics.
#[derive(Debug, Clone)]
pub struct BlockResult {
    /// Block number.
    pub block_number: u64,
    /// Number of successful transactions in this block.
    pub successful: usize,
    /// Number of failed transactions in this block.
    pub failed: usize,
}

/// Multi-block execution result.
#[derive(Debug, Clone)]
pub struct MultiBlockResult {
    /// Results for each block.
    pub blocks: Vec<BlockResult>,
    /// Total successful transactions across all blocks.
    pub total_successful: usize,
    /// Total failed transactions across all blocks.
    pub total_failed: usize,
}

impl MultiBlockResult {
    /// Converts to a simple ExecutionResult for compatibility.
    pub fn to_execution_result(&self) -> ExecutionResult {
        ExecutionResult::new(self.total_successful, self.total_failed)
    }
}

/// In-memory cache for account state changes during block execution.
#[derive(Debug, Default)]
struct BlockCache {
    /// Cached account states.
    accounts: HashMap<Address, Account>,
}

impl BlockCache {
    fn new() -> Self {
        Self {
            accounts: HashMap::new(),
        }
    }

    fn get_account(&self, address: &Address) -> Option<&Account> {
        self.accounts.get(address)
    }

    fn set_account(&mut self, address: Address, account: Account) {
        self.accounts.insert(address, account);
    }

    fn clear(&mut self) {
        self.accounts.clear();
    }
}

/// MDBX batched executor with block-level caching and commit.
///
/// This executor processes transactions in blocks from the workload, caching all state changes
/// in memory during block execution and committing once at the end of each block.
/// This is more realistic for blockchain execution scenarios.
///
/// # Example
///
/// ```ignore
/// use db_test::executor::MdbxBatchedExecutor;
/// use db_test::{Workload, WorkloadConfig};
/// use tempfile::tempdir;
///
/// let dir = tempdir()?;
/// let executor = MdbxBatchedExecutor::new(dir.path(), true)?;
/// let workload = Workload::generate(WorkloadConfig::default());
/// let (result, _) = executor.execute_workload(&workload)?;
/// ```
pub struct MdbxBatchedExecutor {
    db: MdbxDatabase,
    verify_signatures: bool,
}

impl MdbxBatchedExecutor {
    /// Creates a new MDBX batched executor.
    ///
    /// # Arguments
    /// * `path` - Path for the MDBX database
    /// * `verify_signatures` - Whether to verify transaction signatures
    pub fn new<P: AsRef<Path>>(
        path: P,
        verify_signatures: bool,
    ) -> Result<Self> {
        let db = MdbxDatabase::create(path)?;
        Ok(Self {
            db,
            verify_signatures,
        })
    }

    /// Executes a workload across multiple blocks with batched commits.
    /// 
    /// The workload must have transactions organized into blocks (via transactions_per_block config).
    pub fn execute_workload(&self, workload: &Workload) -> Result<(MultiBlockResult, ())> {
        // Initialize accounts in the database
        let accounts: Vec<_> = workload
            .accounts
            .iter()
            .map(|acc| (acc.address, U256::from(1_000_000_000_000_000_000_000u128)))
            .collect();
        
        self.db.init_accounts(&accounts)?;

        let mut block_results = Vec::new();
        let mut total_successful = 0;
        let mut total_failed = 0;

        // Process each block from the workload
        for (block_num, block_txs) in workload.blocks.iter().enumerate() {
            // Execute block with caching
            let (successful, failed) = self.execute_block(block_txs)?;
            
            block_results.push(BlockResult {
                block_number: block_num as u64,
                successful,
                failed,
            });

            total_successful += successful;
            total_failed += failed;
        }

        Ok((
            MultiBlockResult {
                blocks: block_results,
                total_successful,
                total_failed,
            },
            (),
        ))
    }

    /// Executes a single block of transactions with in-memory caching and a single commit.
    fn execute_block(&self, transactions: &[crate::SignedTransaction]) -> Result<(usize, usize)> {
        let mut cache = BlockCache::new();
        let mut successful = 0;
        let mut failed = 0;

        // Execute all transactions in the block, caching changes
        for tx in transactions {
            // Verify signature if enabled
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

            // Get sender account (from cache or database)
            let mut sender = if let Some(cached) = cache.get_account(&tx.from) {
                cached.clone()
            } else {
                match self.db.get_account(tx.from)? {
                    Some(acc) => acc,
                    None => {
                        failed += 1;
                        continue;
                    }
                }
            };

            // Check nonce
            if sender.nonce != tx.nonce {
                failed += 1;
                continue;
            }

            // Check balance
            if sender.balance < tx.value {
                failed += 1;
                continue;
            }

            // Get receiver account (from cache or database)
            let mut receiver = if let Some(cached) = cache.get_account(&tx.to) {
                cached.clone()
            } else {
                self.db.get_account(tx.to)?.unwrap_or(Account {
                    nonce: 0,
                    balance: U256::ZERO,
                    bytecode_hash: None,
                })
            };

            // Execute transfer in cache
            sender.balance -= tx.value;
            sender.nonce += 1;
            receiver.balance += tx.value;

            // Update cache
            cache.set_account(tx.from, sender);
            cache.set_account(tx.to, receiver);

            successful += 1;
        }

        // Commit all cached changes to database in a single transaction
        self.commit_cache(&cache)?;

        Ok((successful, failed))
    }

    /// Commits all cached account changes to the database in a single transaction.
    fn commit_cache(&self, cache: &BlockCache) -> Result<()> {
        use reth_db_api::{database::Database, transaction::{DbTx, DbTxMut}};
        
        let tx = self.db.env.tx_mut()?;
        
        for (address, account) in &cache.accounts {
            let hashed_address = keccak256(address.as_slice());
            tx.put::<super::mdbx::HashedAccountsTable>(hashed_address, account.clone())?;
        }
        
        tx.commit()?;
        Ok(())
    }

    /// Returns whether this executor preserves transaction ordering.
    pub fn preserves_order(&self) -> bool {
        true
    }

    /// Returns the name of this executor.
    pub fn name(&self) -> &'static str {
        "mdbx_batched"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkloadConfig;
    use tempfile::tempdir;

    #[test]
    fn test_mdbx_batched_executor() {
        let dir = tempdir().unwrap();
        let config = WorkloadConfig {
            num_accounts: 20,
            num_transactions: 50,
            conflict_factor: 0.0,
            seed: 42,
            chain_id: 1,
            transactions_per_block: 10,
        };

        let workload = Workload::generate(config);
        let executor = MdbxBatchedExecutor::new(dir.path(), true).unwrap();

        let (result, _) = executor.execute_workload(&workload).unwrap();
        
        assert_eq!(result.blocks.len(), 5);
        assert_eq!(result.total_successful, 50);
        assert_eq!(result.total_failed, 0);
        
        // Verify each block has up to 10 transactions
        for (i, block) in result.blocks.iter().enumerate() {
            assert_eq!(block.block_number, i as u64);
            assert!(block.successful <= 10);
        }
    }

    #[test]
    fn test_block_cache() {
        let mut cache = BlockCache::new();
        
        let addr = Address::with_last_byte(42);
        let account = Account {
            nonce: 5,
            balance: U256::from(1000),
            bytecode_hash: None,
        };
        
        cache.set_account(addr, account);
        
        let retrieved = cache.get_account(&addr).unwrap();
        assert_eq!(retrieved.nonce, 5);
        assert_eq!(retrieved.balance, U256::from(1000));
        
        cache.clear();
        assert!(cache.get_account(&addr).is_none());
    }

    #[test]
    fn test_partial_block() {
        let dir = tempdir().unwrap();
        let config = WorkloadConfig {
            num_accounts: 10,
            num_transactions: 25, // Not evenly divisible by block size
            conflict_factor: 0.0,
            seed: 42,
            chain_id: 1,
            transactions_per_block: 10,
        };

        let workload = Workload::generate(config);
        let executor = MdbxBatchedExecutor::new(dir.path(), true).unwrap();

        let (result, _) = executor.execute_workload(&workload).unwrap();
        
        // Should have 3 blocks: 10 + 10 + 5
        assert_eq!(result.blocks.len(), 3);
        assert_eq!(result.total_successful, 25);
    }
}

