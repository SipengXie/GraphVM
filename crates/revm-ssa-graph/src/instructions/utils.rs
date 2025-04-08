/// Convert U256 value to usize, return usize::MAX if overflow
#[macro_export]
macro_rules! as_usize_saturated {
    ($value:expr) => {{
        let limbs = $value.as_limbs();
        if limbs[1] | limbs[2] | limbs[3] == 0 {
            limbs[0] as usize
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
        if limbs[1] | limbs[2] | limbs[3] == 0 {
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
                ExecutionError::INVALID_BOOLEAN_VALUE.to_string(),
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
                    _ => {
                        return Err(ExecutionError::ExecutionError(
                            ExecutionError::EXPECTED_STACK_VALUE.to_string(),
                        ))
                    }
                }
            }
            _ => {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::INPUT_MUST_BE_STACK_OR_CONST.to_string(),
                ))
            }
        }
    };
}

/// Macro for getting storage value from SSAInput::Storage
/// Returns the value if input is valid Storage, otherwise returns an ExecutionError
#[macro_export]
macro_rules! get_storage_value {
    ($graph:expr, $input:expr, $get_state:expr) => {
        match $input {
            SSAInput::Storage(key, source) => {
                if source == (0, 0) {
                    // fetch from the database
                    $get_state(&key)?
                } else {
                    let dep_node = $graph.get_node(source.0)?;
                    match &dep_node.outputs[source.1 as usize] {
                        SSAOutput::Storage { value, .. } => *value.clone(),
                        _ => {
                            return Err(ExecutionError::ExecutionError(
                                ExecutionError::EXPECTED_STORAGE_VALUE.to_string(),
                            ))
                        }
                    }
                }
            }
            _ => {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::INPUT_MUST_BE_STORAGE_VALUE.to_string(),
                ))
            }
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
                    _ => {
                        return Err(ExecutionError::ExecutionError(
                            ExecutionError::EXPECTED_CONTRACT_ENV_VALUE.to_string(),
                        ))
                    }
                }
            }
            _ => {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::INPUT_MUST_BE_CONTRACT_ENV.to_string(),
                ))
            }
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
                let max_size = mem_deps
                    .iter()
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
                                memory[dst_start..dst_end]
                                    .copy_from_slice(&value[src_start..src_end]);
                            } else {
                                return Err(ExecutionError::ExecutionError(format!(
                                    "Invalid memory range: dst [{},{}], src [{},{}], src len {}",
                                    dst_start,
                                    dst_end,
                                    src_start,
                                    src_end,
                                    value.len()
                                )));
                            }
                        }
                        _ => {
                            return Err(ExecutionError::ExecutionError(
                                ExecutionError::EXPECTED_MEMORY_VALUE.to_string(),
                            ))
                        }
                    }
                }

                memory
            }
            _ => {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::INPUT_MUST_BE_MEMORY_VALUE.to_string(),
                ))
            }
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
                    _ => {
                        return Err(ExecutionError::ExecutionError(
                            ExecutionError::EXPECTED_RETURN_DATA_BUFFER.to_string(),
                        ))
                    }
                }
            }
            _ => {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::INPUT_MUST_BE_RETURN_DATA_BUFFER.to_string(),
                ))
            }
        }
    };
}

/// Macro for getting call input from SSAInput::CallInput
/// Returns the call input value if input is valid CallInput, otherwise returns an ExecutionError
#[macro_export]
macro_rules! get_frame_input {
    ($graph:expr, $input:expr, $first_call_input:expr) => {
        match $input {
            SSAInput::FrameInput((lsn, index)) => {
                if lsn == 0 {
                    &Box::new($first_call_input)
                } else {
                    let dep_node = $graph.get_node(lsn)?;
                    match &dep_node.outputs[index as usize] {
                        SSAOutput::FrameInput(input) => input,
                        _ => {
                            return Err(ExecutionError::ExecutionError(
                                ExecutionError::EXPECTED_CALL_INPUT.to_string(),
                            ))
                        }
                    }
                }
            }
            _ => {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::INPUT_MUST_BE_CALL_INPUT.to_string(),
                ))
            }
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
                    _ => {
                        return Err(ExecutionError::ExecutionError(
                            ExecutionError::EXPECTED_INTERPRETER_RESULT.to_string(),
                        ))
                    }
                }
            }
            _ => {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::INPUT_MUST_BE_INTERPRETER_RESULT.to_string(),
                ))
            }
        }
    };
}

/// Macro for getting gas cost from SSAInput::GasCost
/// Returns the gas cost value if input is valid GasCost, otherwise returns an ExecutionError
#[macro_export]
macro_rules! get_gas_cost {
    ($graph:expr, $input:expr) => {
        match $input {
            SSAInput::GasCost((lsn, index)) => {
                let dep_node = $graph.get_node(lsn)?;
                match &dep_node.outputs[index as usize] {
                    SSAOutput::Gas(gas_cost) => gas_cost,
                    _ => {
                        return Err(ExecutionError::ExecutionError(
                            ExecutionError::EXPECTED_GAS_COST.to_string(),
                        ))
                    }
                }
            }
            _ => {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::EXPECTED_GAS_COST.to_string(),
                ))
            }
        }
    };
}

/// Macro for getting gas refund from SSAInput::GasRefund
/// Returns the gas refund value if input is valid GasRefund, otherwise returns an ExecutionError
#[macro_export]
macro_rules! get_gas_refund {
    ($graph:expr, $input:expr) => {
        match $input {
            SSAInput::GasRefund((lsn, index)) => {
                let dep_node = $graph.get_node(lsn)?;
                match &dep_node.outputs[index as usize] {
                    SSAOutput::GasRefund(gas_refund) => gas_refund,
                    _ => {
                        return Err(ExecutionError::ExecutionError(
                            ExecutionError::EXPECTED_GAS_REFUND.to_string(),
                        ))
                    }
                }
            }
            _ => {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::EXPECTED_GAS_REFUND.to_string(),
                ))
            }
        }
    };
}

#[macro_export]
macro_rules! get_constant_i64 {
    ($graph:expr, $input:expr) => {
        match $input {
            SSAInput::ConstantI64(value) => value,
            _ => {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::EXPECTED_CONSTANT_I64.to_string(),
                ))
            }
        }
    };
}

/// Re-export macros for convenience
pub use {
    as_u64_saturated, as_usize_saturated, get_frame_input, get_constant_i64, get_contract_env,
    get_gas_cost, get_gas_refund, get_interpreter_result, get_memory, get_return_data_buffer,
    get_storage_value, u256_to_bool,
};
