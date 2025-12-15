//! Scheduler for coordinating parallel transaction execution with push-based invalidation.

use crate::types::{ExecutionStatus, Incarnation, TxnIndex, Version};
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// Task for a worker thread to execute.
#[derive(Debug, Clone)]
pub enum Task {
    /// Execute a transaction at a specific incarnation.
    Execute(TxnIndex, Incarnation),
    /// No task currently available, retry later.
    Wait,
    /// All transactions are done.
    Done,
}

/// Scheduler state for coordinating parallel execution.
pub struct Scheduler {
    /// Number of transactions in the block.
    num_txns: usize,
    
    /// Execution status for each transaction.
    statuses: Vec<RwLock<ExecutionStatus>>,
    
    /// Queue of transactions ready to execute.
    ready_queue: Mutex<VecDeque<(TxnIndex, Incarnation)>>,
    
    /// Highest transaction index that has been committed.
    committed_idx: AtomicUsize,
    
    /// Number of transactions that have been executed at least once.
    executed_once_count: AtomicUsize,
    
    /// Whether execution is done.
    done: AtomicBool,
    
    /// Lock for committing transactions (only one thread can commit at a time).
    commit_lock: Mutex<()>,
}

impl Scheduler {
    /// Creates a new scheduler for a block of transactions.
    pub fn new(num_txns: usize) -> Arc<Self> {
        let mut ready_queue = VecDeque::new();
        
        // Initially, all transactions are ready to execute for the first time
        for idx in 0..num_txns {
            ready_queue.push_back((idx, 0));
        }
        
        Arc::new(Self {
            num_txns,
            statuses: (0..num_txns)
                .map(|_| RwLock::new(ExecutionStatus::Pending))
                .collect(),
            ready_queue: Mutex::new(ready_queue),
            committed_idx: AtomicUsize::new(0),
            executed_once_count: AtomicUsize::new(0),
            done: AtomicBool::new(false),
            commit_lock: Mutex::new(()),
        })
    }

    /// Gets the next task for a worker thread.
    pub fn next_task(&self) -> Task {
        // Check if we're done
        if self.done.load(Ordering::Acquire) {
            return Task::Done;
        }
        
        // Try to get a task from the ready queue
        let mut queue = self.ready_queue.lock();
        
        if let Some((txn_idx, incarnation)) = queue.pop_front() {
            // Mark as executing
            *self.statuses[txn_idx].write() = ExecutionStatus::Executing(incarnation);
            drop(queue);
            
            return Task::Execute(txn_idx, incarnation);
        }
        
        drop(queue);
        
        // Check if all transactions are committed
        if self.committed_idx.load(Ordering::Acquire) >= self.num_txns {
            self.done.store(true, Ordering::Release);
            return Task::Done;
        }
        
        // No task available right now
        Task::Wait
    }

    /// Marks a transaction as executed successfully.
    ///
    /// Returns the list of transactions that were invalidated by this execution.
    pub fn finish_execution(
        &self,
        txn_idx: TxnIndex,
        incarnation: Incarnation,
        invalidated: Vec<TxnIndex>,
    ) {
        // Update status
        *self.statuses[txn_idx].write() = ExecutionStatus::Executed(incarnation);
        
        // Track if this was the first execution
        if incarnation == 0 {
            self.executed_once_count.fetch_add(1, Ordering::AcqRel);
        }
        
        // Abort invalidated transactions
        for &invalid_idx in &invalidated {
            self.abort_transaction(invalid_idx);
        }
        
        // Try to acquire commit lock and commit transactions
        // Only one thread should do this at a time to avoid contention
        if let Some(_guard) = self.commit_lock.try_lock() {
            self.try_commit_transactions();
        }
    }

    /// Aborts a transaction and schedules it for re-execution.
    pub fn abort_transaction(&self, txn_idx: TxnIndex) {
        let mut status = self.statuses[txn_idx].write();
        
        match *status {
            ExecutionStatus::Executing(incarnation) | ExecutionStatus::Executed(incarnation) => {
                // Re-schedule for execution with incremented incarnation
                let new_incarnation = incarnation + 1;
                *status = ExecutionStatus::Pending;
                
                let mut queue = self.ready_queue.lock();
                queue.push_back((txn_idx, new_incarnation));
            }
            _ => {
                // Already pending or committed, nothing to do
            }
        }
    }

    /// Tries to commit transactions in order.
    fn try_commit_transactions(&self) {
        let mut committed_idx = self.committed_idx.load(Ordering::Acquire);
        let start_idx = committed_idx;
        
        // Commit transactions in order as long as they're executed
        while committed_idx < self.num_txns {
            let status = self.statuses[committed_idx].read();
            
            match *status {
                ExecutionStatus::Executed(_) => {
                    drop(status);
                    
                    // Commit this transaction
                    *self.statuses[committed_idx].write() = ExecutionStatus::Committed;
                    
                    // Move to next
                    committed_idx += 1;
                    self.committed_idx.store(committed_idx, Ordering::Release);
                }
                _ => {
                    // This transaction is not ready to commit yet
                    break;
                }
            }
        }
        
        // Log commit progress every 1000 commits
        if start_idx % 1000 == 0 && committed_idx > start_idx {
            eprintln!(
                "[Scheduler] Committed {} -> {} (total: {}/{})",
                start_idx,
                committed_idx,
                committed_idx,
                self.num_txns
            );
        }
        
        // If all transactions are committed, mark as done
        if committed_idx >= self.num_txns {
            self.done.store(true, Ordering::Release);
            eprintln!("[Scheduler] All {} transactions committed!", self.num_txns);
        }
    }

    /// Checks if a transaction has been committed.
    pub fn is_committed(&self, txn_idx: TxnIndex) -> bool {
        matches!(
            *self.statuses[txn_idx].read(),
            ExecutionStatus::Committed
        )
    }

    /// Gets the current status of a transaction.
    pub fn get_status(&self, txn_idx: TxnIndex) -> ExecutionStatus {
        *self.statuses[txn_idx].read()
    }

    /// Checks if all transactions are done.
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// Gets statistics about execution progress.
    pub fn stats(&self) -> SchedulerStats {
        let mut pending = 0;
        let mut executing = 0;
        let mut executed = 0;
        let mut committed = 0;
        let mut total_incarnations = 0;
        
        for status_lock in &self.statuses {
            match *status_lock.read() {
                ExecutionStatus::Pending => pending += 1,
                ExecutionStatus::Executing(inc) => {
                    executing += 1;
                    total_incarnations += inc + 1;
                }
                ExecutionStatus::Executed(inc) => {
                    executed += 1;
                    total_incarnations += inc + 1;
                }
                ExecutionStatus::Committed => committed += 1,
            }
        }
        
        SchedulerStats {
            pending,
            executing,
            executed,
            committed,
            total_incarnations,
        }
    }
}

/// Statistics about scheduler state.
#[derive(Debug, Clone)]
pub struct SchedulerStats {
    pub pending: usize,
    pub executing: usize,
    pub executed: usize,
    pub committed: usize,
    pub total_incarnations: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_basic() {
        let scheduler = Scheduler::new(3);
        
        // Should get tasks for all 3 transactions
        match scheduler.next_task() {
            Task::Execute(idx, inc) => {
                assert_eq!(idx, 0);
                assert_eq!(inc, 0);
            }
            _ => panic!("Expected Execute task"),
        }
        
        match scheduler.next_task() {
            Task::Execute(idx, inc) => {
                assert_eq!(idx, 1);
                assert_eq!(inc, 0);
            }
            _ => panic!("Expected Execute task"),
        }
    }

    #[test]
    fn test_abort_and_reexecute() {
        let scheduler = Scheduler::new(2);
        
        // Execute transaction 0
        let _ = scheduler.next_task();
        scheduler.finish_execution(0, 0, vec![]);
        
        // Execute transaction 1
        let _ = scheduler.next_task();
        
        // Transaction 1 invalidates transaction 0
        scheduler.finish_execution(1, 0, vec![0]);
        
        // Should get transaction 0 again with incarnation 1
        match scheduler.next_task() {
            Task::Execute(idx, inc) => {
                assert_eq!(idx, 0);
                assert_eq!(inc, 1);
            }
            _ => panic!("Expected Execute task for re-execution"),
        }
    }
}

