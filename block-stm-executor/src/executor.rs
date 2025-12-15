//! Parallel transaction executor with Block-STM.

use crate::mvhashmap::{MVHashMap, ReadResult};
use crate::scheduler::{Scheduler, Task};
use crate::types::{AccountState, Incarnation, TxnIndex, Version};
use alloy_primitives::{Address, U256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

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
    pub signature_valid: bool,
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
        _worker_id: usize,
        scheduler: Arc<Scheduler>,
        mv_hashmap: Arc<MVHashMap>,
        transactions: Arc<Vec<Transaction>>,
        initial_states: HashMap<Address, AccountState>,
        verify_signatures: bool,
        execution_count: Arc<AtomicUsize>,
        success_count: Arc<AtomicUsize>,
        fail_count: Arc<AtomicUsize>,
    ) {
        loop {
            match scheduler.next_task() {
                Task::Execute(txn_idx, incarnation) => {
                    execution_count.fetch_add(1, Ordering::Relaxed);
                    
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
                        Err(_reason) => {
                            // Execution failed permanently (e.g., invalid signature)
                            fail_count.fetch_add(1, Ordering::Relaxed);
                            
                            // Mark as executed with no invalidations
                            scheduler.finish_execution(txn_idx, incarnation, vec![]);
                        }
                    }
                }
                Task::Wait => {
                    // No task available, sleep briefly
                    thread::sleep(Duration::from_micros(10));
                }
                Task::Done => {
                    // All done
                    break;
                }
            }
        }
    }

    /// Executes a single transaction and returns read/write sets and invalidations.
    fn execute_transaction(
        tx: &Transaction,
        txn_idx: TxnIndex,
        incarnation: Incarnation,
        mv_hashmap: &MVHashMap,
        initial_states: &HashMap<Address, AccountState>,
        verify_signatures: bool,
    ) -> Result<(Vec<Address>, Vec<Address>, Vec<TxnIndex>), String> {
        // Verify signature if enabled
        if verify_signatures && !tx.signature_valid {
            return Err("Invalid signature".to_string());
        }
        
        // Read sender account
        let sender_state = Self::read_account(tx.from, txn_idx, mv_hashmap, initial_states);
        
        // Validate nonce
        if sender_state.nonce != tx.nonce {
            return Err(format!(
                "Nonce mismatch: expected {}, got {}",
                sender_state.nonce, tx.nonce
            ));
        }
        
        // Validate balance
        if sender_state.balance < tx.value {
            return Err("Insufficient balance".to_string());
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
                // Read from initial state
                initial_states
                    .get(&address)
                    .copied()
                    .unwrap_or(AccountState::new(0, U256::ZERO))
            }
            ReadResult::Dependency(_) => {
                // This shouldn't happen in our sequential-order execution
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

    #[test]
    fn test_parallel_execution_simple() {
        let addr1 = Address::random();
        let addr2 = Address::random();
        let addr3 = Address::random();
        
        let mut initial_states = HashMap::new();
        initial_states.insert(addr1, AccountState::new(0, U256::from(1000)));
        initial_states.insert(addr2, AccountState::new(0, U256::from(1000)));
        initial_states.insert(addr3, AccountState::new(0, U256::from(1000)));
        
        let transactions = vec![
            Transaction {
                from: addr1,
                to: addr2,
                value: U256::from(10),
                nonce: 0,
                signature_valid: true,
            },
            Transaction {
                from: addr2,
                to: addr3,
                value: U256::from(5),
                nonce: 0,
                signature_valid: true,
            },
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

