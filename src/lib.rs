//! Benchmarking framework for revm::Database implementations.
//!
//! This crate provides utilities to benchmark different database backends
//! for the revm EVM implementation, focusing on ETH transfer transactions
//! with signature verification.
//!
//! # Architecture
//!
//! The framework is organized around three main concepts:
//!
//! - **Workload**: A pre-generated set of signed transactions and accounts
//! - **Executor**: A strategy for executing transactions (sequential, parallel, etc.)
//! - **Database**: The underlying storage backend (CacheDB, custom implementations)
//!
//! # Quick Start
//!
//! ```
//! use db_test::{Executor, SequentialExecutor, Workload, WorkloadConfig};
//!
//! // Configure the workload
//! let config = WorkloadConfig {
//!     num_accounts: 100,
//!     num_transactions: 50,
//!     conflict_factor: 0.0,
//!     seed: 42,
//!     chain_id: 1,
//! };
//!
//! // Generate workload (signs all transactions upfront)
//! let workload = Workload::generate(config);
//! let db = workload.create_db();
//!
//! // Execute with signature verification
//! let executor = SequentialExecutor::with_verification(true);
//! let (final_db, result) = executor.execute(db, &workload);
//!
//! println!("Successful: {}", result.successful);
//! ```

pub mod executor;

pub use executor::{ExecutionResult, Executor, OrderingMode, SequentialExecutor};

use alloy_primitives::{keccak256, Address, Signature, B256, U256};
use k256::ecdsa::{SigningKey, VerifyingKey};
use rand::{rngs::StdRng, Rng, SeedableRng};
use revm::{
    database::{CacheDB, EmptyDB},
    state::AccountInfo,
};
use std::collections::HashMap;

// ============================================================================
// Account & Key Management
// ============================================================================

/// An account with its signing key for transaction signing.
#[derive(Clone)]
pub struct Account {
    /// The secp256k1 signing key.
    pub signing_key: SigningKey,
    /// The Ethereum address derived from the public key.
    pub address: Address,
}

impl Account {
    /// Creates a new account from a signing key.
    pub fn from_signing_key(signing_key: SigningKey) -> Self {
        let verifying_key = VerifyingKey::from(&signing_key);
        let address = public_key_to_address(&verifying_key);
        Self { signing_key, address }
    }

    /// Generates a deterministic account from a seed.
    pub fn from_seed(seed: u64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut key_bytes = [0u8; 32];
        rng.fill(&mut key_bytes);
        let signing_key = SigningKey::from_bytes(&key_bytes.into())
            .expect("valid key bytes");
        Self::from_signing_key(signing_key)
    }
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("address", &self.address)
            .finish()
    }
}

/// Derives an Ethereum address from a secp256k1 public key.
fn public_key_to_address(verifying_key: &VerifyingKey) -> Address {
    let public_key_bytes = verifying_key.to_encoded_point(false);
    // Skip the 0x04 prefix byte, hash the rest.
    let hash = keccak256(&public_key_bytes.as_bytes()[1..]);
    Address::from_slice(&hash[12..])
}

// ============================================================================
// Signed Transaction
// ============================================================================

/// A signed ETH transfer transaction ready for execution.
#[derive(Debug, Clone)]
pub struct SignedTransaction {
    /// The sender's address (derived from signature during verification).
    pub from: Address,
    /// The recipient's address.
    pub to: Address,
    /// The value to transfer in wei.
    pub value: U256,
    /// The transaction nonce.
    pub nonce: u64,
    /// The ECDSA signature.
    pub signature: Signature,
    /// The hash of the transaction data that was signed.
    pub tx_hash: B256,
}

impl SignedTransaction {
    /// Creates a new signed transaction.
    /// The signature is created over a simplified hash of (from, to, value, nonce, chain_id).
    pub fn new(
        account: &Account,
        to: Address,
        value: U256,
        nonce: u64,
        chain_id: u64,
    ) -> Self {
        let tx_hash = Self::compute_tx_hash(account.address, to, value, nonce, chain_id);
        let signature = Self::sign(&account.signing_key, tx_hash);
        
        Self {
            from: account.address,
            to,
            value,
            nonce,
            signature,
            tx_hash,
        }
    }

    /// Computes the transaction hash for signing.
    fn compute_tx_hash(from: Address, to: Address, value: U256, nonce: u64, chain_id: u64) -> B256 {
        let mut data = Vec::with_capacity(20 + 20 + 32 + 8 + 8);
        data.extend_from_slice(from.as_slice());
        data.extend_from_slice(to.as_slice());
        data.extend_from_slice(&value.to_be_bytes::<32>());
        data.extend_from_slice(&nonce.to_be_bytes());
        data.extend_from_slice(&chain_id.to_be_bytes());
        keccak256(&data)
    }

    /// Signs a transaction hash with the given signing key.
    fn sign(signing_key: &SigningKey, tx_hash: B256) -> Signature {
        let (sig, recovery_id) = signing_key
            .sign_prehash_recoverable(tx_hash.as_slice())
            .expect("signing should succeed");
        
        Signature::from_signature_and_parity(sig, recovery_id.is_y_odd())
    }

    /// Recovers the sender's address from the signature.
    /// Returns None if signature verification fails.
    pub fn recover_signer(&self) -> Option<Address> {
        self.signature
            .recover_address_from_prehash(&self.tx_hash)
            .ok()
    }

    /// Verifies the signature and returns true if valid.
    pub fn verify(&self) -> bool {
        self.recover_signer()
            .map(|addr| addr == self.from)
            .unwrap_or(false)
    }
}

// ============================================================================
// Workload Configuration & Generation
// ============================================================================

/// Configuration for workload generation.
#[derive(Debug, Clone)]
pub struct WorkloadConfig {
    /// Total number of accounts in the system.
    pub num_accounts: usize,
    /// Number of transactions to generate.
    pub num_transactions: usize,
    /// Conflict factor: 0.0 = no conflicts, 1.0 = all transactions touch same accounts.
    pub conflict_factor: f64,
    /// Random seed for reproducibility.
    pub seed: u64,
    /// Chain ID for transaction signing.
    pub chain_id: u64,
}

impl Default for WorkloadConfig {
    fn default() -> Self {
        Self {
            num_accounts: 1000,
            num_transactions: 100,
            conflict_factor: 0.0,
            seed: 42,
            chain_id: 1,
        }
    }
}

/// A complete benchmark workload with pre-generated accounts and signed transactions.
#[derive(Debug, Clone)]
pub struct Workload {
    /// The accounts (with signing keys) participating in this workload.
    pub accounts: Vec<Account>,
    /// The pre-signed transactions to execute.
    pub transactions: Vec<SignedTransaction>,
    /// The configuration used to generate this workload.
    pub config: WorkloadConfig,
}

impl Workload {
    /// Generates a new workload from the given configuration.
    /// All transactions are pre-signed during generation.
    pub fn generate(config: WorkloadConfig) -> Self {
        let mut rng = StdRng::seed_from_u64(config.seed);
        
        // Generate accounts with deterministic keys.
        let accounts: Vec<Account> = (0..config.num_accounts)
            .map(|i| Account::from_seed(config.seed.wrapping_add(i as u64)))
            .collect();

        // Track nonces per account for proper transaction sequencing.
        let mut nonces: HashMap<usize, u64> = HashMap::new();

        // Calculate "hot" account range for conflict simulation.
        let hot_account_count = if config.conflict_factor > 0.0 {
            (2.0 + (1.0 - config.conflict_factor) * (config.num_accounts as f64 - 2.0))
                .max(2.0) as usize
        } else {
            config.num_accounts
        };

        // Generate and sign transactions.
        let transactions: Vec<SignedTransaction> = (0..config.num_transactions)
            .map(|_| {
                let use_hot = rng.gen::<f64>() < config.conflict_factor;

                let (from_idx, to_idx) = if use_hot {
                    let from = rng.gen_range(0..hot_account_count);
                    let mut to = rng.gen_range(0..hot_account_count);
                    while to == from {
                        to = rng.gen_range(0..hot_account_count);
                    }
                    (from, to)
                } else {
                    let from = rng.gen_range(0..config.num_accounts);
                    let mut to = rng.gen_range(0..config.num_accounts);
                    while to == from {
                        to = rng.gen_range(0..config.num_accounts);
                    }
                    (from, to)
                };

                let nonce = nonces.entry(from_idx).or_insert(0);
                let tx = SignedTransaction::new(
                    &accounts[from_idx],
                    accounts[to_idx].address,
                    U256::from(1_000_000_000_000_000u64), // 0.001 ETH
                    *nonce,
                    config.chain_id,
                );
                *nonce += 1;
                tx
            })
            .collect();

        Self {
            accounts,
            transactions,
            config,
        }
    }

    /// Creates a CacheDB pre-funded with all accounts in this workload.
    pub fn create_db(&self) -> CacheDB<EmptyDB> {
        let mut db = CacheDB::new(EmptyDB::default());
        let initial_balance = U256::from(1_000_000_000_000_000_000_000u128); // 1000 ETH

        for account in &self.accounts {
            let info = AccountInfo {
                balance: initial_balance,
                nonce: 0,
                code_hash: revm::primitives::KECCAK_EMPTY,
                code: None,
            };
            db.insert_account_info(account.address, info);
        }

        db
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_generation() {
        let acc1 = Account::from_seed(1);
        let acc2 = Account::from_seed(2);
        
        // Different seeds produce different accounts.
        assert_ne!(acc1.address, acc2.address);
        
        // Same seed produces same account.
        let acc1_copy = Account::from_seed(1);
        assert_eq!(acc1.address, acc1_copy.address);
    }

    #[test]
    fn test_signature_verification() {
        let account = Account::from_seed(42);
        let tx = SignedTransaction::new(
            &account,
            Address::ZERO,
            U256::from(1000),
            0,
            1,
        );

        assert!(tx.verify());
        assert_eq!(tx.recover_signer(), Some(account.address));
    }

    #[test]
    fn test_workload_generation() {
        let config = WorkloadConfig {
            num_accounts: 10,
            num_transactions: 20,
            conflict_factor: 0.0,
            seed: 123,
            chain_id: 1,
        };

        let workload = Workload::generate(config);
        
        assert_eq!(workload.accounts.len(), 10);
        assert_eq!(workload.transactions.len(), 20);

        // All transactions should have valid signatures.
        for tx in &workload.transactions {
            assert!(tx.verify(), "Transaction signature should be valid");
        }
    }
}
