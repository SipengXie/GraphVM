use revm_primitives::U256;

use crate::ExecutionError;

/// Convert usize type value to U256, return U256::MAX if overflow
#[inline]
pub fn as_usize_saturated(value: U256) -> usize {
    let limbs = value.as_limbs();
    if limbs[1] == 0 && limbs[2] == 0 && limbs[3] == 0 {
        usize::try_from(limbs[0]).unwrap_or(usize::MAX)
    } else {
        usize::MAX
    }
}

#[inline]
pub fn as_u64_saturated(value: U256) -> u64 {
    let limbs = value.as_limbs();
    if limbs[1] == 0 && limbs[2] == 0 && limbs[3] == 0 {
        limbs[0]
    } else {
        u64::MAX
    }
}

pub fn u256_to_bool(value: U256) -> Result<bool, ExecutionError> {
    match value.try_into() {
        Ok(0) => Ok(false),
        Ok(1) => Ok(true),
        _ => Err(ExecutionError::ExecutionError(
            "Invalid boolean value".to_string()
        )),
    }
}

/// Macro for matching SSAInput to extract value, supporting both Stack and Constant variants
#[macro_export]
macro_rules! match_ssa_output_stack_or_const {
    ($input:expr, $ordinal:expr) => {
        match $input {
            SSAOutput::Stack(value) => value,
            SSAOutput::Constant(value) => value,
            _ => return Err(ExecutionError::ExecutionError(
                format!("{} operand must be Stack or Constant value", $ordinal)
            )),
        }
    };
}

#[macro_export]
macro_rules! match_input {
    ($inputs:expr, $index:expr, $pattern:pat => $result:expr, $err_msg:expr) => {
        match $inputs.get($index) {
            Some($pattern) => $result,
            _ => return Err(ExecutionError::ExecutionError(
                format!("Operand {} must be {}", $index + 1, $err_msg)
            )),
        }
    };
}