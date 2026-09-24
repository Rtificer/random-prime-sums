use std::io;
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::thread;

// v1. Should be faster for small n
//
// struct BitField {
//     data: Box<[u64]>,
//     len: usize,
// }
//
// impl BitField {
//     fn new(len: usize) -> Self {
//         Self {
//             data: vec![0; len.div_ceil(64)].into_boxed_slice(),
//             len,
//         }
//     }
//
//     fn get(&self, i: usize) -> bool {
//         debug_assert!(i < self.len);
//         self.data[i / 64] & (1 << (i % 64)) != 0
//     }
//
//     fn set(&mut self, i: usize, value: bool) {
//         debug_assert!(i < self.len);
//
//         let word = &mut self.data[i / 64];
//         let mask = 1u64 << (i % 64);
//
//         if value {
//             *word |= mask;
//         } else {
//             *word &= !mask;
//         }
//     }
// }
//
// fn find_sum(field: &mut BitField, num: usize) -> usize {
//     let mut multiple = 4;
//     while multiple <= num {
//         field.set(multiple, true);
//         multiple += 2;
//     }
//
//     let mut prime = 3usize;
//     while prime * prime <= num {
//         if !field.get(prime) {
//             let mut multiple = prime * prime;
//             while multiple <= num {
//                 field.set(multiple, true);
//                 multiple += prime;
//             }
//         }
//         prime += 2;
//     }
//
//     (3..=num).step_by(2).filter(|&i| !field.get(i)).sum()
// }
//
//
//fn main() {
//    let mut buffer = String::new();
//    let stdin = io::stdin();
//
//    loop {
//        buffer.clear();
//        println!("Input number to find sum of primes up to (Use Ctrl+C to quit):");
//        if let Err(e) = stdin.read_line(&mut buffer) {
//            eprintln!("Failed to read terminal: {e}");
//            continue;
//        }
//
//        let num = match buffer.trim().parse() {
//            Ok(n) => n,
//            Err(e) => {
//                eprintln!("Failed to convert input to usize: {e}");
//                continue;
//            }
//        };
//
//        let mut tracking_field = BitField::new(num + 1);
//
//        let start = std::time::Instant::now();
//        println!(
//            "Sum: {} (took {:?})",
//            find_sum(&mut tracking_field, num),
//            start.elapsed()
//        );
//    }
//}

struct BitField {
    data: Vec<u64>,
    len: usize,
}

impl BitField {
    fn new(len: usize) -> Self {
        Self {
            data: vec![0; len.div_ceil(64)],
            len,
        }
    }

    fn get(&self, i: usize) -> bool {
        debug_assert!(i < self.len);
        self.data[i / 64] & (1 << (i % 64)) != 0
    }

    fn set(&mut self, i: usize, value: bool) {
        debug_assert!(i < self.len);
        let word = &mut self.data[i / 64];
        let mask = 1u64 << (i % 64);
        if value {
            *word |= mask;
        } else {
            *word &= !mask;
        }
    }

    fn reset(&mut self, new_len: usize) {
        let words_needed = new_len.div_ceil(64);
        if self.data.len() < words_needed {
            self.data.resize(words_needed, 0)
        } else {
            self.data[..words_needed].fill(0);
        }
        self.len = new_len;
    }
}

fn estimate_prime_count(limit: usize) -> usize {
    if limit < 2 {
        return 0;
    }
    let count_estimate = (limit as f64) / ((limit as f64).ln() - 1.0);
    count_estimate as usize + 16
}

fn estimate_base_primes_bytes(limit: usize) -> usize {
    estimate_prime_count(limit) * std::mem::size_of::<usize>()
}

const MIN_SEGMENT_SIZE: NonZeroUsize = NonZeroUsize::new(1 << 16).unwrap();
const MAX_SEGMENT_SIZE: NonZeroUsize = NonZeroUsize::new(1 << 22).unwrap();

fn choose_segment_size(
    num: NonZeroUsize,
    max_memory_bytes: NonZeroUsize,
    num_threads: NonZeroUsize,
) -> NonZeroUsize {
    let sqrt_n = num.get().isqrt() + 1;
    let base_prime_bytes = estimate_base_primes_bytes(sqrt_n);

    let remaining = max_memory_bytes.get().saturating_sub(base_prime_bytes);
    let per_thread_budget = remaining / num_threads.get();

    let segment_size =
        (per_thread_budget * 8).clamp(MIN_SEGMENT_SIZE.get(), MAX_SEGMENT_SIZE.get());

    NonZeroUsize::new(segment_size).unwrap()
}

fn to_bit_index(num: usize) -> usize {
    debug_assert!(num % 2 == 1);
    num / 2
}

fn from_bit_index(i: usize) -> usize {
    2 * i + 1
}

fn small_primes(limit: usize) -> Vec<usize> {
    if limit < 2 {
        return vec![];
    }

    let odd_count = (limit / 2) + 1;
    let mut is_composite = BitField::new(odd_count);

    is_composite.set(to_bit_index(1), true);

    let mut prime = 3;
    while prime * prime <= limit {
        if !is_composite.get(to_bit_index(prime)) {
            let mut multiple = prime * prime;
            while multiple <= limit {
                is_composite.set(to_bit_index(multiple), true);
                multiple += 2 * prime;
            }
        }
        prime += 2;
    }

    let mut primes = Vec::with_capacity(estimate_prime_count(limit));
    primes.push(2);
    primes.extend(
        (1..odd_count)
            .map(from_bit_index)
            .filter(|&num| num <= limit && !is_composite.get(to_bit_index(num))),
    );
    primes
}

fn segmented_sum_region(
    low_start: usize,
    high_end: usize,
    segment_size: NonZeroUsize,
    base_primes: &[usize],
) -> usize {
    let mut total = 0;
    let mut low = low_start;

    let mut is_composite = BitField::new(segment_size.get());

    while low <= high_end {
        let high = (low + segment_size.get() - 1).min(high_end);
        let len = high - low + 1;
        is_composite.reset(len);

        for &prime in base_primes {
            let start = low.div_ceil(prime).max(prime) * prime;

            let mut multiple = start;
            while multiple <= high {
                is_composite.set(multiple - low, true);
                multiple += prime;
            }
        }

        total += (0..len)
            .filter(|&i| !is_composite.get(i))
            .map(|i| low + i)
            .sum::<usize>();

        low += segment_size.get();
    }

    total
}

fn segmented_sum_parellel(
    num: NonZeroUsize,
    max_memory_bytes: NonZeroUsize,
    num_threads: NonZeroUsize,
) -> usize {
    if num.get() < 2 {
        return 0;
    }

    let sqrt_num = num.get().isqrt() + 1;
    let base_primes = small_primes(sqrt_num);
    let segment_size = choose_segment_size(num, max_memory_bytes, num_threads);

    let mut total: usize = base_primes
        .iter()
        .filter(|&&prime| prime <= num.get())
        .sum();

    let region_start = sqrt_num + 1;
    if region_start > num.get() {
        return total;
    }
    let region_len = num.get() - region_start + 1;
    let per_thread = region_len.div_ceil(num_threads.get());

    let region_sums: Vec<usize> = thread::scope(|scope| {
        let mut handles = Vec::new();
        let mut low = region_start;

        while low <= num.get() {
            let high = (low + per_thread - 1).min(num.get());
            let base_primes_ref = &base_primes;
            handles.push(
                scope.spawn(move || segmented_sum_region(low, high, segment_size, base_primes_ref)),
            );

            low += per_thread;
        }

        handles
            .into_iter()
            .map(|h| h.join().expect("failed to join thread handles"))
            .collect()
    });

    total += region_sums.into_iter().sum::<usize>();
    total
}

fn prompt_parse<T: FromStr>(stdin: &io::Stdin, buffer: &mut String, prompt: &str) -> T
where
    T::Err: std::fmt::Display,
{
    loop {
        buffer.clear();
        println!("{prompt}");
        if let Err(e) = stdin.read_line(buffer) {
            eprintln!("failed to read terminal: {e}");
            continue;
        }

        match buffer.trim().parse::<T>() {
            Ok(n) => return n,
            Err(e) => eprintln!("failed to parse input: {e}"),
        }
    }
}

fn main() {
    let mut buffer = String::new();
    let stdin = io::stdin();

    let max_mem_mib: NonZeroUsize = prompt_parse(
        &stdin,
        &mut buffer,
        "Input maximum memory allocation (MiB): ",
    );
    let max_mem_bytes = NonZeroUsize::new(max_mem_mib.get() << 20)
        .expect("MiB value too large, overflowed to zero");

    let thread_count = loop {
        buffer.clear();
        println!("Input thread count. Leave blank to use all available threads: ");
        if let Err(e) = stdin.read_line(&mut buffer) {
            eprintln!("failed to read terminal: {e}");
            continue;
        }

        match buffer.trim() {
            "" => match thread::available_parallelism() {
                Ok(threads) => break threads,
                Err(e) => eprintln!("Failed to get available threads: {e}"),
            },
            threads_str => match threads_str.parse::<NonZeroUsize>() {
                Ok(threads) => break threads,
                Err(e) => eprintln!("Failed to convert input to NonZeroUsize: {e}"),
            },
        }
    };

    loop {
        let num: NonZeroUsize = prompt_parse(
            &stdin,
            &mut buffer,
            "Input number to find sum of primes up to (Use Ctrl+C to quit): ",
        );

        let start = std::time::Instant::now();
        println!(
            "Sum: {} (took {:?})",
            segmented_sum_parellel(num, max_mem_bytes, thread_count),
            start.elapsed()
        );
    }
}
