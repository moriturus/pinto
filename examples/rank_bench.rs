//! Benchmark the rank-generation algorithm and compare the numeric midpoint with a reference
//! fractional-indexing implementation.
//!
//! Criterion is intentionally avoided to keep dependencies small; measurements use
//! `std::time::Instant`. Run with `cargo run --release --example rank_bench`.
//!
//! Results are recorded in `docs/benchmarks.md` and the rank-ordering design in `docs/DESIGN.md`.

use pinto::rank::{Rank, RankStats};
use std::time::Instant;

/// Number of insertions used to evaluate cost and rank-length growth.
const N: usize = 10_000;

fn main() {
    println!("# rank benchmark (N = {N})\n");
    bench_append();
    bench_same_interval();
    compare_algorithms();
}

/// Append N items through the dedicated `Rank::after` path.
fn bench_append() {
    let start = Instant::now();
    let mut ranks = Vec::with_capacity(N);
    let mut prev: Option<Rank> = None;
    for _ in 0..N {
        let next = Rank::after(prev.as_ref());
        prev = Some(next.clone());
        ranks.push(next);
    }
    let elapsed = start.elapsed();
    let stats = RankStats::collect(&ranks);
    println!(
        "append x{N:>6}      : {elapsed:>10.3?}  max_len={:>4}  avg_len={:.2}",
        stats.max_len,
        stats.average_len()
    );
}

/// Insert N items into the same interval, the worst case for `Rank::between`.
fn bench_same_interval() {
    let lo = Rank::between(None, None).expect("open bounds produce a rank");
    let hi = Rank::after(Some(&lo));
    let start = Instant::now();
    let mut ranks = Vec::with_capacity(N);
    let mut left = lo;
    for _ in 0..N {
        let mid = Rank::between(Some(&left), Some(&hi)).expect("valid ascending bounds");
        left = mid.clone();
        ranks.push(mid);
    }
    let elapsed = start.elapsed();
    let stats = RankStats::collect(&ranks);
    println!(
        "same-interval x{N:>6}: {elapsed:>10.3?}  max_len={:>4}  avg_len={:.2}",
        stats.max_len,
        stats.average_len()
    );
}

/// Compare rank-length growth and elapsed time for numeric midpoint (`Rank::between`) and a
/// reference string-based fractional-indexing midpoint when repeatedly inserting into one interval.
fn compare_algorithms() {
    println!("\n## algorithm comparison (same-interval insert x{N})");

    // Numeric midpoint implementation.
    let first = Rank::between(None, None).expect("open bounds produce a rank");
    let hi = Rank::after(Some(&first));
    let start = Instant::now();
    let mut left = first;
    let mut numeric = RankStats::default();
    for _ in 0..N {
        let mid = Rank::between(Some(&left), Some(&hi)).expect("valid ascending bounds");
        accumulate(&mut numeric, mid.as_str().len());
        left = mid;
    }
    let numeric_time = start.elapsed();

    // Reference string-based fractional-indexing midpoint.
    let hi_s = hi.as_str().to_string();
    let start = Instant::now();
    let mut left = Rank::between(None, None)
        .expect("open bounds produce a rank")
        .as_str()
        .to_string();
    let mut fi = RankStats::default();
    for _ in 0..N {
        let mid = fi_midpoint(&left, Some(&hi_s));
        debug_assert!(
            left < mid && mid < hi_s,
            "fi not ordered: {left} < {mid} < {hi_s}"
        );
        accumulate(&mut fi, mid.len());
        left = mid;
    }
    let fi_time = start.elapsed();

    println!(
        "numeric-midpoint   : {numeric_time:>10.3?}  max_len={:>4}  avg_len={:.2}",
        numeric.max_len,
        numeric.average_len()
    );
    println!(
        "fi-string-midpoint : {fi_time:>10.3?}  max_len={:>4}  avg_len={:.2}",
        fi.max_len,
        fi.average_len()
    );
}

/// Add one rank length to `RankStats` (a lightweight helper for comparison loops).
fn accumulate(stats: &mut RankStats, len: usize) {
    stats.count += 1;
    stats.total_len += len;
    stats.max_len = stats.max_len.max(len);
}

const REF_DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// Convert one base-36 character to a digit value (0–35).
fn ref_index(c: u8) -> usize {
    match c {
        b'0'..=b'9' => (c - b'0') as usize,
        b'a'..=b'z' => (c - b'a' + 10) as usize,
        _ => unreachable!("valid rank digit"),
    }
}

/// Reference implementation: an adaptation of David Greenspan's fractional-indexing `midpoint`.
///
/// Assuming `a < b` (`b == None` means no upper bound, or 1.0), return the **shortest key**
/// between them. Inputs and output use the canonical form without trailing zeroes.
fn fi_midpoint(a: &str, b: Option<&str>) -> String {
    match b {
        Some(b) => {
            // Compare the longest common prefix, padding a with zeroes on the right.
            let ab = a.as_bytes();
            let bb = b.as_bytes();
            let mut n = 0;
            while n < bb.len() && ab.get(n).copied().unwrap_or(b'0') == bb[n] {
                n += 1;
            }
            if n > 0 {
                let a_rest = if n < ab.len() { &a[n..] } else { "" };
                return format!("{}{}", &b[..n], fi_midpoint(a_rest, Some(&b[n..])));
            }
            // The leading digits differ.
            let digit_a = if a.is_empty() { 0 } else { ref_index(ab[0]) };
            let digit_b = ref_index(bb[0]);
            if digit_b - digit_a > 1 {
                let mid = (digit_a + digit_b) / 2;
                return (REF_DIGITS[mid] as char).to_string();
            }
            // The leading digits are consecutive.
            if b.len() > 1 {
                b[..1].to_string()
            } else {
                let a_rest = if a.is_empty() { "" } else { &a[1..] };
                format!(
                    "{}{}",
                    REF_DIGITS[digit_a] as char,
                    fi_midpoint(a_rest, None)
                )
            }
        }
        None => {
            let ab = a.as_bytes();
            let digit_a = if a.is_empty() { 0 } else { ref_index(ab[0]) };
            // No upper bound is equivalent to digit value 36.
            if 36 - digit_a > 1 {
                let mid = (digit_a + 36) / 2;
                (REF_DIGITS[mid] as char).to_string()
            } else {
                let a_rest = if a.is_empty() { "" } else { &a[1..] };
                format!(
                    "{}{}",
                    REF_DIGITS[digit_a] as char,
                    fi_midpoint(a_rest, None)
                )
            }
        }
    }
}
