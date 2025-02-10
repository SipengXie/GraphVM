use revm_primitives::U256;


/// Convert usize type value to U256, return U256::MAX if overflow
pub fn as_usize_saturated(value: U256) -> usize {
    let limbs = value.as_limbs();
    if limbs[1] == 0 && limbs[2] == 0 && limbs[3] == 0 {
        usize::try_from(limbs[0]).unwrap_or(usize::MAX)
    } else {
        usize::MAX
    }
}


// pub fn pad_memory_to_word(data: Bytes) -> Bytes {
//     let len = data.len();
//     // If length is less than 32, directly pad to 32
//     if len < 32 {
//         let mut padded = vec![0u8; 32];
//         padded[32-len..].copy_from_slice(&data);
//         return padded.into();
//     }
//     // If length is not a multiple of 32, pad to a multiple of 32
//     let padding_len = (32 - (len % 32)) % 32;
//     if padding_len > 0 {
//         let mut padded = vec![0u8; len + padding_len];
//         padded[padding_len..].copy_from_slice(&data);
//         padded.into()
//     } else {
//         data
//     }
// } 