# Parallel REVM Testing Guide

This guide will walk you through setting up and running parallel tests for the Altius-REVM project.

<br>

## Prerequisites

1. Install Rust:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

2. Clone the Altius-REVM repository and checkout the `alpha_release` branch:
```bash
git clone https://github.com/Altius-Labs/revm.git
cd revm
git checkout alpha_release
```

3. Clone the altius-benchtools repository:
```bash
cd ..
git clone https://github.com/Altius-Labs/altius-benchtools.git
```

<br>

## Generating Test Cases

Build the transaction generator:
```bash
cd altius-benchtools
cargo build --release --features generator
```

The generator supports two types of transactions:
  - ETH transfers
  - ERC20 transfers


### ETH Transfer Examples

1. Show the help message and available patterns:

```bash
./target/release/generate pattern --help
```

It will explain the available patterns (one-to-many, many-to-many, chained, etc.) and their parameters.

2. Generate a JSON file with 100 ETH-transfer transactions in 10 groups, using the `one-to-many` pattern:
```bash
mkdir -p ./data     # create a data directory for the test cases
./target/release/generate pattern -y o2m -t 100 -g 10 -o ./data/o2m-100-10.json

# or
./target/release/generate pattern --type o2m --num-transactions 100 --num-groups 10 --output ./data/o2m-100-10.json
```

3. Generate a JSON file with 200 ETH-transfer transactions in 5 groups, using the `chained` pattern:
```bash
./target/release/generate pattern -y chained -t 200 -g 5 -o ./data/chained-200-5.json

# or
./target/release/generate pattern --type chained --num-transactions 200 --num-groups 5 --output ./data/chained-200-5.json
```

4. Generate a JSON file with 100 ETH-transfer transactions with 60% conflict rate, using the `many-to-many` pattern:
```bash
./target/release/generate pattern -y m2m -t 100 -c 0.6 -o ./data/m2m-100-60.json

# or
./target/release/generate pattern --type m2m --num-transactions 100 --conflict-rate 0.6 --output ./data/m2m-100-60.json
```

### ERC20 Transfer Examples

Simply add the `--erc20` flag to generate ERC20 transfer transactions:

```bash
./target/release/generate pattern -y m2m -t 10000 -c 0 -o ./data/m2m-erc20-10k.json --erc20
```

### Test Case Format

The generated test case JSON file contains:
- A list of transactions
- Pre-state of the blockchain
- Environment configuration
- Post-state expectations

Example JSON structure:
```json
{
  "just-test": {
    "_info": { "...": "..." },
    "env": { "...": "..." },
    "post": {
      "Cancun": { "...": "..." }
    },
    "pre": {
      "0xcc2564c36a3440e7d6dd4c67b50f885edbfa5141": {
        "balance": "0x056bc75e2d63100000",
        "code": "0x",
        "nonce": "0x00",
        "storage": {}
      }
    },
    "transaction": [
      {
        "data": "0x",
        "gasLimit": "0x0f4240",
        "gasPrice": "0x0a",
        "nonce": "0x00",
        "secretKey": "0xa119adadef6246ab1780711938aa3b73f86ca408fc2fbbb2fa69135e3ae65c72",
        "sender": "0xcc2564c36a3440e7d6dd4c67b50f885edbfa5141",
        "to": "0xfa3d1fa8d995c05e9fbea98b0f2242391c738625",
        "value": "0x02b5e3af16b1880000"
      }
    ]
  }
}
```

More documentation can be found in the [altius-benchtools](https://github.com/Altius-Labs/altius-benchtools) repository.

<br>

## Running Parallel Tests

1. Return to the Altius-REVM directory and build the `revme` binary:
```bash
cd ../revm
cargo build --release --package revme
export RUST_MIN_STACK=8388608     # Set the minimum stack size in case of stack overflow
```

2. Run parallel tests using the generated transaction data:
```bash
./target/release/revme parallel-test ../altius-benchtools/data/m2m-erc20-10k.json --parallel --num-of-threads 8
```

Or with a custom test file:
```bash
./target/release/revme parallel-test "$file_path" --parallel --num-of-threads 8
```

3. Run with SSA, dependency graph, and prefetch:
```bash
./target/release/revme parallel-test ../altius-benchtools/data/m2m-erc20-10k.json --parallel --num-of-threads 8 --enable-ssa --enable-dep-graph --enable-prefetch
```

The command will run the test in parallel with 8 threads, and enable SSA, dependency graph, and prefetch features.

After the test finishes, you will see the following output:

```bash
========== Running in parallel mode ==========

Finished execute tasks size: 10000 with conflict rate: 0.00%
- prepare time: 3.49486ms
- parallel execution time: 656.595831ms
- sequential execution time: 0ns
- commit time: 237.422364ms

Total time: 905.792896ms
TPS: 11040.051 tx/s

State root: 0x9864bc0646e27947ddcddecfff7d390f866f458dacba2165a7cb021118f34d01
```

4. Run sequential tests and compare with parallel tests:
```bash
./target/release/revme parallel-test ../altius-benchtools/data/m2m-erc20-10k.json --parallel=false
```

After the test finishes, you will see the following output:

```bash
========== Running in sequential mode ==========

Execute time: 2.842444602s
Commit time: 149.205257ms

Total time: 3.381541027s
TPS: 2957.232 tx/s

State root: 0x9864bc0646e27947ddcddecfff7d390f866f458dacba2165a7cb021118f34d01
```

<br>

## Troubleshooting

1. If you encounter build errors:
  - Ensure you have the latest Rust toolchain: `rustup update`
  - Clean and rebuild: `cargo clean && cargo build`

2. If parallel tests fail:
  - Try reducing the number of threads
  - Try reducing the transaction size of the test case
  - Check if the test case JSON is properly formatted
  - Verify that all required dependencies are installed

3. If you continue to encounter issues, please open an issue on our GitHub repository with the following information:
   - A clear description of the problem
   - Steps to reproduce the issue
   - Expected behavior and actual behavior
   - Environment details (Rust version, OS, etc.)
   - Any relevant error messages or logs

   You can create a new issue at: https://github.com/Altius-Labs/revm/issues/new

<br>

## Additional Resources

- [AltiusVM GitHub Repository](https://github.com/Altius-Labs/AltiusVM)
- [altius-benchtools GitHub Repository](https://github.com/Altius-Labs/altius-benchtools)
