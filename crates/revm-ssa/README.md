# revm-ssa

SSA (Static Single Assignment) analysis for REVM - A powerful logging and analysis system for EVM execution.

## Table of Contents

- [Introduction](#introduction)
- [Core Features](#core-features)
- [Supported Operations](#supported-operations)
- [Installation](#installation)
- [Usage Examples](#usage-examples)
- [Detailed Documentation](#detailed-documentation)
- [Performance Considerations](#performance-considerations)
- [Testing](#testing)
- [Contributing](#contributing)

## Introduction

revm-ssa is a core component of REVM (Rust Ethereum Virtual Machine) that provides comprehensive tracking and analysis of EVM execution in SSA (Static Single Assignment) form. It maintains detailed operation logs and dependency relationships, offering powerful support for smart contract analysis, optimization, and debugging.

### Key Features

- Complete EVM operation tracking
- Precise dependency analysis
- Detailed storage access tracking
- Efficient frame management
- Rich analysis tools

## Core Features

### SSA Logger Structure

```rust
pub struct SSALogger {
    // Current LSN - increments with each operation
    pub current_lsn: LsnType,
    // Log entries for all operations
    logs: Vec<SSALogEntry>,
    // Shadow stack pool - tracks stack item definitions for different frames
    pub stack_pool: Vec<ShadowStack>,
    // Maps storage slots to their last write LSN
    latest_writes: HashMap<StorageKey, LsnWithIndex>,
    // Maps storage slots to their first read LSN
    origin_reads: HashMap<StorageKey, Vec<LsnType>>,
    // Last LSN that modified memory
    last_memory: LsnWithIndex,
    // Last LSN that modified return data buffer
    last_return_data_buffer: LsnWithIndex,
    // Last LSN that returned from interpreter
    last_interpreter_return: LsnWithIndex,
    // Track contract environment at different levels
    pub contract_env: Vec<LsnWithIndex>,
    // First frame's input
    pub first_frame_input: Option<FrameInput>,
    // Buffers for optimization
    input_buf: Vec<SSAInput>,
    output_buf: Vec<SSAOutput>,
    // Gas tracking
    gas_cost: Vec<(LsnWithIndex, u64)>,
    gas_refund: Vec<(LsnWithIndex, i64)>,
}
```

### Value Types

```rust
pub enum SSAInput {
    Constant(U256),
    ConstantI64(i64),
    Stack(LsnWithIndex),
    Memory(Vec<MemoryDep>),
    Storage(StorageKey, LsnWithIndex),
    Transient(LsnWithIndex),
    ReturnDataBuffer(LsnWithIndex),
    InterpreterResult(LsnWithIndex),
    CallOutcome(LsnWithIndex),
    CreateOutcome(LsnWithIndex),
    MemorySizeChange(LsnWithIndex),
    FrameInput(LsnWithIndex),
    ContractEnv(LsnWithIndex),
    GasCost(LsnWithIndex),
    GasRefund(LsnWithIndex),
}

pub enum SSAOutput {
    Constant(U256),
    Stack(U256),
    Memory(Bytes),
    Storage {
        key: Box<StorageKey>,
        value: Box<StorageValue>,
    },
    Transient(U256),
    Jump(isize),
    ReturnDataBuffer(Bytes),
    InterpreterResult(SSAInterpreterResult),
    MemorySize(usize),
    CreateOutcome(Box<SSACreateOutcome>),
    FrameInput(Box<FrameInput>),
    CallOutcome(Box<SSACallOutcome>),
    Log(Box<Log>),
    ContractEnv(Box<ContractEnv>),
    Gas(u64),
    GasRefund(i64),
}
```

### Log Entry Structure

```rust
pub struct SSALogEntry {
    // The LSN of the log entry
    pub lsn: LsnType,
    // The opcode of the instruction
    pub opcode: u8,
    // The inputs of the instruction
    pub inputs: Vec<SSAInput>,
    // The outputs of the instruction
    pub outputs: Vec<SSAOutput>,
}
```

## Supported Operations

### EVM Operations

#### 1. Stack Operations
- **Push Operations**: PUSH1 to PUSH32 (32 operations)
- **Pop Operations**: POP
- **Duplication**: DUP1 to DUP16 (16 operations)
- **Swap Operations**: SWAP1 to SWAP16 (16 operations)

#### 2. Arithmetic Operations
- **Basic Arithmetic**
  - ADD, MUL, SUB, DIV
  - SDIV, MOD, SMOD
  - ADDMOD, MULMOD
- **Comparison**
  - LT, GT, SLT, SGT, EQ
- **Bitwise Operations**
  - AND, OR, XOR, NOT
  - BYTE, SHL, SHR, SAR

#### 3. Memory Operations
- **Basic Memory**
  - MLOAD: Load word from memory
  - MSTORE: Save word to memory
  - MSTORE8: Save byte to memory
- **Memory Info**
  - MSIZE: Get memory size
- **Memory Expansion**: Automatic tracking

#### 4. Storage Operations
- **Storage Access**
  - SLOAD: Load from storage
  - SSTORE: Save to storage
- **Access Tracking**
  - Storage slot tracking
  - First read/last write tracking

#### 5. Flow Operations
- **Jumps**
  - JUMP: Unconditional jump
  - JUMPI: Conditional jump
- **Execution Control**
  - STOP: Halt execution
  - RETURN: Return data
  - REVERT: Revert state
  - INVALID: Invalid instruction

#### 6. Environmental Operations
- **Block Information**
  - BLOCKHASH, COINBASE
  - TIMESTAMP, NUMBER
  - DIFFICULTY, GASLIMIT
- **Execution Context**
  - ADDRESS, BALANCE
  - ORIGIN, CALLER
  - CALLVALUE
  - CALLDATALOAD, CALLDATASIZE, CALLDATACOPY
  - CODESIZE, CODECOPY
  - EXTCODESIZE, EXTCODECOPY
  - RETURNDATASIZE, RETURNDATACOPY
  - EXTCODEHASH
- **Gas Operations**
  - GAS: Remaining gas
  - GASPRICE: Transaction gas price

#### 7. System Operations
- **Logging**
  - LOG0 to LOG4: Event logging (5 operations)
- **Contract Creation**
  - CREATE: Create new contract
  - CREATE2: Create new contract with salt
- **Contract Calls**
  - CALL: Regular call
  - CALLCODE: Code-only call
  - DELEGATECALL: Delegated call
  - STATICCALL: Static call
- **Self Destruction**
  - SELFDESTRUCT: Destroy contract

#### 8. Cryptographic Operations
- KECCAK256 (SHA3): Compute Keccak-256 hash

### Special Use Cases

#### 1. Contract Analysis
- Creation trace tracking
- Call chain analysis
- State change monitoring
- Dependency tracking between contracts

#### 2. Storage Analysis
- Storage access pattern detection
- Storage conflict identification
- State dependency analysis
- Write/Read sequence tracking

#### 3. Gas Analysis
- Operation cost tracking
- High-cost operation identification
- Optimization suggestions
- Gas usage patterns

#### 4. Security Analysis
- Reentrancy detection
- Access control verification
- Exception handling analysis
- State consistency checking

#### 5. Data Flow Analysis
- Value origin tracking
- Dependency relationship analysis
- Optimization opportunity identification
- Cross-contract data flow

#### 6. Debugging Support
- Detailed execution tracing
- State backtracking
- Error localization
- Stack state inspection

#### 7. Performance Optimization
- Memory management
- Batch operation processing
- Caching strategies
- Resource usage optimization

### Internal Operations

The logger also tracks several internal operations:

1. **Frame Management**
   - Frame creation tracking
   - Frame transition logging
   - Call result recording
   - State preservation

2. **Dependency Tracking**
   - Inter-operation dependencies
   - Cross-frame dependencies
   - Storage dependencies
   - Memory dependencies

3. **State Transitions**
   - Contract creation states
   - Call execution states
   - Return data handling
   - Exception management

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
revm-ssa = "0.1.0"
```

## Usage Examples

### Basic Operation Logging

```rust
use revm_ssa::SSALogger;

// Create new logger
let mut logger = SSALogger::new();

// Log a binary operation (e.g., ADD)
logger.log_binary_operation(
    0x01, // ADD opcode
    first_operand,
    second_operand,
    result
);

// Log a storage operation
logger.log_sstore(
    0x55, // SSTORE opcode
    address,
    key,
    value,
    gas_cost,
    gas_refund
);
```

### Frame Management

```rust
// Create new stack frame
logger.generate_new_stack();

// Log contract call
logger.log_call(
    0xF1, // CALL opcode
    gas_limit,
    address,
    value,
    input_offset,
    input_length,
    output_offset,
    output_length,
    input_data,
    memory_deps,
    memory_length,
    caller,
    target
);

// Remove frame after completion
logger.remove_last_stack();
```

### Gas Tracking

```rust
// Log gas costs
logger.log_gas(0x5A, available_gas);

// Track gas refunds
logger.gas_refund.push((lsn, refund_amount));
```

## Detailed Documentation

### Lifecycle in REVM

1. **Initialization**
```rust
// Basic initialization
let logger = SSALogger::new();

// With specific LSN
let logger = SSALogger::new_with_lsn(start_lsn);
```

2. **Transfer Flow**
```rust
// 1. Logger starts in EVM context
assert!(context.evm.ssa_logger.is_some());

// 2. Transferred to interpreter
context.evm.inner_context_mut().transfer_ssa_logger_to_interpreter(&mut frame.interpreter);

// 3. Used during execution
interpreter.log_operation(...);

// 4. Recovered back to context
context.evm.inner_context_mut().recover_ssa_logger_from_interpreter(&mut frame.interpreter);
```

### API Reference

#### Basic Stack Operations
- `log_push_operation`: PUSH1-PUSH32
- `log_pop_operation`: POP
- `log_dup_operation`: DUP1-DUP16

#### Memory Operations
- `log_mload_operation`: MLOAD
- `log_mstore_operation`: MSTORE/MSTORE8
- `log_msize_operation`: MSIZE

#### Storage Operations
- `log_sload_operation`: SLOAD
- `log_sstore_operation`: SSTORE

#### Contract Operations
- `log_create_opcode`: CREATE/CREATE2
- `log_call_opcode`: CALL/CALLCODE/DELEGATECALL/STATICCALL

#### Environment Information
- `log_gas_operation`: GAS
- `log_balance_operation`: BALANCE
- `log_codesize_operation`: EXTCODESIZE
- `log_codehash_operation`: EXTCODEHASH

#### Other Operations
- `log_keccak256_operation`: KECCAK256/SHA3
- `log_log_operation`: LOG0-LOG4

## Performance Considerations

### Memory Management
- Pre-allocated buffers for inputs and outputs
- Efficient memory copying with padding
- Smart buffer reuse for common operations

### Stack Operations
- Optimized stack manipulation
- Efficient frame management
- Fast access to current stack frame

### Storage Access
- Cached storage access patterns
- Efficient tracking of dependencies
- Optimized storage slot management

## Testing

The crate includes comprehensive tests covering:
- Basic operations
- Complex contract interactions
- Gas calculations
- Memory management
- Storage operations
- Frame handling
- Error conditions

## Contributing

Contributions are welcome! Please ensure:
1. Code follows Rust best practices
2. All tests pass
3. Documentation is updated
4. Performance considerations are maintained