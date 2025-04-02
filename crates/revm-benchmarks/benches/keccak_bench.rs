use criterion::{criterion_group, criterion_main, Criterion};
use revm_primitives::keccak256;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use std::time::Duration;

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("keccak256");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(5));

    // Test different input sizes
    let sizes = [32, 64, 128, 256, 512, 1024, 4096];
    
    for size in sizes {
        // Create a deterministic random number generator
        let mut rng = StdRng::seed_from_u64(42);
        
        // Generate random data
        let mut data = vec![0u8; size];
        rng.fill(&mut data[..]);
        
        group.bench_function(&format!("single_hash_{}", size), |b| {
            let data = data.clone();
            b.iter(|| keccak256(&data))
        });
        
        // 10k hash test
        group.bench_function(&format!("10k_hash_{}", size), |b| {
            let data = data.clone();
            b.iter(|| {
                for _ in 0..10_000 {
                    let _ = keccak256(&data);
                }
            })
        });
    }
    
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .measurement_time(Duration::from_secs(5))
        .warm_up_time(Duration::from_secs(2));
    targets = criterion_benchmark
}

criterion_main!(benches);
