use core::cmp::Ordering;
use revm_primitives::{U256, U256_ONE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i8)]
pub enum Sign {
    // same as `cmp::Ordering`
    Minus = -1,
    Zero = 0,
    #[allow(dead_code)] // "constructed" with `mem::transmute` in `i256_sign` below
    Plus = 1,
}

pub const MAX_POSITIVE_VALUE: U256 = U256::from_limbs([
    0xffffffffffffffff,
    0xffffffffffffffff,
    0xffffffffffffffff,
    0x7fffffffffffffff,
]);

pub const MIN_NEGATIVE_VALUE: U256 = U256::from_limbs([
    0x0000000000000000,
    0x0000000000000000,
    0x0000000000000000,
    0x8000000000000000,
]);

const FLIPH_BITMASK_U64: u64 = 0x7FFFFFFFFFFFFFFF;

#[inline(always)]
pub fn i256_sign(val: &U256) -> Sign {
    if val.bit(U256::BITS - 1) {
        Sign::Minus
    } else {
        // SAFETY: false == 0 == Zero, true == 1 == Plus
        unsafe { core::mem::transmute::<bool, Sign>(!val.is_zero()) }
    }
}

#[inline(always)]
pub fn i256_sign_compl(val: &mut U256) -> Sign {
    let sign = i256_sign(val);
    if sign == Sign::Minus {
        two_compl_mut(val);
    }
    sign
}

#[inline(always)]
fn u256_remove_sign(val: &mut U256) {
    // SAFETY: U256 does not have any padding bytes
    unsafe {
        val.as_limbs_mut()[3] &= FLIPH_BITMASK_U64;
    }
}

#[inline(always)]
pub fn two_compl_mut(op: &mut U256) {
    *op = two_compl(*op);
}

#[inline(always)]
pub fn two_compl(op: U256) -> U256 {
    op.wrapping_neg()
}

#[inline(always)]
pub fn i256_cmp(first: &U256, second: &U256) -> Ordering {
    let first_sign = i256_sign(first);
    let second_sign = i256_sign(second);
    match first_sign.cmp(&second_sign) {
        // note: adding `if first_sign != Sign::Zero` to short circuit zero comparisons performs
        // slower on average, as of #582
        Ordering::Equal => first.cmp(second),
        o => o,
    }
}

#[inline(always)]
pub fn i256_div(mut first: U256, mut second: U256) -> U256 {
    let second_sign = i256_sign_compl(&mut second);
    if second_sign == Sign::Zero {
        return U256::ZERO;
    }

    let first_sign = i256_sign_compl(&mut first);
    if first == MIN_NEGATIVE_VALUE && second == U256_ONE {
        return two_compl(MIN_NEGATIVE_VALUE);
    }

    // necessary overflow checks are done above, perform the division
    let mut d = first / second;

    // set sign bit to zero
    u256_remove_sign(&mut d);

    // two's complement only if the signs are different
    // note: this condition has better codegen than an exhaustive match, as of #582
    if (first_sign == Sign::Minus && second_sign != Sign::Minus)
        || (second_sign == Sign::Minus && first_sign != Sign::Minus)
    {
        two_compl(d)
    } else {
        d
    }
}

#[inline(always)]
pub fn i256_mod(mut first: U256, mut second: U256) -> U256 {
    let first_sign = i256_sign_compl(&mut first);
    if first_sign == Sign::Zero {
        return U256::ZERO;
    }

    let second_sign = i256_sign_compl(&mut second);
    if second_sign == Sign::Zero {
        return U256::ZERO;
    }

    let mut r = first % second;

    // set sign bit to zero
    u256_remove_sign(&mut r);

    if first_sign == Sign::Minus {
        two_compl(r)
    } else {
        r
    }
}
