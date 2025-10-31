use revm_primitives::{b256, hex, U256};

// 用于基准测试的代码和输入数据
pub struct BenchCase {
    pub name: &'static str,
    pub bytecode: Vec<u8>,
    pub calldata: Vec<u8>,
    pub pre_determined_slots: Vec<(U256, U256)>,
}

impl BenchCase {
    // 创建一个新的基准测试用例
    pub fn new(name: &'static str, bytecode: &str, calldata: &str) -> Self {
        let bytecode = hex::decode(&bytecode).unwrap_or_default();
        let calldata = if calldata.is_empty() {
            vec![]
        } else {
            hex::decode(&calldata[2..]).unwrap_or_default()
        };
        Self {
            name,
            bytecode,
            calldata,
            pre_determined_slots: vec![],
        }
    }

    pub fn new_with_pre_determined_slots(
        name: &'static str,
        bytecode: &str,
        calldata: &str,
        pre_determined_slots: Vec<(U256, U256)>,
    ) -> Self {
        let bytecode = hex::decode(&bytecode).unwrap_or_default();
        let calldata = if calldata.is_empty() {
            vec![]
        } else {
            hex::decode(&calldata[2..]).unwrap_or_default()
        };
        Self {
            name,
            bytecode,
            calldata,
            pre_determined_slots,
        }
    }
}

// 获取基准测试用例
pub fn get_bench_cases() -> Vec<BenchCase> {
    vec![
        // ERC20测试
        BenchCase::new_with_pre_determined_slots(
            "erc20_runtime", 
            include_str!("../../../../data/erc20_runtime.rt.hex"),
            "0x40c10f1900000000000000000000000001010101010101010101010101010101010101010000000000000000000000000000000000000000000000000000000000010000",
            vec![
                (b256!("d2869508550c71a0ebfe05ddd28ce832b357803f6f387154b1a5451da28aca19").into(), U256::from(10000000000 as u64)),
                (b256!("ac0ab67043ecc9a2f17c6f6ba97786b2b1051a49d0101c2e2da0641d9a0e6da7").into(), U256::from(9900000000 as u64)),
            ]
        ),
        // 斐波那契自定义输入测试
        BenchCase::new(
            "fibonacci_calldata", 
            include_str!("../../../../data/fibonacci_calldata.rt.hex"),
            "0xc6c2ea1700000000000000000000000000000000000000000000000000000000000003e8"
        ),
        // 斐波那契常量输入测试
        BenchCase::new(
            "fibonacci_constant",
            include_str!("../../../../data/fibonacci_constant.rt.hex"),
            "0x9246aa9a"
        ),
        // 阶乘测试自定义输入测试
        BenchCase::new(
            "factorial_calldata", 
            include_str!("../../../../data/factorial_calldata.rt.hex"),
            "0x8371483400000000000000000000000000000000000000000000000000000000000003e8"
        ),
        // 阶乘常量输入测试
        BenchCase::new(
            "factorial_constant",
            include_str!("../../../../data/factorial_constant.rt.hex"),
            "0x981111ef"
        ),
        // Snailtracer测试
        BenchCase::new(
            "snailtracer",
            include_str!("../../../../data/snailtracer.rt.hex"),
            "0x30627b7c"
        ),
        // Hash 10k测试
        BenchCase::new(
            "hash_10k",
            include_str!("../../../../data/hash_10k.rt.hex"),
            "0xdc6bf8a7000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000021234000000000000000000000000000000000000000000000000000000000000"
        ),
        // Uniswap V2测试
        BenchCase::new(
            "uniswap_v2",
            include_str!("../../../../data/uniswap_v2.rt.hex"),
            "0xdfa5235e"
        ),
        // WETH测试
        BenchCase::new(
            "weth",
            include_str!("../../../../data/weth.rt.hex"),
            "0x6b7c477a"
        ),
    ]
}
