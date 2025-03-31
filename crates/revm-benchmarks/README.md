# Revm SSA 基准测试

这个库提供了与revm的SSA（Static Single Assignment）图执行相关的基准测试。基准测试使用Criterion框架进行测量和报告，并且支持多种不同的测试模式。

## 功能特点

- 支持与传统的非SSA执行进行比较
- 支持串行和并行SSA图执行
- 包含针对不同合约类型的测试（如ERC20、计算密集型操作等）
- 提供并行扩展性测试，可测试不同线程数的性能影响
- 支持不同执行模式（Full、Partial）的性能比较

## 测试案例

当前包含的测试案例：

1. `erc20_runtime` - ERC20代币合约的运行时代码
2. `compute` - 简单计算测试
3. `fibonacci` - 斐波那契序列计算
4. `factorial` - 阶乘计算

## 使用方法

### 运行所有基准测试

```bash
cargo bench
```

### 运行特定的基准测试组

```bash
cargo bench --bench revm_ssa_bench -- simple_test
cargo bench --bench revm_ssa_bench -- revm_ssa
cargo bench --bench revm_ssa_bench -- parallelism_scaling
cargo bench --bench revm_ssa_bench -- execution_modes
```

### 运行特定的基准测试

```bash
cargo bench --bench revm_ssa_bench -- revm_ssa/erc20_runtime
cargo bench --bench revm_ssa_bench -- revm_ssa/erc20_runtime/non_ssa
```

## 结果解释

基准测试结果显示了不同执行策略的性能差异：

1. `non_ssa` - 传统的非SSA方式执行
2. `serial_ssa` - 使用SSA图的串行执行
3. `parallel_ssa` - 使用SSA图的并行执行

对于并行化伸缩性测试，结果显示了不同线程数（1、2、4、8、16）对性能的影响。

## 并行比率

在并行执行时，基准测试会输出一个"并行比率"值，这个值代表了图中可以并行执行的工作比例。值越高意味着并行潜力越大。

## 添加新的测试案例

要添加新的测试案例，请在`get_bench_cases`函数中添加新的`BenchCase`实例：

```rust
BenchCase::new(
    "new_test_name",
    "0x<bytecode_hex>",
    "0x<calldata_hex>"
)
```

## 开发

本基准测试库是基于以下文件创建的：

- `revmc/crates/revmc-cli/benches/bench.rs`
- `revmc/crates/revmc-cli/src/benches.rs`
- `revm/crates/revm/tests/mod.rs`

该库旨在为revm的SSA执行引擎提供全面的性能评估。 