use db_test::{Workload, WorkloadConfig};
use std::collections::HashMap;

fn main() {
    let config = WorkloadConfig {
        num_accounts: 50_000,
        num_transactions: 10_000,
        transactions_per_block: 5_000,
        conflict_factor: 0.0,
        seed: 42,
        chain_id: 1,
    };
    
    let workload = Workload::generate(config);
    
    let mut sender_counts: HashMap<_, usize> = HashMap::new();
    for block in &workload.blocks {
        for tx in block {
            *sender_counts.entry(tx.from).or_insert(0) += 1;
        }
    }
    
    let multi_tx_accounts: Vec<_> = sender_counts.iter()
        .filter(|(_, &count)| count > 1)
        .collect();
    
    println!("Total accounts that sent txs: {}", sender_counts.len());
    println!("Accounts that sent >1 tx: {}", multi_tx_accounts.len());
    println!("Max txs from one account: {}", sender_counts.values().max().unwrap());
    
    let mut hist = HashMap::new();
    for count in sender_counts.values() {
        *hist.entry(*count).or_insert(0) += 1;
    }
    
    println!("\nTransaction count distribution:");
    let mut hist_vec: Vec<_> = hist.iter().collect();
    hist_vec.sort_by_key(|(k, _)| *k);
    for (count, num_accounts) in hist_vec {
        println!("  {} tx: {} accounts", count, num_accounts);
    }
}


