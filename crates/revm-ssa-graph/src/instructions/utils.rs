/// Convert U256 value to usize, return usize::MAX if overflow
#[macro_export]
macro_rules! as_usize_saturated {
    ($value:expr) => {{
        let limbs = $value.as_limbs();
        if limbs[1] == 0 && limbs[2] == 0 && limbs[3] == 0 {
            usize::try_from(limbs[0]).unwrap_or(usize::MAX)
        } else {
            usize::MAX
        }
    }};
}

/// Convert U256 value to u64, return u64::MAX if overflow
#[macro_export]
macro_rules! as_u64_saturated {
    ($value:expr) => {{
        let limbs = $value.as_limbs();
        if limbs[1] == 0 && limbs[2] == 0 && limbs[3] == 0 {
            limbs[0]
        } else {
            u64::MAX
        }
    }};
}

/// Convert U256 value to bool, return error if value is not 0 or 1
#[macro_export]
macro_rules! u256_to_bool {
    ($value:expr) => {{
        match $value.try_into() {
            Ok(0) => Ok(false),
            Ok(1) => Ok(true),
            _ => Err(ExecutionError::ExecutionError(
                "Invalid boolean value".to_string()
            )),
        }
    }};
}

/// Macro for extracting value from SSAInput, supporting both Stack and Constant variants
/// Returns the value if input is Stack or Constant, otherwise returns an ExecutionError
#[macro_export]
macro_rules! get_ssa_output_stack_or_const {
    ($graph:expr, $input:expr) => {
        match $input {
            SSAInput::Constant(value) => value,
            SSAInput::Stack(lsn_with_index) => {
                let dep_node = $graph.get_node(lsn_with_index.0)?;
                match dep_node.outputs[lsn_with_index.1 as usize] {
                    SSAOutput::Stack(value) => value,
                    _ => return Err(ExecutionError::ExecutionError(
                        "Expected Stack output value".to_string()
                    ))
                }
            },
            _ => return Err(ExecutionError::ExecutionError(
                "Input must be Stack or Constant value".to_string()
            ))
        }
    };
}

/// Macro for getting storage value from SSAInput::Storage
/// Returns the value if input is valid Storage, otherwise returns an ExecutionError
#[macro_export]
macro_rules! get_storage_value {
    ($graph:expr, $input:expr, $get_state:expr) => {
        match $input {
            SSAInput::Storage (key, source) => {
                if source == (0, 0) {
                    // fetch from the database
                    $get_state(&key)?
                } else {
                    let dep_node = $graph.get_node(source.0)?;
                    match &dep_node.outputs[source.1 as usize] {
                        SSAOutput::Storage { value, .. } => *value.clone(),
                        _ => return Err(ExecutionError::ExecutionError(
                            "Expected Storage output value".to_string()
                        ))
                    }
                }
            },
            _ => return Err(ExecutionError::ExecutionError(
                "Input must be Storage value".to_string()
            ))
        }
    };
}

/// Macro for getting contract environment value from SSAInput::ContractEnv
/// Returns the target_address if input is valid ContractEnv, otherwise returns an ExecutionError
#[macro_export]
macro_rules! get_contract_env {
    ($graph:expr, $input:expr) => {
        match $input {
            SSAInput::ContractEnv((lsn, index)) => {
                let dep_node = $graph.get_node(lsn)?;
                match &dep_node.outputs[index as usize] {
                    SSAOutput::ContractEnv(value) => value,
                    _ => return Err(ExecutionError::ExecutionError(
                        "Expected ContractEnv output value".to_string()
                    ))
                }
            },
            _ => return Err(ExecutionError::ExecutionError(
                "Input must be ContractEnv value".to_string()
            ))
        }
    };
}

/// Macro for getting memory value from SSAInput::Memory
/// Returns a vector of bytes representing the memory state if input is valid Memory,
/// otherwise returns an ExecutionError
#[macro_export]
macro_rules! get_memory {
    ($graph:expr, $input:expr) => {
        match $input {
            SSAInput::Memory(mem_deps) => {
                // Calculate required memory size - find the maximum end position
                let max_size = mem_deps.iter()
                    .map(|dep| dep.self_offset + dep.length)
                    .max()
                    .unwrap_or(0);

                let mut memory = vec![0u8; max_size];

                for dep in mem_deps {
                    let dep_node = $graph.get_node(dep.lsn.0)?;
                    let dep_node_output = &dep_node.outputs[dep.lsn.1 as usize];
                    match dep_node_output {
                        SSAOutput::Memory(value) => {
                            let dst_start = dep.self_offset;
                            let dst_end = dst_start + dep.length;
                            let src_start = dep.lsn_offset;
                            let src_end = src_start + dep.length;
                            // Ensure range is valid
                            if src_end <= value.len() {
                                memory[dst_start..dst_end].copy_from_slice(&value[src_start..src_end]);
                            } else {
                                return Err(ExecutionError::ExecutionError(
                                    format!("Invalid memory range: dst [{},{}], src [{},{}], src len {}",
                                        dst_start, dst_end, src_start, src_end, value.len())
                                ));
                            }
                        },
                        _ => return Err(ExecutionError::ExecutionError(
                            "Expected Memory output value".to_string()
                        ))
                    }
                }

                memory
            },
            _ => return Err(ExecutionError::ExecutionError(
                "Input must be Memory value".to_string()
            ))
        }
    };
}

/// Macro for getting return data buffer from SSAInput::ReturnDataBuffer
/// Returns the buffer value if input is valid ReturnDataBuffer, otherwise returns an ExecutionError
#[macro_export]
macro_rules! get_return_data_buffer {
    ($graph:expr, $input:expr) => {
        match $input {
            SSAInput::ReturnDataBuffer((lsn, index)) => {
                let dep_node = $graph.get_node(lsn)?;
                match &dep_node.outputs[index as usize] {
                    SSAOutput::ReturnDataBuffer(value) => value,
                    _ => return Err(ExecutionError::ExecutionError(
                        "Expected ReturnDataBuffer output value".to_string()
                    ))
                }
            },
            _ => return Err(ExecutionError::ExecutionError(
                "Input must be ReturnDataBuffer value".to_string()
            ))
        }
    };
}

/// Macro for getting call input from SSAInput::CallInput
/// Returns the call input value if input is valid CallInput, otherwise returns an ExecutionError
#[macro_export]
macro_rules! get_call_input {
    ($graph:expr, $input:expr) => {
        match $input {
            SSAInput::CallInput((lsn, index)) => {
                let dep_node = $graph.get_node(lsn)?;
                match &dep_node.outputs[index as usize] {
                    SSAOutput::CallInput(input) => input,
                    _ => return Err(ExecutionError::ExecutionError(
                        "Expected CallInput output value".to_string()
                    ))
                }
            },
            _ => return Err(ExecutionError::ExecutionError(
                "Input must be CallInput value".to_string()
            ))
        }
    };
}

/// Macro for getting interpreter result from SSAInput::InterpreterResult
/// Returns the interpreter result if input is valid InterpreterResult, otherwise returns an ExecutionError
#[macro_export]
macro_rules! get_interpreter_result {
    ($graph:expr, $input:expr) => {
        match $input {
            SSAInput::InterpreterResult((lsn, index)) => {
                let dep_node = $graph.get_node(lsn)?;
                match &dep_node.outputs[index as usize] {
                    SSAOutput::InterpreterResult(result) => result,
                    _ => return Err(ExecutionError::ExecutionError(
                        "Expected InterpreterResult output value".to_string()
                    ))
                }
            },
            _ => return Err(ExecutionError::ExecutionError(
                "Input must be InterpreterResult value".to_string()
            ))
        }
    };
}

/// Re-export macros for convenience
pub use {
    as_u64_saturated, as_usize_saturated, u256_to_bool,
    get_storage_value, get_contract_env, get_memory, get_return_data_buffer, 
    get_call_input, get_interpreter_result
};