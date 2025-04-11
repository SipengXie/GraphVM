/// Check if an operation involves storage write (including InternalOp)
#[macro_export]
macro_rules! is_storage_write {
    ($op:expr) => {{
        let op_byte = $op as u8;
        // Check InternalOp range
        if (0xD4..=0xDB).contains(&op_byte) {
            matches!(
                op_byte,
                0xD4 |      // MAKE_CREATE_FRAME, modify caller nonce
                0xD5 |      // CREATE_RETURN , modify contract code
                0xD7 |      // MAKE_CALL_FRAME, modify caller and target balance
                0xDA |      // DEDUCT_CALLER, modify caller balance and nonce
                0xDB        // REFUND_GAS, modfiy caller balance
            )
        } else {
            // Standard opcodes
            matches!(
                op_byte,
                0x55 |         // SSTORE
                0xFF           // SELFDESTRUCT
            )
        }
    }};
}
