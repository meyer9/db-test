//! Multi-version hashmap for storing versioned account states.
//!
//! This is the core data structure that enables parallel execution with
//! optimistic concurrency control. For each address, it stores a versioned
//! history of account states, allowing transactions to read from the correct
//! version based on transaction ordering.

use crate::types::{AccountState, Incarnation, TxnIndex, Version};
use alloy_primitives::Address;
use dashmap::DashMap;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Entry in the version history for an address.
#[derive(Debug, Clone)]
pub struct VersionedEntry {
    pub version: Version,
    pub state: AccountState,
    /// Transactions that have read from this version (for push-based invalidation).
    pub readers: Vec<TxnIndex>,
}

/// Multi-version hashmap storing versioned account states.
///
/// Structure: Address -> BTreeMap<TxnIndex -> VersionedEntry>
///
/// The BTreeMap is keyed by TxnIndex for efficient range queries to find
/// the latest version written by a transaction with index < current_txn_idx.
pub struct MVHashMap {
    /// Map from address to version history.
    data: DashMap<Address, BTreeMap<TxnIndex, VersionedEntry>>,
}

/// Result of reading from the MVHashMap.
#[derive(Debug, Clone)]
pub enum ReadResult {
    /// Value found at a specific version.
    Versioned(Version, AccountState),
    /// No version found, should read from base storage.
    Storage,
    /// Dependency on a transaction that is still executing.
    Dependency(TxnIndex),
}

/// Result of writing to the MVHashMap.
#[derive(Debug, Clone)]
pub struct WriteResult {
    /// Transactions that had read the previous version and need to be invalidated.
    pub invalidated_readers: Vec<TxnIndex>,
}

impl MVHashMap {
    /// Creates a new empty multi-version hashmap.
    pub fn new() -> Self {
        Self {
            data: DashMap::new(),
        }
    }

    /// Reads the latest version of an account for the given transaction index.
    ///
    /// Returns:
    /// - `ReadResult::Versioned` if a version exists from a lower transaction
    /// - `ReadResult::Storage` if no version exists (read from base storage)
    /// - `ReadResult::Dependency` if the latest write is from a higher or equal transaction
    pub fn read(&self, address: Address, reader_txn_idx: TxnIndex) -> ReadResult {
        let entry = self.data.get(&address);
        
        if let Some(versions) = entry {
            // Find the latest version written by a transaction with txn_idx < reader_txn_idx
            if let Some((_write_txn_idx, entry)) = versions
                .range(..reader_txn_idx)
                .next_back()
            {
                return ReadResult::Versioned(entry.version, entry.state);
            }
            
            // Check if there's a write from a higher transaction (shouldn't happen in correct execution)
            if let Some((_write_txn_idx, _)) = versions.range(reader_txn_idx..).next() {
                // This is a dependency on a later transaction - should not happen
                // as we execute in order, but return Storage to be safe
                return ReadResult::Storage;
            }
        }
        
        // No version found, read from storage
        ReadResult::Storage
    }

    /// Writes a new version of an account state.
    ///
    /// This invalidates any transactions that read from the previous version
    /// at this address and have a higher transaction index.
    ///
    /// Returns the list of transaction indices that need to be invalidated.
    pub fn write(
        &self,
        address: Address,
        writer_txn_idx: TxnIndex,
        incarnation: Incarnation,
        state: AccountState,
    ) -> WriteResult {
        let mut invalidated = Vec::new();
        
        self.data
            .entry(address)
            .and_modify(|versions| {
                // Collect readers from the previous version that need to be invalidated
                if let Some((_prev_txn_idx, prev_entry)) = versions
                    .range(..writer_txn_idx)
                    .next_back()
                {
                    // Any reader with txn_idx > writer_txn_idx needs to be invalidated
                    invalidated.extend(
                        prev_entry
                            .readers
                            .iter()
                            .filter(|&&reader_idx| reader_idx > writer_txn_idx)
                            .copied(),
                    );
                }
                
                // Insert or update the version for this transaction
                versions.insert(
                    writer_txn_idx,
                    VersionedEntry {
                        version: Version::new(writer_txn_idx, incarnation),
                        state,
                        readers: Vec::new(),
                    },
                );
            })
            .or_insert_with(|| {
                let mut versions = BTreeMap::new();
                versions.insert(
                    writer_txn_idx,
                    VersionedEntry {
                        version: Version::new(writer_txn_idx, incarnation),
                        state,
                        readers: Vec::new(),
                    },
                );
                versions
            });
        
        WriteResult {
            invalidated_readers: invalidated,
        }
    }

    /// Records that a transaction has read from a specific version.
    ///
    /// This is used for push-based invalidation: when a transaction writes,
    /// we can immediately identify which readers need to be invalidated.
    pub fn record_read(&self, address: Address, reader_txn_idx: TxnIndex, version: Version) {
        if let Some(mut versions) = self.data.get_mut(&address) {
            if let Some(entry) = versions.get_mut(&version.txn_idx) {
                if entry.version == version && !entry.readers.contains(&reader_txn_idx) {
                    entry.readers.push(reader_txn_idx);
                }
            }
        }
    }

    /// Clears all versions for a transaction (used when aborting/re-executing).
    pub fn clear_transaction(&self, txn_idx: TxnIndex) {
        for mut entry in self.data.iter_mut() {
            entry.value_mut().remove(&txn_idx);
        }
    }

    /// Gets the committed state for final output (after all transactions are done).
    pub fn get_committed_states(&self) -> Vec<(Address, AccountState)> {
        let mut result = Vec::new();
        
        for entry in self.data.iter() {
            let address = *entry.key();
            if let Some((_, versioned)) = entry.value().iter().next_back() {
                result.push((address, versioned.state));
            }
        }
        
        result
    }
}

impl Default for MVHashMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::U256;

    #[test]
    fn test_read_write() {
        let mv = MVHashMap::new();
        let addr = Address::random();
        
        // Read from empty map
        let result = mv.read(addr, 1);
        assert!(matches!(result, ReadResult::Storage));
        
        // Write from transaction 0
        let state0 = AccountState::new(1, U256::from(100));
        mv.write(addr, 0, 0, state0);
        
        // Transaction 1 should see transaction 0's write
        let result = mv.read(addr, 1);
        if let ReadResult::Versioned(version, state) = result {
            assert_eq!(version.txn_idx, 0);
            assert_eq!(state, state0);
        } else {
            panic!("Expected Versioned result");
        }
    }

    #[test]
    fn test_invalidation() {
        let mv = MVHashMap::new();
        let addr = Address::random();
        
        // Transaction 0 writes
        let state0 = AccountState::new(1, U256::from(100));
        mv.write(addr, 0, 0, state0);
        
        // Transaction 2 reads (and we record it)
        let result = mv.read(addr, 2);
        if let ReadResult::Versioned(version, _) = result {
            mv.record_read(addr, 2, version);
        }
        
        // Transaction 1 writes (should invalidate transaction 2)
        let state1 = AccountState::new(2, U256::from(200));
        let write_result = mv.write(addr, 1, 0, state1);
        
        assert_eq!(write_result.invalidated_readers.len(), 1);
        assert_eq!(write_result.invalidated_readers[0], 2);
    }
}

