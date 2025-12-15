# CLI Usage Guide

The benchmark runner (`db-test`) provides extensive command-line options for customizing benchmark runs.

## Basic Usage

```bash
# Run default benchmarks (sequential executor only)
cargo run --release

# Run all available executors
cargo run --release -- --all

# Run specific executor with custom parameters
cargo run --release --features block-stm -- \
  --block-stm \
  -t 5000 \
  -b 1000 \
  -c 0.0,0.5,1.0 \
  --threads 2,4,8
```

## Command-Line Options

### Workload Configuration

- `-a, --num-accounts <N>` - Number of accounts in the system (default: 50,000)
- `-t, --num-transactions <N>` - Total number of transactions to execute (default: 2,500)
- `-b, --transactions-per-block <N>` - Transactions per block (default: 625)
- `-c, --conflicts <LIST>` - Conflict factors to test, comma-separated (default: 0.0,0.25,0.5,0.75,1.0)
- `--threads <LIST>` - Thread counts for parallel executors (default: 1,2,4,8)

### Executor Selection

- `--sequential` - Enable sequential in-memory executor (default: true)
- `--mdbx-sequential` - Enable MDBX sequential executor (requires `--features mdbx`)
- `--mdbx-batched` - Enable MDBX batched executor (requires `--features mdbx`)
- `--fdb` - Enable FoundationDB parallel executor (requires `--features fdb`)
- `--block-stm` - Enable Block-STM parallel executor (requires `--features block-stm`)
- `--all` - Enable all available executors

### Other Options

- `--no-verify` - Disable signature verification (faster but less realistic)
- `-h, --help` - Print help information
- `-V, --version` - Print version

## Examples

### Quick Test with Small Workload

```bash
cargo run --release -- \
  -t 100 \
  -b 50 \
  -c 0.0,1.0 \
  --sequential
```

### Compare All Executors

Build with all features:
```bash
cargo build --release --all-features
```

Run all executors:
```bash
./target/release/db-test --all
```

### Test Block-STM with Custom Thread Counts

```bash
cargo run --release --features block-stm -- \
  --block-stm \
  --threads 1,4,16 \
  -c 0.0,0.5,1.0
```

### Realistic Blockchain Workload

```bash
cargo run --release --features mdbx -- \
  --mdbx-batched \
  -a 100000 \
  -t 10000 \
  -b 2000 \
  -c 0.0,0.25,0.5
```

### Disable Signature Verification for Speed

```bash
cargo run --release -- \
  --sequential \
  --no-verify \
  -t 10000
```

### Test Only Parallel Executors

```bash
cargo run --release --all-features -- \
  --block-stm \
  --fdb \
  --threads 2,4,8 \
  -c 0.0,0.5,1.0
```

## Output Format

The benchmark runner provides:

1. **Configuration Summary** - Shows selected parameters
2. **Per-Executor Results** - Detailed table with:
   - Conflict level
   - Executor name
   - Ordering mode (strict/loose)
   - Successful/failed transaction counts
   - Execution time (ms)
   - Throughput (TPS)
3. **Summary Statistics** - Average, min, and max TPS per executor

## Feature Flags

Different executors require different feature flags at compile time:

- **No features** - Sequential in-memory executor only
- `--features mdbx` - Adds MDBX sequential and batched executors
- `--features fdb` - Adds FoundationDB parallel executor
- `--features block-stm` - Adds Block-STM parallel executor
- `--all-features` - Enables all executors

## Performance Tips

1. **Always use `--release`** - Debug builds are 10-100x slower
2. **Start small** - Test with `-t 100` first to verify setup
3. **Adjust block size** - Larger blocks reduce overhead for batched executors
4. **Disable verification** - Use `--no-verify` for pure database benchmarks
5. **Isolate tests** - Run one executor at a time for accurate measurements

## Troubleshooting

### Executor Not Available

If you see: `⚠️ X executor not available (rebuild with --features Y)`

Rebuild with the required feature:
```bash
cargo build --release --features Y
```

### Slow Execution

- Reduce transaction count: `-t 500`
- Reduce conflict testing: `-c 0.0,1.0`
- Reduce thread counts: `--threads 1,2`
- Disable verification: `--no-verify`

### Out of Memory

- Reduce account count: `-a 10000`
- Reduce transaction count: `-t 1000`
- Close other applications

