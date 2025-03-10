use crate::{
    gas, opcode::*, primitives::{Spec, U256}, Host, Interpreter
};
use core::cmp::max;

pub fn mload<H: Host + ?Sized>(interpreter: &mut Interpreter, _host: &mut H) {
    gas!(interpreter, gas::VERYLOW);
    pop_top!(interpreter, top);
    let offset = as_usize_or_fail!(interpreter, top);
    let resized =resize_memory!(interpreter, offset, 32);
    *top = interpreter.shared_memory.get_u256(offset);
    if let Some(logger) = interpreter.ssa_logger.as_mut() {
        let memory_deps = interpreter.shared_memory.get_shadow_deps(offset..offset+32);
        let mem_length = if resized { Some(interpreter.shared_memory.len()) } else { None };
        logger.log_mload_operation(MLOAD, 
            offset, 
            *top, 
            memory_deps,
            mem_length);
    }
}

pub fn mstore<H: Host + ?Sized>(interpreter: &mut Interpreter, _host: &mut H) {
    gas!(interpreter, gas::VERYLOW);
    pop!(interpreter, offset, value);
    let offset = as_usize_or_fail!(interpreter, offset);
    let resized = resize_memory!(interpreter, offset, 32);
    interpreter.shared_memory.set_u256(offset, value);
    if let Some(logger) = interpreter.ssa_logger.as_mut() {
        let mem_length = if resized { Some(interpreter.shared_memory.len()) } else { None };
        let lsn = logger.log_mstore_operation(MSTORE, 
            offset, 
            value, 
            mem_length);
        interpreter.shared_memory.record_shadow_write(offset, 32, (lsn, 0));
        // eprintln!("mem_deps: {:?}", interpreter.shared_memory.get_shadow_deps(offset..offset+32));
    }
}

pub fn mstore8<H: Host + ?Sized>(interpreter: &mut Interpreter, _host: &mut H) {
    gas!(interpreter, gas::VERYLOW);
    pop!(interpreter, offset, value);
    let offset = as_usize_or_fail!(interpreter, offset);
    let resized = resize_memory!(interpreter, offset, 1);
    interpreter.shared_memory.set_byte(offset, value.byte(0));
    if let Some(logger) = interpreter.ssa_logger.as_mut() {
        let mem_length = if resized { Some(interpreter.shared_memory.len()) } else { None };
        let lsn = logger.log_mstore_operation(MSTORE8, 
            offset, 
            value, 
            mem_length);
        interpreter.shared_memory.record_shadow_write(offset, 1, (lsn, 0));
    }
}

pub fn msize<H: Host + ?Sized>(interpreter: &mut Interpreter, _host: &mut H) {
    gas!(interpreter, gas::BASE);
    push!(interpreter, U256::from(interpreter.shared_memory.len()));
    if let Some(logger) = interpreter.ssa_logger.as_mut() {
        logger.log_msize_operation(MSIZE, interpreter.shared_memory.len());
    }
}

// EIP-5656: MCOPY - Memory copying instruction
pub fn mcopy<H: Host + ?Sized, SPEC: Spec>(interpreter: &mut Interpreter, _host: &mut H) {
    check!(interpreter, CANCUN);
    pop!(interpreter, dst, src, len);

    // into usize or fail
    let len = as_usize_or_fail!(interpreter, len);
    // deduce gas
    gas_or_fail!(interpreter, gas::verylowcopy_cost(len as u64));
    if len == 0 {
        return;
    }

    let dst = as_usize_or_fail!(interpreter, dst);
    let src = as_usize_or_fail!(interpreter, src);
    // resize memory
    let resized = resize_memory!(interpreter, max(dst, src), len);
    // copy memory in place
    interpreter.shared_memory.copy(dst, src, len);
    if let Some(logger) = interpreter.ssa_logger.as_mut() {
        // Get memory dependencies for the source region
        let memory_deps = interpreter.shared_memory.get_shadow_deps(src..src+len);
        let mem_length = if resized { Some(interpreter.shared_memory.len()) } else { None };
        let result  = interpreter.shared_memory.slice(src, len).to_vec().into();
        let lsn = logger.log_mcopy_operation(MCOPY, 
            dst, 
            src, 
            len, 
            result, 
            memory_deps,
            mem_length);
        interpreter.shared_memory.record_shadow_write(dst, len, (lsn, 0));
    }
}
