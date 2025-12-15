//! FoundationDB parallel executor with automatic conflict resolution.
//!
//! This executor uses FoundationDB's built-in MVCC and optimistic concurrency control
//! to execute transactions in parallel across multiple threads. Conflicts are automatically
//! retried an infinite number of times until they succeed.
//!
//! Key features:
//! - Parallel execution across configurable number of threads
//! - Automatic conflict detection and retry
//! - Atomic transactions
//! - Does NOT preserve strict ordering due to parallel execution and retries

use alloy_primitives::{keccak256, Address, U256};
use foundationdb::{Database, FdbBindingError};
use std::sync::Arc;
use std::thread;

use super::ExecutionResult;
use crate::Workload;

/// Result of multi-threaded execution with per-thread statistics.
#[derive(Debug, Clone)]
pub struct ParallelExecutionResult {
    /// Results from each thread.
    pub thread_results: Vec<ThreadResult>,
    /// Total successful transactions across all threads.
    pub total_successful: usize,
    /// Total failed transactions across all threads.
    pub total_failed: usize,
}

impl ParallelExecutionResult {
    /// Converts to a simple ExecutionResult for compatibility.
    pub fn to_execution_result(&self) -> ExecutionResult {
        ExecutionResult::new(self.total_successful, self.total_failed)
    }
}

/// Result from a single thread of execution.
#[derive(Debug, Clone)]
pub struct ThreadResult {
    /// Thread ID.
    pub thread_id: usize,
    /// Number of successful transactions.
    pub successful: usize,
    /// Number of permanently failed transactions (e.g., invalid signatures).
    /// Validation failures (nonce mismatch, insufficient balance) are retried until success.
    pub failed: usize,
}

/// FoundationDB parallel executor with automatic retry and conflict resolution.
///
/// This executor processes transactions in parallel using multiple threads.
/// Each transaction is executed atomically through FoundationDB's `Database::run`
/// which automatically retries on conflicts until success.
///
/// # Retry Behavior
/// - **FDB conflicts**: Automatic infinite retry (handled by FDB)
/// - **Nonce mismatches**: Manual infinite retry with 100μs delay
/// - **Insufficient balance**: Manual infinite retry with 100μs delay (rare with 1 wei transfers)
/// - **Invalid signatures**: Permanent failure (no retry)
///
/// With 1 wei transfers and large initial balances, retries are primarily due to nonce
/// ordering in parallel execution. This means transactions will eventually succeed 
/// (showing decreased TPS) rather than failing outright.
///
/// # Example
///
/// ```ignore
/// use db_test::executor::FdbParallelExecutor;
/// use db_test::{Workload, WorkloadConfig};
///
/// let executor = FdbParallelExecutor::new(4, true).await?; // 4 threads
/// let workload = Workload::generate(WorkloadConfig::default());
/// let result = executor.execute_workload(&workload).await?;
/// ```
pub struct FdbParallelExecutor {
    db: Arc<Database>,
    verify_signatures: bool,
    num_threads: usize,
}

impl FdbParallelExecutor {
    /// Creates a new FoundationDB parallel executor.
    ///
    /// # Arguments
    /// * `num_threads` - Number of threads to use for parallel execution
    /// * `verify_signatures` - Whether to verify transaction signatures
    pub async fn new(num_threads: usize, verify_signatures: bool) -> Result<Self, FdbBindingError> {
        let db = Database::default()?;
        
        Ok(Self {
            db: Arc::new(db),
            verify_signatures,
            num_threads: num_threads.max(1),
        })
    }

    /// Clears all keys from the database.
    /// This is useful for starting with a clean slate.
    pub async fn clear_database(&self) -> Result<(), FdbBindingError> {
        let db = self.db.clone();
        
        // Use a transaction to clear our account key space
        // Using a narrow range is better practice than clearing everything
        db.run(|trx, _maybe_committed| async move {
            // Clear only our account keyspace
            trx.clear_range(b"account/", b"account/\xff");
            Ok(())
        })
        .await?;
        
        Ok(())
    }

    /// Initializes accounts in the database.
    /// Batches the writes to avoid transaction_too_old errors.
    pub async fn init_accounts(&self, accounts: &[(Address, U256)]) -> Result<(), FdbBindingError> {
        let db = self.db.clone();
        
        // Batch size - keep transactions small to avoid hitting time limits
        const BATCH_SIZE: usize = 1000;
        
        // Process accounts in batches
        for chunk in accounts.chunks(BATCH_SIZE) {
            let accounts_batch = chunk.to_vec();
            
            db.run(|trx, _maybe_committed| {
                let accounts_batch = accounts_batch.clone();
                async move {
                    for (address, balance) in accounts_batch {
                        let key = Self::account_key(address);
                        let value = Self::encode_account(0, balance);
                        trx.set(&key, &value);
                    }
                    Ok(())
                }
            })
            .await?;
        }
        
        Ok(())
    }

    /// Executes a workload across multiple threads with parallel execution.
    /// 
    /// Transaction boundaries: Each ETH transfer = one FDB transaction
    /// - We use workload.transactions (flat list), NOT workload.blocks
    /// - Each thread processes a subset of transactions
    /// - Each transaction within a thread is an independent FDB transaction
    /// - FDB handles all conflict detection and retry automatically
    pub async fn execute_workload(
        &self,
        workload: &Workload,
    ) -> Result<ParallelExecutionResult, FdbBindingError> {
        // Clear the database first
        self.clear_database().await?;
        
        // Initialize accounts in batches to avoid transaction_too_old
        let accounts: Vec<_> = workload
            .accounts
            .iter()
            .map(|acc| (acc.address, U256::from(1_000_000_000_000_000_000_000u128)))
            .collect();
        
        self.init_accounts(&accounts).await?;

        // Divide transactions among threads (each thread gets a slice of the flat transaction list)
        let txs_per_thread = (workload.transactions.len() + self.num_threads - 1) / self.num_threads;
        
        let mut handles = Vec::new();
        
        for thread_id in 0..self.num_threads {
            let start_idx = thread_id * txs_per_thread;
            let end_idx = (start_idx + txs_per_thread).min(workload.transactions.len());
            
            if start_idx >= workload.transactions.len() {
                break;
            }
            
            let thread_txs = workload.transactions[start_idx..end_idx].to_vec();
            let db = self.db.clone();
            let verify_signatures = self.verify_signatures;
            
            let handle = thread::spawn(move || {
                Self::execute_thread(thread_id, db, &thread_txs, verify_signatures)
            });
            
            handles.push(handle);
        }
        
        // Collect results from all threads
        let mut thread_results = Vec::new();
        let mut total_successful = 0;
        let mut total_failed = 0;
        
        for handle in handles {
            let result = handle.join().expect("Thread panicked");
            total_successful += result.successful;
            total_failed += result.failed;
            thread_results.push(result);
        }
        
        Ok(ParallelExecutionResult {
            thread_results,
            total_successful,
            total_failed,
        })
    }

    /// Executes transactions on a single thread with infinite retry.
    /// 
    /// IMPORTANT: Each ETH transfer is executed as its own independent FDB transaction.
    /// This allows maximum concurrency - multiple threads can execute transfers in parallel,
    /// and FDB's MVCC will automatically detect conflicts and retry until success.
    /// 
    /// Retry behavior:
    /// - Invalid signatures: Fail immediately (permanent error)
    /// - Nonce mismatches: Retry with 100μs delay (the main retry case with parallel execution)
    /// - Insufficient balance: Retry with 100μs delay (rare with 1 wei transfers)
    /// - FDB conflicts: Automatic retry (handled by db.run())
    /// 
    /// With 1 wei transfers, nonce ordering is the primary challenge.
    /// 
    /// The `workload.blocks` structure is ignored - we process all transactions in a flat list.
    fn execute_thread(
        thread_id: usize,
        db: Arc<Database>,
        transactions: &[crate::SignedTransaction],
        verify_signatures: bool,
    ) -> ThreadResult {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        
        let mut successful = 0;
        let mut failed = 0;
        
        for tx in transactions {
            let tx = tx.clone();
            
            // ═══════════════════════════════════════════════════════════════════════════
            // Each iteration of this loop = ONE FDB transaction (one ETH transfer)
            // We retry until success - validation failures will retry after a delay
            // db.run() provides automatic conflict detection and retry
            // ═══════════════════════════════════════════════════════════════════════════
            
            // Verify signature once upfront (permanent failure if invalid)
            if verify_signatures {
                let recovered = tx.recover_signer();
                if recovered.is_none() || recovered.unwrap() != tx.from {
                    failed += 1;
                    continue; // Skip this transaction - signature is permanently invalid
                }
            }
            
            // Retry loop for validation failures
            // With 1 wei transfers: primarily nonce mismatches from out-of-order execution
            loop {
                let result = rt.block_on(async {
                    db.run(|trx, _maybe_committed| {
                        let tx = tx.clone();
                        async move {
                            // Get sender account
                            let sender_key = Self::account_key(tx.from);
                            let sender_data = trx.get(&sender_key, false).await?;
                            
                            let sender_data = match sender_data {
                                Some(data) => data,
                                None => return Ok(false), // Account not found
                            };
                            
                            let (sender_nonce, sender_balance) = Self::decode_account(&sender_data);
                            
                            // Check nonce - might be wrong due to out-of-order parallel execution
                            if sender_nonce != tx.nonce {
                                return Ok(false); // Nonce mismatch - will retry
                            }
                            
                            // Check balance
                            if sender_balance < tx.value {
                                return Ok(false); // Insufficient balance - will retry
                            }
                            
                            // Get receiver account
                            let receiver_key = Self::account_key(tx.to);
                            let receiver_data = trx.get(&receiver_key, false).await?;
                            
                            let (receiver_nonce, receiver_balance) = if let Some(data) = receiver_data {
                                Self::decode_account(&data)
                            } else {
                                (0, U256::ZERO)
                            };
                            
                            // Execute transfer
                            let new_sender_balance = sender_balance - tx.value;
                            let new_sender_nonce = sender_nonce + 1;
                            let new_receiver_balance = receiver_balance + tx.value;
                            
                            // Write updates
                            trx.set(&sender_key, &Self::encode_account(new_sender_nonce, new_sender_balance));
                            trx.set(&receiver_key, &Self::encode_account(receiver_nonce, new_receiver_balance));
                            
                            Ok(true) // Success!
                        }
                    })
                    .await
                });
                
                match result {
                    Ok(true) => {
                        // Transaction succeeded
                        successful += 1;
                        break;
                    }
                    Ok(false) => {
                        // Validation failed (nonce mismatch or insufficient balance)
                        // Wait a tiny bit and retry - another transaction might complete
                        std::thread::sleep(std::time::Duration::from_micros(100));
                        continue; // Retry the transaction
                    }
                    Err(_) => {
                        // FDB error (should be rare due to automatic retry)
                        // Wait and retry
                        std::thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                }
            }
        }
        
        ThreadResult {
            thread_id,
            successful,
            failed,
        }
    }

    /// Returns whether this executor preserves transaction ordering.
    pub fn preserves_order(&self) -> bool {
        false // Parallel execution with retries does not guarantee order
    }

    /// Returns the name of this executor.
    pub fn name(&self) -> &'static str {
        "fdb_parallel"
    }

    /// Returns the number of threads.
    pub fn num_threads(&self) -> usize {
        self.num_threads
    }

    // Helper methods for key encoding
    
    fn account_key(address: Address) -> Vec<u8> {
        let mut key = b"account/".to_vec();
        key.extend_from_slice(keccak256(address.as_slice()).as_slice());
        key
    }
    
    fn encode_account(nonce: u64, balance: U256) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&nonce.to_be_bytes());
        data.extend_from_slice(&balance.to_be_bytes::<32>());
        data
    }
    
    fn decode_account(data: &[u8]) -> (u64, U256) {
        let nonce = u64::from_be_bytes(data[0..8].try_into().unwrap());
        let balance = U256::from_be_bytes::<32>(data[8..40].try_into().unwrap());
        (nonce, balance)
    }
    
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkloadConfig;

    #[tokio::test]
    #[ignore] // Requires FoundationDB running
    async fn test_fdb_parallel_executor() {
        // This test requires FoundationDB to be running
        let config = WorkloadConfig {
            num_accounts: 20,
            num_transactions: 50,
            conflict_factor: 0.5, // Some conflicts
            seed: 42,
            chain_id: 1,
            transactions_per_block: 10,
        };

        let workload = Workload::generate(config);
        let executor = FdbParallelExecutor::new(4, true).await.unwrap();

        let result = executor.execute_workload(&workload).await.unwrap();
        
        assert_eq!(result.total_successful, 50);
        assert_eq!(result.total_failed, 0);
        assert!(executor.preserves_order() == false);
        assert_eq!(executor.name(), "fdb_parallel");
        assert_eq!(executor.num_threads(), 4);
    }

    #[tokio::test]
    #[ignore] // Requires FoundationDB running
    async fn test_clear_database() {
        let executor = FdbParallelExecutor::new(1, true).await.unwrap();
        
        // Add some data
        let accounts = vec![
            (Address::with_last_byte(1), U256::from(1000)),
            (Address::with_last_byte(2), U256::from(2000)),
        ];
        executor.init_accounts(&accounts).await.unwrap();
        
        // Clear database
        executor.clear_database().await.unwrap();
        
        // Verify it's empty by trying to read
        // (In a real test, you'd query the DB to verify)
    }
}

