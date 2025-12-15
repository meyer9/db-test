//! Core types for Block-STM execution.

use alloy_primitives::{Address, U256};
use std::fmt;

/// Transaction index in the block (0-based).
pub type TxnIndex = usize;

/// Incarnation number (how many times a transaction has been re-executed).
pub type Incarnation = usize;

/// Version identifier for a transaction execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub txn_idx: TxnIndex,
    pub incarnation: Incarnation,
}

impl Version {
    pub fn new(txn_idx: TxnIndex, incarnation: Incarnation) -> Self {
        Self { txn_idx, incarnation }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.txn_idx, self.incarnation)
    }
}

/// Account state in the EVM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountState {
    pub nonce: u64,
    pub balance: U256,
}

impl AccountState {
    pub fn new(nonce: u64, balance: U256) -> Self {
        Self { nonce, balance }
    }
}

/// Read or write operation on an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessType {
    Read,
    Write,
}

/// A memory access (read or write) by a transaction.
#[derive(Debug, Clone)]
pub struct MemoryAccess {
    pub address: Address,
    pub access_type: AccessType,
    pub version: Option<Version>, // Version read from (for reads)
    pub value: Option<AccountState>, // Value written (for writes)
}

/// Result of a transaction execution attempt.
#[derive(Debug, Clone)]
pub enum ExecutionResult {
    /// Transaction executed successfully.
    Success {
        read_set: Vec<MemoryAccess>,
        write_set: Vec<MemoryAccess>,
        gas_used: u64,
    },
    /// Transaction execution failed (signature invalid, etc.).
    Failed {
        reason: String,
    },
    /// Transaction needs to be re-executed (dependency changed).
    Retry,
}

/// Status of a transaction in the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStatus {
    /// Not yet scheduled for execution.
    Pending,
    /// Currently executing.
    Executing(Incarnation),
    /// Finished execution, result available.
    Executed(Incarnation),
    /// Committed to final state.
    Committed,
}


