//! ZK-SNARK Benchmark Suite for the Omnia Protocol.
//!
//! Consolidated from zk/benches/zk_benchmarks.rs into the shared
//! omnia-benches crate. Uses omnia-adapters for ZK operations.
//!
//! Benchmarks for:
//! - Poseidon hash (off-chain)
//! - Groth16 proof generation (basic and expanded circuits)
//! - Groth16 proof verification
//! - Merkle tree construction
//! - Trusted setup / key generation

use ark_bn254::Fr;
use ark_ff::PrimeField;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use omnia_adapters::circuit::{ExpandedRollupCircuit, RollupCircuit};
use omnia_adapters::merkle::build_merkle_tree;
use omnia_adapters::poseidon::poseidon_hash_offchain;
use omnia_adapters::prover::{
    create_expanded_proof, create_proof, generate_trusted_setup, generate_trusted_setup_expanded, verify_proof,
};

fn bench_poseidon_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("poseidon_hash");
    let left = Fr::from(42u64);
    let right = Fr::from(99u64);

    group.bench_function("offchain", |b| {
        b.iter(|| {
            let _ = poseidon_hash_offchain(left, right);
        })
    });

    group.finish();
}

fn bench_groth16_proof_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("groth16_proof_generation");

    // Basic circuit
    let circuit = RollupCircuit::empty();
    let (pk, _vk) = generate_trusted_setup(&circuit).expect("setup failed");

    group.bench_function("basic_circuit", |b| {
        b.iter(|| {
            let old = [1u8; 32];
            let new = [2u8; 32];
            let circuit = RollupCircuit::from_state_roots(old, new, 5);
            let _ = create_proof(circuit, &pk);
        })
    });

    // Expanded circuit with different batch sizes
    for &num_events in &[1, 4, 16] {
        let merkle_depth = 8;
        let (pk, _vk) = generate_trusted_setup_expanded(num_events, merkle_depth).expect("expanded setup failed");

        group.bench_with_input(
            BenchmarkId::new("expanded_circuit", num_events),
            &num_events,
            |b, &_num| {
                b.iter(|| {
                    let circuit = ExpandedRollupCircuit::empty(num_events, merkle_depth);
                    let _ = create_expanded_proof(circuit, &pk);
                })
            },
        );
    }

    group.finish();
}

fn bench_groth16_proof_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("groth16_proof_verification");

    let circuit = RollupCircuit::empty();
    let (pk, vk) = generate_trusted_setup(&circuit).expect("setup failed");

    let old = [1u8; 32];
    let new = [2u8; 32];
    let circuit = RollupCircuit::from_state_roots(old, new, 5);
    let proof = create_proof(circuit, &pk).expect("proof failed");
    let public_inputs = vec![Fr::from_be_bytes_mod_order(&new)];

    group.bench_function("single_proof", |b| {
        b.iter(|| {
            let _ = verify_proof(&vk, &public_inputs, &proof);
        })
    });

    group.finish();
}

fn bench_merkle_tree(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_tree");

    for &size in &[8, 64, 256, 1024] {
        let items: Vec<[u8; 32]> = (0..size)
            .map(|i| {
                let mut h = [0u8; 32];
                h[0] = (i % 256) as u8;
                h[1] = ((i >> 8) % 256) as u8;
                h
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("build_tree", size), &size, |b, _| {
            b.iter(|| {
                let _ = build_merkle_tree(&items);
            })
        });
    }

    group.finish();
}

fn bench_key_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("key_generation");

    group.bench_function("trusted_setup_basic", |b| {
        b.iter(|| {
            let circuit = RollupCircuit::empty();
            let _ = generate_trusted_setup(&circuit);
        })
    });

    group.bench_function("trusted_setup_expanded_4_events", |b| {
        b.iter(|| {
            let _ = generate_trusted_setup_expanded(4, 8);
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_poseidon_hash,
    bench_groth16_proof_generation,
    bench_groth16_proof_verification,
    bench_merkle_tree,
    bench_key_generation,
);
criterion_main!(benches);
