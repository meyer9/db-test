//! Parallel transaction executor with Block-STM.

use crate::mvhashmap::{MVHashMap, ReadResult};
use crate::scheduler::{Scheduler, Task};
use crate::types::{AccountState, Incarnation, TxnIndex, Version};
use alloy_primitives::{Address, Signature, B256, U256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Error type for transaction execution.
#[derive(Debug, Clone)]
pub enum ExecutionError {
    /// Permanent failure - transaction is invalid (e.g., bad signature).
    Permanent(String),
    /// Retry needed - a dependency hasn't been resolved yet.
    /// The transaction should be re-executed after invalidation.
    Retry,
}

/// A simplified transaction for execution.
///
/// In practice, this would be imported from the main crate, but for now
/// we'll define it here.
#[derive(Debug, Clone)]
pub struct Transaction {
    pub from: Address,
    pub to: Address,
    pub value: U256,
    pub nonce: u64,
    /// The ECDSA signature for verification (done in parallel).
    pub signature: Signature,
    /// The hash that was signed.
    pub tx_hash: B256,
}

impl Transaction {
    /// Recovers the signer address from the signature.
    /// This is the expensive cryptographic operation that should be parallelized.
    pub fn recover_signer(&self) -> Option<Address> {
        self.signature
            .recover_address_from_prehash(&self.tx_hash)
            .ok()
    }

    /// Verifies the signature matches the claimed sender.
    pub fn verify_signature(&self) -> bool {
        self.recover_signer()
            .map(|addr| addr == self.from)
            .unwrap_or(false)
    }
}

/// Configuration for parallel execution.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Number of worker threads.
    pub num_threads: usize,
    /// Whether to verify signatures.
    pub verify_signatures: bool,
    /// Initial account states (address -> (nonce, balance)).
    pub initial_states: HashMap<Address, AccountState>,
}

/// Result of parallel block execution.
#[derive(Debug, Clone)]
pub struct BlockExecutionResult {
    /// Number of successful transactions.
    pub successful: usize,
    /// Number of failed transactions.
    pub failed: usize,
    /// Total number of transaction executions (including re-executions).
    pub total_executions: usize,
    /// Final account states after execution.
    pub final_states: Vec<(Address, AccountState)>,
    /// Execution time.
    pub duration: Duration,
}

/// Parallel Block-STM executor.
pub struct ParallelExecutor {
    config: ExecutorConfig,
}

impl ParallelExecutor {
    /// Creates a new parallel executor.
    pub fn new(config: ExecutorConfig) -> Self {
        Self { config }
    }

    /// Executes a block of transactions in parallel.
    pub fn execute_block(&self, transactions: Vec<Transaction>) -> BlockExecutionResult {
        let start = Instant::now();
        let num_txns = transactions.len();
        
        // Create shared state
        let scheduler = Scheduler::new(num_txns);
        let mv_hashmap = Arc::new(MVHashMap::new());
        let transactions = Arc::new(transactions);
        let execution_count = Arc::new(AtomicUsize::new(0));
        let success_count = Arc::new(AtomicUsize::new(0));
        let fail_count = Arc::new(AtomicUsize::new(0));
        
        // Spawn worker threads
        let mut handles = Vec::new();
        for worker_id in 0..self.config.num_threads {
            let scheduler = scheduler.clone();
            let mv_hashmap = mv_hashmap.clone();
            let transactions = transactions.clone();
            let initial_states = self.config.initial_states.clone();
            let verify_signatures = self.config.verify_signatures;
            let execution_count = execution_count.clone();
            let success_count = success_count.clone();
            let fail_count = fail_count.clone();
            
            let handle = thread::spawn(move || {
                Self::worker_loop(
                    worker_id,
                    scheduler,
                    mv_hashmap,
                    transactions,
                    initial_states,
                    verify_signatures,
                    execution_count,
                    success_count,
                    fail_count,
                );
            });
            
            handles.push(handle);
        }
        
        // Wait for all workers to finish
        for handle in handles {
            handle.join().expect("Worker thread panicked");
        }
        
        let duration = start.elapsed();
        
        // Collect final states
        let final_states = mv_hashmap.get_committed_states();
        
        BlockExecutionResult {
            successful: success_count.load(Ordering::Acquire),
            failed: fail_count.load(Ordering::Acquire),
            total_executions: execution_count.load(Ordering::Acquire),
            final_states,
            duration,
        }
    }

    /// Worker thread main loop.
    fn worker_loop(
        worker_id: usize,
        scheduler: Arc<Scheduler>,
        mv_hashmap: Arc<MVHashMap>,
        transactions: Arc<Vec<Transaction>>,
        initial_states: HashMap<Address, AccountState>,
        verify_signatures: bool,
        execution_count: Arc<AtomicUsize>,
        success_count: Arc<AtomicUsize>,
        fail_count: Arc<AtomicUsize>,
    ) {
        let mut local_executions = 0;
        let mut wait_count = 0;
        
        loop {
            match scheduler.next_task() {
                Task::Execute(txn_idx, incarnation) => {
                    local_executions += 1;
                    execution_count.fetch_add(1, Ordering::Relaxed);
                    wait_count = 0; // Reset wait counter on successful task
                    
                    let tx = &transactions[txn_idx];
                    
                    // Execute the transaction
                    let result = Self::execute_transaction(
                        tx,
                        txn_idx,
                        incarnation,
                        &mv_hashmap,
                        &initial_states,
                        verify_signatures,
                    );
                    
                    match result {
                        Ok((_read_addrs, _write_addrs, invalidated)) => {
                            // Execution succeeded
                            success_count.fetch_add(1, Ordering::Relaxed);
                            
                            // Notify scheduler
                            scheduler.finish_execution(txn_idx, incarnation, invalidated);
                        }
                        Err(ExecutionError::Retry) => {
                            // Transaction couldn't execute due to unmet dependencies.
                            // The reads have been recorded, so when the dependency writes,
                            // this transaction will be invalidated and re-executed.
                            // Mark as "executed" so it can be invalidated.
                            scheduler.finish_execution(txn_idx, incarnation, vec![]);
                        }
                        Err(ExecutionError::Permanent(_reason)) => {
                            // Execution failed permanently (e.g., invalid signature)
                            fail_count.fetch_add(1, Ordering::Relaxed);
                            
                            // Mark as executed with no invalidations
                            scheduler.finish_execution(txn_idx, incarnation, vec![]);
                        }
                    }
                    
                    // Log progress every 1000 executions
                    if local_executions % 1000 == 0 {
                        let stats = scheduler.stats();
                        let total_exec = execution_count.load(Ordering::Relaxed);
                        eprintln!(
                            "[Worker {}] Executed: {}, Total: {}, Pending: {}, Executing: {}, Executed: {}, Committed: {}, Incarnations: {}",
                            worker_id,
                            local_executions,
                            total_exec,
                            stats.pending,
                            stats.executing,
                            stats.executed,
                            stats.committed,
                            stats.total_incarnations
                        );
                    }
                }
                Task::Wait => {
                    // No task available, sleep briefly
                    wait_count += 1;
                    
                    // Log if we're waiting too long
                    if wait_count % 100 == 0 {
                        let stats = scheduler.stats();
                        eprintln!(
                            "[Worker {}] Waiting... ({}x) - Pending: {}, Executing: {}, Executed: {}, Committed: {}",
                            worker_id,
                            wait_count,
                            stats.pending,
                            stats.executing,
                            stats.executed,
                            stats.committed
                        );
                    }
                    
                    thread::sleep(Duration::from_micros(10));
                }
                Task::Done => {
                    // All done
                    eprintln!(
                        "[Worker {}] Done! Total executions: {}",
                        worker_id,
                        local_executions
                    );
                    break;
                }
            }
        }
    }

    /// Executes a single transaction and returns read/write sets and invalidations.
    /// 
    /// Returns:
    /// - Ok(...) - Transaction executed successfully
    /// - Err(ExecutionError::Permanent) - Transaction failed permanently (bad signature)
    /// - Err(ExecutionError::Retry) - Transaction should be retried (nonce/balance dependency)
    fn execute_transaction(
        tx: &Transaction,
        txn_idx: TxnIndex,
        incarnation: Incarnation,
        mv_hashmap: &MVHashMap,
        initial_states: &HashMap<Address, AccountState>,
        verify_signatures: bool,
    ) -> Result<(Vec<Address>, Vec<Address>, Vec<TxnIndex>), ExecutionError> {
        // Verify signature if enabled - this is the expensive operation that
        // benefits from parallelization (~50-200μs per signature recovery)
        if verify_signatures && !tx.verify_signature() {
            return Err(ExecutionError::Permanent("Invalid signature".to_string()));
        }
        
        // Read sender account
        let sender_state = Self::read_account(tx.from, txn_idx, mv_hashmap, initial_states);
        
        // Validate nonce - if wrong, we need to retry (dependency not ready)
        if sender_state.nonce != tx.nonce {
            // This means a lower-indexed transaction that updates this account
            // hasn't executed yet. We should retry later.
            return Err(ExecutionError::Retry);
        }
        
        // Validate balance - if insufficient, retry (might be updated by another tx)
        if sender_state.balance < tx.value {
            return Err(ExecutionError::Retry);
        }
        
        // Read receiver account
        let receiver_state = Self::read_account(tx.to, txn_idx, mv_hashmap, initial_states);
        
        // Execute transfer
        let new_sender_state = AccountState::new(
            sender_state.nonce + 1,
            sender_state.balance - tx.value,
        );
        let new_receiver_state = AccountState::new(
            receiver_state.nonce,
            receiver_state.balance + tx.value,
        );
        
        // Write updates to multi-version hashmap
        let mut invalidated = Vec::new();
        
        let write_result_sender = mv_hashmap.write(tx.from, txn_idx, incarnation, new_sender_state);
        invalidated.extend(write_result_sender.invalidated_readers);
        
        let write_result_receiver = mv_hashmap.write(tx.to, txn_idx, incarnation, new_receiver_state);
        invalidated.extend(write_result_receiver.invalidated_readers);
        
        // Remove duplicates and sort
        invalidated.sort_unstable();
        invalidated.dedup();
        
        Ok((
            vec![tx.from, tx.to],  // read addresses
            vec![tx.from, tx.to],  // write addresses
            invalidated,
        ))
    }

    /// Reads an account from the multi-version hashmap or initial state.
    fn read_account(
        address: Address,
        reader_txn_idx: TxnIndex,
        mv_hashmap: &MVHashMap,
        initial_states: &HashMap<Address, AccountState>,
    ) -> AccountState {
        match mv_hashmap.read(address, reader_txn_idx) {
            ReadResult::Versioned(version, state) => {
                // Record this read for push-based invalidation
                mv_hashmap.record_read(address, reader_txn_idx, version);
                state
            }
            ReadResult::Storage => {
                // Record storage read for push-based invalidation!
                // When a lower-indexed tx writes to this address, we must be invalidated.
                mv_hashmap.record_storage_read(address, reader_txn_idx);
                
                // Read from initial state
                initial_states
                    .get(&address)
                    .copied()
                    .unwrap_or(AccountState::new(0, U256::ZERO))
            }
            ReadResult::Dependency(_) => {
                // This shouldn't happen in our implementation
                initial_states
                    .get(&address)
                    .copied()
                    .unwrap_or(AccountState::new(0, U256::ZERO))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::keccak256;
    use k256::ecdsa::{SigningKey, VerifyingKey};
    use rand::{rngs::StdRng, Rng, SeedableRng};

    /// A test account with signing key.
    struct TestAccount {
        signing_key: SigningKey,
        address: Address,
    }

    impl TestAccount {
        fn from_seed(seed: u64) -> Self {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut key_bytes = [0u8; 32];
            rng.fill(&mut key_bytes);
            let signing_key = SigningKey::from_bytes(&key_bytes.into())
                .expect("valid key bytes");
            let verifying_key = VerifyingKey::from(&signing_key);
            let address = public_key_to_address(&verifying_key);
            Self { signing_key, address }
        }

        fn sign_tx(&self, to: Address, value: U256, nonce: u64) -> Transaction {
            let tx_hash = compute_tx_hash(self.address, to, value, nonce);
            let (sig, recovery_id) = self.signing_key
                .sign_prehash_recoverable(tx_hash.as_slice())
                .expect("signing should succeed");
            let signature = Signature::from_signature_and_parity(sig, recovery_id.is_y_odd());
            
            Transaction {
                from: self.address,
                to,
                value,
                nonce,
                signature,
                tx_hash,
            }
        }
    }

    fn public_key_to_address(verifying_key: &VerifyingKey) -> Address {
        let public_key_bytes = verifying_key.to_encoded_point(false);
        let hash = keccak256(&public_key_bytes.as_bytes()[1..]);
        Address::from_slice(&hash[12..])
    }

    fn compute_tx_hash(from: Address, to: Address, value: U256, nonce: u64) -> B256 {
        let mut data = Vec::with_capacity(20 + 20 + 32 + 8);
        data.extend_from_slice(from.as_slice());
        data.extend_from_slice(to.as_slice());
        data.extend_from_slice(&value.to_be_bytes::<32>());
        data.extend_from_slice(&nonce.to_be_bytes());
        keccak256(&data)
    }

    #[test]
    fn test_parallel_execution_simple() {
        // Generate test accounts with proper signing keys
        let acc1 = TestAccount::from_seed(1);
        let acc2 = TestAccount::from_seed(2);
        let acc3 = TestAccount::from_seed(3);
        
        let mut initial_states = HashMap::new();
        initial_states.insert(acc1.address, AccountState::new(0, U256::from(1000)));
        initial_states.insert(acc2.address, AccountState::new(0, U256::from(1000)));
        initial_states.insert(acc3.address, AccountState::new(0, U256::from(1000)));
        
        // Create properly signed transactions
        let transactions = vec![
            acc1.sign_tx(acc2.address, U256::from(10), 0),
            acc2.sign_tx(acc3.address, U256::from(5), 0),
        ];
        
        let config = ExecutorConfig {
            num_threads: 2,
            verify_signatures: true,
            initial_states,
        };
        
        let executor = ParallelExecutor::new(config);
        let result = executor.execute_block(transactions);
        
        assert_eq!(result.successful, 2);
        assert_eq!(result.failed, 0);
    }
}

