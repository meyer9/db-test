//! MDBX database backend executor.
//!
//! This module provides an executor that uses MDBX for persistent storage,
//! with hashed accounts and hashed storage tables similar to Reth's design.

use alloy_primitives::{keccak256, Address, B256, U256};
use eyre::Result;
use reth_db::{mdbx::DatabaseArguments, ClientVersion, DatabaseEnv, DatabaseEnvKind};
use reth_db_api::{
    database::Database,
    table::{DupSort, Table},
    transaction::{DbTx, DbTxMut},
};
use reth_primitives_traits::{Account, StorageEntry};
use std::path::Path;

use super::ExecutionResult;
use crate::Workload;

// ============================================================================
// Table Definitions
// ============================================================================

/// Hashed accounts table - stores account state indexed by keccak256(address).
#[derive(Debug)]
pub struct HashedAccountsTable;

impl Table for HashedAccountsTable {
    const NAME: &'static str = "HashedAccounts";
    const DUPSORT: bool = false;
    type Key = B256;
    type Value = Account;
}

/// Hashed storages table - stores storage values indexed by account hash and storage key hash.
#[derive(Debug)]
pub struct HashedStoragesTable;

impl Table for HashedStoragesTable {
    const NAME: &'static str = "HashedStorages";
    const DUPSORT: bool = true;
    type Key = B256;
    type Value = StorageEntry;
}

impl DupSort for HashedStoragesTable {
    type SubKey = B256;
}

// ============================================================================
// MDBX Database Wrapper
// ============================================================================

/// MDBX database wrapper for EVM execution.
pub struct MdbxDatabase {
    /// The MDBX database environment.
    pub(crate) env: DatabaseEnv,
}

impl MdbxDatabase {
    /// Creates a new MDBX database at the specified path.
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        std::fs::create_dir_all(path)?;

        let args = DatabaseArguments::new(ClientVersion::default());
        let env = DatabaseEnv::open(path, DatabaseEnvKind::RW, args)?;

        // Create tables using low-level API
        {
            let tx = env.begin_rw_txn()?;
            tx.create_db(Some(HashedAccountsTable::NAME), Default::default())?;
            tx.create_db(Some(HashedStoragesTable::NAME), reth_libmdbx::DatabaseFlags::DUP_SORT)?;
            tx.commit()?;
        }

        Ok(Self { env })
    }

    /// Gets an account by its address.
    pub fn get_account(&self, address: Address) -> Result<Option<Account>> {
        let tx = self.env.tx()?;
        let hashed_address = keccak256(address.as_slice());
        Ok(tx.get::<HashedAccountsTable>(hashed_address)?)
    }

    /// Sets an account state.
    pub fn set_account(&self, address: Address, account: Account) -> Result<()> {
        let tx = self.env.tx_mut()?;
        let hashed_address = keccak256(address.as_slice());
        tx.put::<HashedAccountsTable>(hashed_address, account)?;
        tx.commit()?;
        Ok(())
    }

    /// Initializes the database with pre-funded accounts.
    pub fn init_accounts(&self, accounts: &[(Address, U256)]) -> Result<()> {
        let tx = self.env.tx_mut()?;
        
        for &(address, balance) in accounts {
            let hashed_address = keccak256(address.as_slice());
            let account = Account {
                nonce: 0,
                balance,
                bytecode_hash: None,
            };
            tx.put::<HashedAccountsTable>(hashed_address, account)?;
        }
        
        tx.commit()?;
        Ok(())
    }
}

// ============================================================================
// MDBX Executor Implementation
// ============================================================================

/// MDBX-backed sequential executor that processes transactions with persistent storage.
///
/// This executor processes transactions one at a time in strict order, using MDBX
/// for persistent storage with hashed account and storage tables.
///
/// # Example
///
/// ```ignore
/// use db_test::executor::MdbxSequentialExecutor;
/// use tempfile::tempdir;
///
/// let dir = tempdir().unwrap();
/// let executor = MdbxSequentialExecutor::new(dir.path(), true).unwrap();
/// ```
pub struct MdbxSequentialExecutor {
    db: MdbxDatabase,
    verify_signatures: bool,
}

impl MdbxSequentialExecutor {
    /// Creates a new MDBX sequential executor.
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

    /// Executes a workload on the MDBX database.
    pub fn execute_workload(&self, workload: &Workload) -> Result<(ExecutionResult, ())> {
        // Initialize accounts
        let accounts: Vec<_> = workload
            .accounts
            .iter()
            .map(|acc| (acc.address, U256::from(1_000_000_000_000_000_000_000u128)))
            .collect();
        
        self.db.init_accounts(&accounts)?;

        // Execute transactions
        let mut successful = 0;
        let mut failed = 0;

        for tx in &workload.transactions {
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

            // Get sender account
            let mut sender = match self.db.get_account(tx.from)? {
                Some(acc) => acc,
                None => {
                    failed += 1;
                    continue;
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

            // Get receiver account or create new one
            let mut receiver = self.db.get_account(tx.to)?.unwrap_or(Account {
                nonce: 0,
                balance: U256::ZERO,
                bytecode_hash: None,
            });

            // Execute transfer
            sender.balance -= tx.value;
            sender.nonce += 1;
            receiver.balance += tx.value;

            // Write back to database
            self.db.set_account(tx.from, sender)?;
            self.db.set_account(tx.to, receiver)?;

            successful += 1;
        }

        Ok((ExecutionResult::new(successful, failed), ()))
    }
}

// Note: MdbxSequentialExecutor does not implement the Executor trait directly
// because it doesn't use the standard Database type. Instead, it provides
// execute_workload() which returns the same ExecutionResult type.
//
// To use it in benchmarks, wrap calls in a similar pattern to SequentialExecutor.

impl MdbxSequentialExecutor {
    /// Returns whether this executor preserves transaction ordering.
    pub fn preserves_order(&self) -> bool {
        true
    }

    /// Returns the name of this executor.
    pub fn name(&self) -> &'static str {
        "mdbx_sequential"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkloadConfig;
    use tempfile::tempdir;

    #[test]
    fn test_mdbx_database_creation() {
        let dir = tempdir().unwrap();
        let db = MdbxDatabase::create(dir.path()).unwrap();

        // Test account operations with a deterministic address
        let addr = Address::with_last_byte(42);
        let account = Account {
            nonce: 1,
            balance: U256::from(1000),
            bytecode_hash: None,
        };

        db.set_account(addr, account).unwrap();
        let retrieved = db.get_account(addr).unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().nonce, 1);
    }

    #[test]
    fn test_mdbx_sequential_executor() {
        let dir = tempdir().unwrap();
        let config = WorkloadConfig {
            num_accounts: 10,
            num_transactions: 5,
            conflict_factor: 0.0,
            seed: 42,
            chain_id: 1,
            transactions_per_block: 5,
        };

        let workload = Workload::generate(config);
        let executor = MdbxSequentialExecutor::new(dir.path(), true).unwrap();

        let (result, _) = executor.execute_workload(&workload).unwrap();
        assert_eq!(result.successful, 5);
        assert_eq!(result.failed, 0);
        assert!(executor.preserves_order());
        assert_eq!(executor.name(), "mdbx_sequential");
    }
}
