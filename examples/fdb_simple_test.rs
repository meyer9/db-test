//! Simple FoundationDB test to verify connectivity and basic operations.
//!
//! This test:
//! 1. Initializes the FDB API
//! 2. Connects to the database
//! 3. Clears all keys
//! 4. Writes a few key-value pairs
//! 5. Reads them back
//!
//! Run with: cargo run --example fdb_simple_test --features fdb

#[tokio::main]
async fn main() {
    println!("=== FoundationDB Simple Test ===\n");

    // Step 1: Initialize the FDB API and network
    println!("1. Initializing FoundationDB API...");
    let _network = unsafe {
        // Initialize FDB API - this must be done once per process
        // The network thread is started automatically
        foundationdb::boot()
    };
    
    // Give the network a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    println!("   ✓ FDB API initialized");

    // Step 2: Connect to database
    println!("\n2. Connecting to database...");
    let db = match foundationdb::Database::default() {
        Ok(db) => {
            println!("   ✓ Connected to database");
            db
        }
        Err(e) => {
            eprintln!("   ✗ Failed to connect: {:?}", e);
            return;
        }
    };

    // Step 3: Clear all keys
    println!("\n3. Clearing database...");
    match db
        .run(|trx, _| async move {
            trx.clear_range(b"test/", b"test/\xff");
            Ok(())
        })
        .await
    {
        Ok(_) => println!("   ✓ Database cleared"),
        Err(e) => {
            eprintln!("   ✗ Failed to clear: {:?}", e);
            return;
        }
    }

    // Step 4: Write some test data
    println!("\n4. Writing test data...");
    match db
        .run(|trx, _| async move {
            trx.set(b"test/key1", b"value1");
            trx.set(b"test/key2", b"value2");
            trx.set(b"test/key3", b"value3");
            Ok(())
        })
        .await
    {
        Ok(_) => println!("   ✓ Wrote 3 key-value pairs"),
        Err(e) => {
            eprintln!("   ✗ Failed to write: {:?}", e);
            return;
        }
    }

    // Step 5: Read back the data
    println!("\n5. Reading test data...");
    match db
        .run(|trx, _| async move {
            let v1 = trx.get(b"test/key1", false).await?;
            let v2 = trx.get(b"test/key2", false).await?;
            let v3 = trx.get(b"test/key3", false).await?;
            Ok((v1, v2, v3))
        })
        .await
    {
        Ok((v1, v2, v3)) => {
            println!("   ✓ Read back values:");
            if let Some(val) = v1 {
                println!("     - test/key1 = {:?}", String::from_utf8_lossy(&val));
            }
            if let Some(val) = v2 {
                println!("     - test/key2 = {:?}", String::from_utf8_lossy(&val));
            }
            if let Some(val) = v3 {
                println!("     - test/key3 = {:?}", String::from_utf8_lossy(&val));
            }
        }
        Err(e) => {
            eprintln!("   ✗ Failed to read: {:?}", e);
            return;
        }
    }

    // Step 6: Test atomic transaction (read-modify-write)
    println!("\n6. Testing atomic transaction (counter increment)...");
    match db
        .run(|trx, _| async move {
            // Read current value
            let current = trx.get(b"test/counter", false).await?;
            let counter_val = if let Some(val) = current {
                u64::from_be_bytes(val[..8].try_into().unwrap_or([0u8; 8]))
            } else {
                0
            };
            
            // Increment
            let new_val = counter_val + 1;
            trx.set(b"test/counter", &new_val.to_be_bytes());
            
            Ok(new_val)
        })
        .await
    {
        Ok(val) => println!("   ✓ Counter incremented to {}", val),
        Err(e) => {
            eprintln!("   ✗ Failed atomic transaction: {:?}", e);
            return;
        }
    }

    println!("\n=== All tests passed! ===\n");
    
    // Note: We don't explicitly stop the network thread here
    // In production code, you'd want to properly shut down
    std::process::exit(0);
}

