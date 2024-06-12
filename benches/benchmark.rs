extern crate criterion;
use core::fmt;
use std::fmt::Display;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use oram::{BlockValue, CountAccessesDatabase, IndexType, LinearTimeORAM, SimpleDatabase, ORAM};
use rand::{rngs::StdRng, thread_rng, Rng, SeedableRng};

const CAPACITIES_TO_BENCHMARK: [usize; 2] = [64, 256];
const NUM_RANDOM_OPERATIONS_TO_RUN: usize = 64;

// Here, all benchmarks are run for block sizes of 64 and 4096.
criterion_group!(
    benches,
    benchmark_initialization::<64>,
    benchmark_initialization::<4096>,
    benchmark_read::<64>,
    benchmark_read::<4096>,
    benchmark_write::<64>,
    benchmark_write::<4096>,
    benchmark_random_operations::<64>,
    benchmark_random_operations::<4096>,
    print_read_header,
    count_accesses_on_read::<64>,
    count_accesses_on_read::<4096>,
    print_write_header,
    count_accesses_on_write::<64>,
    count_accesses_on_write::<4096>,
    print_random_operations_header,
    count_accesses_on_random_workload::<64>,
    count_accesses_on_random_workload::<4096>,
);
criterion_main!(benches);

fn count_accesses_on_read<const B: usize>(_: &mut Criterion) {
    for capacity in CAPACITIES_TO_BENCHMARK {
        let mut oram: LinearTimeORAM<CountAccessesDatabase<BlockValue<B>>> =
            LinearTimeORAM::new(capacity);
        oram.read(black_box(0));

        let read_count = oram.physical_memory.get_read_count();
        let write_count = oram.physical_memory.get_write_count();

        print_table_row(capacity, B, read_count, write_count);
    }
}

fn count_accesses_on_write<const B: usize>(_: &mut Criterion) {
    for capacity in CAPACITIES_TO_BENCHMARK {
        let mut oram: LinearTimeORAM<CountAccessesDatabase<BlockValue<B>>> =
            LinearTimeORAM::new(capacity);
        oram.write(black_box(0), black_box(BlockValue::default()));

        let read_count = oram.physical_memory.get_read_count();
        let write_count = oram.physical_memory.get_write_count();

        print_table_row(capacity, B, read_count, write_count);
    }
}

fn count_accesses_on_random_workload<const B: usize>(_: &mut Criterion) {
    for capacity in CAPACITIES_TO_BENCHMARK {
        let number_of_operations_to_run = 64usize;

        let mut rng = StdRng::seed_from_u64(0);

        let mut read_versus_write_randomness = vec![false; number_of_operations_to_run];
        rng.fill(&mut read_versus_write_randomness[..]);
        let mut value_randomness = vec![0u8; 4096 * capacity];
        rng.fill(&mut value_randomness[..]);

        let mut index_randomness = vec![0usize; number_of_operations_to_run];
        for i in 0..number_of_operations_to_run {
            index_randomness[i] = rng.gen_range(0..capacity);
        }

        let mut oram: LinearTimeORAM<CountAccessesDatabase<BlockValue<B>>> =
            LinearTimeORAM::new(capacity);
        run_many_random_accesses(
            &mut oram,
            number_of_operations_to_run,
            black_box(&index_randomness),
            black_box(&read_versus_write_randomness),
            black_box(&value_randomness),
        );

        let read_count = oram.physical_memory.get_read_count();
        let write_count = oram.physical_memory.get_write_count();

        print_table_row(capacity, B, read_count, write_count);
    }
}

fn benchmark_initialization<const B: usize>(c: &mut Criterion) {
    let mut group = c.benchmark_group("initialization");
    for capacity in CAPACITIES_TO_BENCHMARK.iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(ReadWriteParameters {
                capacity: *capacity,
                block_size: B,
            }),
            capacity,
            |b, &capacity| {
                b.iter(|| -> LinearTimeORAM<SimpleDatabase<BlockValue<B>>> {
                    LinearTimeORAM::new(capacity)
                })
            },
        );
    }
}

fn benchmark_read<const B: usize>(c: &mut Criterion) {
    let mut group = c.benchmark_group("read");
    for capacity in CAPACITIES_TO_BENCHMARK.iter() {
        let mut oram: LinearTimeORAM<SimpleDatabase<BlockValue<B>>> =
            LinearTimeORAM::new(*capacity);
        group.bench_function(
            BenchmarkId::from_parameter(ReadWriteParameters {
                capacity: *capacity,
                block_size: B,
            }),
            |b| b.iter(|| oram.read(0)),
        );
    }
}

fn benchmark_write<const B: usize>(c: &mut Criterion) {
    let mut group = c.benchmark_group("write");
    for capacity in CAPACITIES_TO_BENCHMARK.iter() {
        let mut oram: LinearTimeORAM<SimpleDatabase<BlockValue<B>>> =
            LinearTimeORAM::new(*capacity);
        group.bench_function(
            BenchmarkId::from_parameter(ReadWriteParameters {
                capacity: *capacity,
                block_size: B,
            }),
            |b| b.iter(|| oram.write(0, BlockValue::default())),
        );
    }
}

fn benchmark_random_operations<const B: usize>(c: &mut Criterion) {
    let mut group = c.benchmark_group("random_operations");

    for capacity in CAPACITIES_TO_BENCHMARK {
        let mut oram: LinearTimeORAM<SimpleDatabase<BlockValue<64>>> =
            LinearTimeORAM::new(capacity);

        // benchmark_random_operations_helper(&mut oram, &mut group);
        let number_of_operations_to_run = 64 as usize;

        let block_size = oram.block_size();
        let capacity: usize = oram.block_capacity();
        let parameters = &RandomOperationsParameters {
            capacity,
            block_size,
            number_of_operations_to_run,
        };

        let mut index_randomness = vec![0usize; number_of_operations_to_run];
        let mut read_versus_write_randomness = vec![false; number_of_operations_to_run];
        let mut value_randomness = vec![0u8; block_size * capacity];
        for i in 0..number_of_operations_to_run {
            index_randomness[i] = thread_rng().gen_range(0..capacity);
        }

        thread_rng().fill(&mut read_versus_write_randomness[..]);
        thread_rng().fill(&mut value_randomness[..]);

        group.bench_with_input(
            BenchmarkId::from_parameter(parameters),
            parameters,
            |b, &parameters| {
                b.iter(|| {
                    run_many_random_accesses(
                        &mut oram,
                        parameters.number_of_operations_to_run,
                        black_box(&index_randomness),
                        black_box(&read_versus_write_randomness),
                        black_box(&value_randomness),
                    )
                })
            },
        );
    }
    group.finish();
}

fn run_many_random_accesses<const B: usize, T: ORAM<B>>(
    oram: &mut T,
    number_of_operations_to_run: usize,
    index_randomness: &[IndexType],
    read_versus_write_randomness: &[bool],
    value_randomness: &[u8],
) -> BlockValue<B> {
    for operation_number in 0..number_of_operations_to_run {
        let random_index = index_randomness[operation_number];
        let random_read_versus_write: bool = read_versus_write_randomness[operation_number];

        if random_read_versus_write {
            oram.read(random_index);
        } else {
            let block_size = oram.block_size();
            let start_index = block_size * random_index;
            let end_index = block_size * (random_index + 1);
            let random_bytes: [u8; B] =
                value_randomness[start_index..end_index].try_into().unwrap();
            oram.write(random_index, BlockValue::from_byte_array(random_bytes));
        }
    }

    BlockValue::default()
}

#[derive(Clone, Copy)]
struct ReadWriteParameters {
    capacity: usize,
    block_size: usize,
}

impl fmt::Display for ReadWriteParameters {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "(Capacity: {} Blocksize: {})",
            self.capacity, self.block_size,
        )
    }
}

#[derive(Clone, Copy)]
struct RandomOperationsParameters {
    capacity: usize,
    block_size: usize,
    number_of_operations_to_run: usize,
}

impl fmt::Display for RandomOperationsParameters {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "(Capacity: {} Blocksize: {}, Ops: {})",
            self.capacity, self.block_size, self.number_of_operations_to_run,
        )
    }
}

fn print_table_row<A: Display, B: Display, C: Display, D: Display>(s1: A, s2: B, s3: C, s4: D) {
    println!("{0: <15} | {1: <15} | {2: <15} | {3: <15}", s1, s2, s3, s4)
}

fn print_read_header(_: &mut Criterion) {
    println!("Physical reads and writes incurred by 1 ORAM read:");
    print_table_header();
}

fn print_write_header(_: &mut Criterion) {
    println!();
    println!("Physical reads and writes incurred by 1 ORAM write:");
    print_table_header();
}

fn print_random_operations_header(_: &mut Criterion) {
    println!();
    println!(
        "Physical reads and writes incurred by {} random ORAM operations:",
        NUM_RANDOM_OPERATIONS_TO_RUN
    );
    print_table_header();
}
fn print_table_header() {
    print_table_row(
        "ORAM Capacity",
        "ORAM Blocksize",
        "Physical Reads",
        "Physical Writes",
    );
}