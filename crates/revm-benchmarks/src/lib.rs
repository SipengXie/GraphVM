// 基准测试库文件
// 这个库主要包含revm基准测试

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use revm::Evm;
    use revm_primitives::{hex, AccountInfo, Address, Bytes, Env, SpecId, TxKind};
    use revm::db::{CacheDB, EmptyDB};

    #[test]
    fn test_execution() {
        // 获取WETH合约字节码
        let bytecode = hex::decode(include_str!("../../../data/weth.rt.hex")).unwrap();
        let bytecode = Bytes::from(bytecode);
        
        // 设置基本参数
        let gas_limit = 1_000_000_000;
        let caller = Address::from([0x1; 20]);
        let contract_addr = Address::from([0x2; 20]);

        // 设置EVM环境
        let mut env = Env::default();
        env.tx.caller = caller;
        env.tx.transact_to = TxKind::Call(contract_addr);
        env.tx.data = Bytes::default();
        env.tx.gas_limit = gas_limit;
        
        // 准备合约和字节码
        let bytecode = revm_interpreter::analysis::to_analysed(revm_primitives::Bytecode::new_raw(bytecode));
        
        // 准备缓存数据库
        let mut cache = CacheDB::new(EmptyDB::default());
        cache.insert_account_info(
            contract_addr,
            AccountInfo {
                code_hash: bytecode.hash_slow(),
                code: Some(bytecode),
                ..Default::default()
            },
        );

        // 创建和执行EVM
        let mut evm = Evm::builder()
            .with_spec_id(SpecId::LATEST)
            .with_ref_db(cache)
            .with_env(Box::new(env))
            .build();
        
        // 执行交易并打印结果
        let result = evm.transact_preverified();
        println!("execution result: {:#?}", result);
    }
} 