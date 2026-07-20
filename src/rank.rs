//! Lexicographic rank for fractional indexing.
//!
//! Backlog order is stored as a string that sorts lexicographically. Because a gap can always be
//! generated between two ranks, reordering an item only rewrites that one item — neighbors keep
//! their ranks, so insertion and movement are O(1) writes.
//!
//! The internal representation is a base-36 (`0-9a-z`) digit string read as the fraction
//! `0.d1 d2 …`. In ASCII the ordering `0`–`9` < `a`–`z` matches the numeric order of the digits, so
//! a plain string comparison is already numeric sort order. Digit values stay in `0..36`, so all
//! internal arithmetic uses `u8`.
//!
//! It is implemented in-house rather than pulling in a crate: in line with the project's
//! minimal-dependency policy, the logic fits in a few dozen lines of `std`-only code
//! (see `docs/DESIGN.md`).

use crate::error::{Error, Result};
use std::fmt;
use std::str::FromStr;

/// Base-36 radix. Since the digit value falls within the range 0 to 35, it is represented by `u8`.
const BASE: u8 = 36;

/// Comparable ranks ordered lexicographically.
///
/// Generated ranks are always in normal form: a non-empty alphanumeric string with no trailing
/// `0`. In that form, lexicographic comparison matches the intended rank order; see
/// [`Rank::parse`] for the validation rules.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rank(String);

impl Rank {
    /// Return the rank string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parse a rank from its string representation.
    ///
    /// Conditions for a valid rank (**normal form**):
    /// - Non-empty and all base-36 alphanumeric characters (`0-9a-z`).
    /// - **Does not end with `0`**. A trailing zero does not change a base-36
    ///   fraction (`"1"` and `"10"` both represent 1/36), which would permit
    ///   multiple strings for the same rank. Disallowing it makes the mapping
    ///   between rank strings and values one-to-one, so lexicographic comparison
    ///   always matches numeric order. Leading and middle zeroes remain valid:
    ///   they contribute to the value (`"01"` ≠ `"1"`).
    /// - This rule also excludes zero (all digits `0`), which is reserved as the
    ///   lower endpoint of the open rank interval.
    ///
    /// If the condition is not met, [`Error::InvalidRank`].
    ///
    /// # Examples
    ///
    /// ```
    /// use pinto::rank::Rank;
    ///
    /// let lower = Rank::parse("i").expect("canonical rank");
    /// let upper = Rank::after(Some(&lower));
    /// assert!(lower < upper);
    /// ```
    pub fn parse(s: &str) -> Result<Rank> {
        let bytes = s.as_bytes();
        let valid = !bytes.is_empty()
            && bytes.iter().all(|&b| digit_value(b).is_some())
            && bytes.last() != Some(&b'0');
        if !valid {
            return Err(Error::InvalidRank(s.to_string()));
        }
        Ok(Rank(s.to_string()))
    }

    /// Return a rank between `lo` and `hi`. `None` represents the open lower or upper endpoint.
    ///
    /// Precondition: `lo < hi` (when both are `Some`). The return value `mid` satisfies `lo < mid < hi`.
    ///
    /// Returns [`Error::InvalidRank`] when the bounds are not in ascending order.
    #[must_use = "handle an invalid rank bound"]
    pub fn between(lo: Option<&Rank>, hi: Option<&Rank>) -> Result<Rank> {
        if matches!((lo, hi), (Some(l), Some(h)) if l >= h) {
            let lower = lo.map_or_else(|| "<open>".to_string(), ToString::to_string);
            let upper = hi.map_or_else(|| "<open>".to_string(), ToString::to_string);
            return Err(Error::InvalidRank(format!(
                "between requires lo < hi (lo={lower:?}, hi={upper:?})"
            )));
        }
        Ok(Self::between_unchecked(lo, hi))
    }

    /// Calculate a midpoint after the caller has established the bounds are valid.
    fn between_unchecked(lo: Option<&Rank>, hi: Option<&Rank>) -> Rank {
        let lo_digits: Vec<u8> = lo.map(|r| digits_of(r.as_str())).unwrap_or_default();
        // Treat `hi = None` as 1.0 and a concrete upper bound as a fraction below 1.0.
        let (hi_int, hi_digits): (u8, Vec<u8>) = match hi {
            Some(r) => (0, digits_of(r.as_str())),
            None => (1, Vec::new()),
        };

        let (int_sum, frac_sum) = add_frac(&lo_digits, &hi_digits, hi_int);
        let mut mid = half_frac(int_sum, &frac_sum);

        // Remove trailing zeroes during normalization; they do not change the represented value.
        while mid.last() == Some(&0) {
            mid.pop();
        }
        Rank(digits_to_string(&mid))
    }

    /// Return the rank immediately following (greater than) `prev`. If `prev` is `None`, return
    /// the first rank.
    ///
    /// Uses tail-add-only logic and does not go through `between(prev, None)` (midpoint to 1.0).
    /// The midpoint method halves the difference from 1.0 on each append, which makes repeated
    /// appends produce increasingly long strings. Instead, compute the shortest rank greater than
    /// `prev` directly:
    ///
    /// - Increase the digits less than `z` (`BASE-1`) by one when looking from the right edge, and truncate the rest.
    ///   It lexicographically exceeds `prev` by that digit, and the number of digits does not increase (often decreases).
    /// - Only when all digits are `z` (`prev ≒ 1.0`), add an intermediate digit (`BASE/2`) to the end to extend it.
    ///
    /// The incremented digits and added intermediate digits are always non-zero, so the result is always in normal form (no trailing zeros).
    #[must_use]
    pub fn after(prev: Option<&Rank>) -> Rank {
        let digits = match prev {
            // First rank: 0.5 = 'i' in base-36 (= BASE/2). Matches between(None, None).
            None => vec![BASE / 2],
            Some(r) => {
                let mut d = digits_of(r.as_str());
                match d.iter().rposition(|&v| v < BASE - 1) {
                    Some(i) => {
                        d[i] += 1;
                        d.truncate(i + 1);
                        d
                    }
                    // All digits are `z`; append an intermediate digit because no increment is possible.
                    None => {
                        d.push(BASE / 2);
                        d
                    }
                }
            }
        };
        Rank(digits_to_string(&digits))
    }

    /// Return the rank immediately preceding (less than) `next`.
    ///
    /// This is a convenience wrapper around `between(None, Some(next))`, matching [`Rank::after`]
    /// so callers do not need to spell out the open lower endpoint.
    /// - `before(Some(n))` always returns a rank less than `n`.
    /// - `before(None)` returns the median rank (base-36 `"i"` = 0.5), matching
    ///   [`Rank::after`] with `None`.
    #[must_use]
    pub fn before(next: Option<&Rank>) -> Rank {
        // `next` is already a valid Rank, and the lower endpoint is open, so these bounds cannot
        // violate `between`'s precondition.
        Self::between_unchecked(None, next)
    }

    /// Generate a short, evenly spaced rank sequence while maintaining the order.
    ///
    /// The smallest fixed width that can hold count canonical ranks is chosen.
    /// Every rank has that width and a non-zero last digit, so it remains in
    /// normal form. The returned sequence is strictly monotonically increasing
    /// and leaves roughly equal gaps between adjacent values.
    ///
    /// If count is zero, the returned vector is empty.
    #[must_use]
    pub fn rebalance(count: usize) -> Vec<Rank> {
        if count == 0 {
            return Vec::new();
        }

        let (width, capacity) = rebalance_layout(count);
        let count_u128 = count as u128;
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            // Select the j-th point at floor(j * capacity / (count + 1)).
            // The open interval leaves one gap at each end.
            let ordinal = ((index as u128 + 1) * capacity) / (count_u128 + 1);
            out.push(rank_at_ordinal(ordinal, width));
        }
        out
    }

    /// Return the fixed rank width [`Rank::rebalance`] would use for `count`
    /// items, without allocating the sequence.
    ///
    /// Callers deciding whether a scope needs rebalancing only need to compare
    /// its current maximum rank length against this width; generating the full
    /// replacement sequence just to measure it is wasted when the scope is left
    /// untouched. Returns `0` for `count == 0`, matching the empty sequence.
    #[must_use]
    pub fn rebalance_width(count: usize) -> usize {
        if count == 0 {
            return 0;
        }
        rebalance_layout(count).0
    }
}

/// Return the minimal fixed width and the number of canonical ranks available
/// at that width.
fn rebalance_layout(count: usize) -> (usize, u128) {
    let target = count as u128;
    let mut width = 1;
    let mut prefix_space = 1u128;
    loop {
        let capacity = prefix_space.saturating_mul(u128::from(BASE - 1));
        if capacity >= target {
            return (width, capacity);
        }
        prefix_space = prefix_space.saturating_mul(u128::from(BASE));
        width += 1;
    }
}

/// Convert an ordinal among fixed-width canonical ranks into a Rank.
///
/// The final digit is selected from 1..=35; the preceding digits enumerate
/// all base-36 prefixes. This covers exactly the canonical strings of a given
/// width without generating a trailing zero.
fn rank_at_ordinal(ordinal: u128, width: usize) -> Rank {
    let last_digit = ordinal % u128::from(BASE - 1) + 1;
    let mut prefix = ordinal / u128::from(BASE - 1);
    let mut digits = vec![0; width];
    for position in (0..width - 1).rev() {
        digits[position] = (prefix % u128::from(BASE)) as u8;
        prefix /= u128::from(BASE);
    }
    digits[width - 1] = last_digit as u8;
    Rank(digits_to_string(&digits))
}

/// Rank-length statistics used to decide whether [`Rank::rebalance`] is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RankStats {
    /// Number of ranks to be aggregated.
    pub count: usize,
    /// Maximum rank length in digits, the main indicator of rank-space expansion.
    pub max_len: usize,
    /// Sum of all rank lengths (used to calculate average).
    pub total_len: usize,
}

impl RankStats {
    /// Traverse the rank column and aggregate statistics.
    pub fn collect<'a, I>(ranks: I) -> RankStats
    where
        I: IntoIterator<Item = &'a Rank>,
    {
        let mut stats = RankStats::default();
        for rank in ranks {
            let len = rank.as_str().len();
            stats.count += 1;
            stats.total_len += len;
            stats.max_len = stats.max_len.max(len);
        }
        stats
    }

    /// Return the average rank length, or `0.0` when no ranks were provided.
    #[must_use]
    pub fn average_len(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.total_len as f64 / self.count as f64
        }
    }

    /// Return whether the maximum rank length exceeds `max_len_threshold`.
    #[must_use]
    pub fn should_rebalance(&self, max_len_threshold: usize) -> bool {
        self.max_len > max_len_threshold
    }
}

impl fmt::Display for Rank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Rank {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Rank::parse(s)
    }
}

/// Convert digit value string to normal form string (assuming `0 <= v < 36`).
fn digits_to_string(digits: &[u8]) -> String {
    digits.iter().map(|&v| value_digit(v) as char).collect()
}

/// A single alphanumeric character to a base-36 digit value (`None` if out of range).
fn digit_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'z' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// Base-36 digit value to one alphanumeric character (assuming `0 <= v < 36`).
fn value_digit(v: u8) -> u8 {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    DIGITS[v as usize]
}

/// Convert regular rank string to digit value string (all digits are valid due to invariant conditions).
fn digits_of(s: &str) -> Vec<u8> {
    s.bytes().filter_map(digit_value).collect()
}

/// Adds the decimal numbers `0.a` and `b_int.b` and returns `(integer part, decimal digits)`.
///
/// Each digit is 0 to 35, so `da + db + carry` has a maximum of 35+35+1=71, which fits in `u8`.
fn add_frac(a: &[u8], b: &[u8], b_int: u8) -> (u8, Vec<u8>) {
    let n = a.len().max(b.len());
    let mut frac = vec![0u8; n];
    let mut carry = 0u8;
    for i in (0..n).rev() {
        let da = a.get(i).copied().unwrap_or(0);
        let db = b.get(i).copied().unwrap_or(0);
        let sum = da + db + carry;
        frac[i] = sum % BASE;
        carry = sum / BASE;
    }
    (b_int + carry, frac)
}

/// Returns the decimal digits of `int.frac` (base-36 decimal) divided by 2.
///
/// The integer part of the result is always 0 in the caller's range (`lo < 1`, `hi <= 1`).
/// `rem * BASE + d` is at most 1*36+35=71 and fits in `u8`.
fn half_frac(int: u8, frac: &[u8]) -> Vec<u8> {
    debug_assert!(int / 2 == 0, "rank value out of expected range");
    let mut out = Vec::with_capacity(frac.len() + 1);
    let mut rem = int % 2;
    for &d in frac {
        let cur = rem * BASE + d;
        out.push(cur / 2);
        rem = cur % 2;
    }
    // If a fraction (0.5 digit) remains, write the next digit as BASE/2 (divisible).
    if rem != 0 {
        out.push(BASE / 2);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn generated_ranks_preserve_canonical_order_and_uniqueness(steps in 1usize..200) {
            let mut ranks = Vec::with_capacity(steps);
            let mut previous = None;
            for _ in 0..steps {
                let next = Rank::after(previous.as_ref());
                prop_assert!(Rank::parse(next.as_str()).is_ok());
                if let Some(previous) = previous {
                    prop_assert!(previous < next);
                }
                ranks.push(next.clone());
                previous = Some(next);
            }
            let unique = ranks.iter().collect::<std::collections::HashSet<_>>();
            prop_assert_eq!(unique.len(), ranks.len());
        }
    }

    fn r(s: &str) -> Rank {
        Rank::parse(s).expect("valid rank")
    }

    #[test]
    fn parse_accepts_base36_and_rejects_others() {
        assert!(Rank::parse("i").is_ok());
        assert!(Rank::parse("0z9a").is_ok());
        assert!(matches!(Rank::parse(""), Err(Error::InvalidRank(_))));
        assert!(matches!(Rank::parse("AB"), Err(Error::InvalidRank(_)))); // Uppercase is invalid.
        assert!(matches!(Rank::parse("a-b"), Err(Error::InvalidRank(_))));
        // The number 0 (all 0 digits) is reserved at the end of the lower open interval and is not a valid rank.
        assert!(matches!(Rank::parse("0"), Err(Error::InvalidRank(_))));
        assert!(matches!(Rank::parse("000"), Err(Error::InvalidRank(_))));
        assert!(Rank::parse("01").is_ok()); // A leading zero contributes to the value and is valid.
    }

    #[test]
    fn parse_rejects_trailing_zero_for_canonical_form() {
        //Rejected because trailing 0 does not contribute to the number and allows multiple representations of the same number.
        // "1" and "10" are both 1/36 and equivalent, but in lexicographical order "1" < "10".
        // "Dictionary order == numerical order" is broken. Normal form does not have trailing zeros.
        assert!(Rank::parse("1").is_ok());
        assert!(matches!(Rank::parse("10"), Err(Error::InvalidRank(_))));
        assert!(matches!(Rank::parse("100"), Err(Error::InvalidRank(_))));
        assert!(matches!(Rank::parse("iz0"), Err(Error::InvalidRank(_))));
        // Normal form (allowed) because the 0 in the middle contributes to the number.
        assert!(Rank::parse("101").is_ok());
    }

    #[test]
    fn generated_ranks_are_always_canonical() {
        // The product of between / after / before is always in normal form that passes through parse.
        let a = Rank::between(None, None).expect("open bounds produce a rank");
        let b = Rank::after(Some(&a));
        let c = Rank::before(Some(&a));
        for g in [&a, &b, &c] {
            assert!(
                Rank::parse(g.as_str()).is_ok(),
                "generated rank must be canonical: {g}"
            );
        }
    }

    #[test]
    fn display_roundtrips() {
        assert_eq!(r("1i").to_string(), "1i");
        assert_eq!("1i".parse::<Rank>().unwrap(), r("1i"));
    }

    #[test]
    fn first_rank_is_deterministic_middle() {
        // Midpoint between 0.0 and 1.0 = 0.5 = 'i'(18) in base-36.
        assert_eq!(
            Rank::between(None, None).expect("open bounds produce a rank"),
            r("i")
        );
    }

    #[test]
    fn after_produces_strictly_greater() {
        let a = Rank::after(None);
        let b = Rank::after(Some(&a));
        let c = Rank::after(Some(&b));
        assert!(a < b, "{a} < {b}");
        assert!(b < c, "{b} < {c}");
    }

    #[test]
    fn before_produces_strictly_smaller() {
        let a = Rank::between(None, None).expect("open bounds produce a rank");
        let b = Rank::between(None, Some(&a)).expect("valid ascending bounds");
        assert!(b < a, "{b} < {a}");
    }

    #[test]
    fn before_api_is_symmetric_to_after() {
        //before(None) is the first rank of the empty sequence (matches after(None)).
        assert_eq!(Rank::before(None), Rank::after(None));
        assert_eq!(Rank::before(None), r("i"));
        // before(Some(n)) is always less than n. Continuous prepend is also strictly monotonically decreasing.
        let mut next = Rank::before(None);
        for _ in 0..50 {
            let prev = Rank::before(Some(&next));
            assert!(prev < next, "{prev} < {next}");
            next = prev;
        }
    }

    #[test]
    fn between_is_strictly_ordered() {
        let lo = r("1");
        let hi = r("2");
        let mid = Rank::between(Some(&lo), Some(&hi)).expect("valid ascending bounds");
        assert!(lo < mid && mid < hi, "{lo} < {mid} < {hi}");
    }

    #[test]
    fn between_rejects_violated_precondition() {
        // lo == hi violates the precondition (lo < hi), in every build profile.
        let r = Rank::between(None, None).expect("open bounds produce a rank"); // "i"
        let error = Rank::between(Some(&r), Some(&r)).expect_err("equal bounds are rejected");
        assert!(matches!(error, Error::InvalidRank(message) if message.contains("lo < hi")));
    }

    #[test]
    fn repeated_bisection_stays_between_and_unique() {
        // Even if you insert it between the same two points 100 times, it will always be strictly monotone and will not conflict.
        let lo = Rank::between(None, None).expect("open bounds produce a rank"); // "i"
        let hi = Rank::after(Some(&lo)); // > lo
        let mut left = lo.clone();
        let mut seen = std::collections::HashSet::new();
        seen.insert(lo.clone());
        seen.insert(hi.clone());
        for n in 0..100 {
            let mid = Rank::between(Some(&left), Some(&hi)).expect("valid ascending bounds");
            assert!(left < mid && mid < hi, "iter {n}: {left} < {mid} < {hi}");
            assert!(seen.insert(mid.clone()), "iter {n}: duplicate {mid}");
            left = mid;
        }
    }

    #[test]
    fn sequential_appends_sort_in_insertion_order() {
        // The ranks assigned with append are arranged in the order of insertion.
        let mut ranks = Vec::new();
        let mut prev: Option<Rank> = None;
        for _ in 0..50 {
            let next = Rank::after(prev.as_ref());
            ranks.push(next.clone());
            prev = Some(next);
        }
        let mut sorted = ranks.clone();
        sorted.sort();
        assert_eq!(ranks, sorted, "append order must equal sorted order");
        // unique.
        let unique: std::collections::HashSet<_> = ranks.iter().collect();
        assert_eq!(unique.len(), ranks.len());
    }

    #[test]
    fn append_growth_is_bounded_over_many_appends() {
        //after() suppresses rank length expansion using tail addition-only logic.
        // The current between(prev, None) (midpoint to 1.0) method halves the gap each time.
        // It grows to ~500 digits with 1,000 appends. Imposing a significantly shorter upper limit as a regression guard.
        let mut prev: Option<Rank> = None;
        let mut ranks: Vec<Rank> = Vec::with_capacity(1000);
        let mut max_len = 0usize;
        for _ in 0..1000 {
            let next = Rank::after(prev.as_ref());
            max_len = max_len.max(next.as_str().len());
            ranks.push(next.clone());
            prev = Some(next);
        }
        // Strictly monotone (order guaranteed) in the order of addition.
        for w in ranks.windows(2) {
            assert!(
                w[0] < w[1],
                "append must be strictly increasing: {} < {}",
                w[0],
                w[1]
            );
        }
        // unique.
        let unique: std::collections::HashSet<_> = ranks.iter().collect();
        assert_eq!(unique.len(), ranks.len(), "appended ranks must be unique");
        // Tail-append-only insertion remains bounded even before an explicit rebalance.
        assert!(
            max_len <= 70,
            "append growth not bounded: max_len = {max_len}"
        );
    }

    #[test]
    fn rebalance_preserves_order_and_shortens_ranks() {
        //If you insert a large amount into the same section, the rank will become bloated.
        let lo = Rank::between(None, None).expect("open bounds produce a rank");
        let hi = Rank::after(Some(&lo));
        let mut left = lo.clone();
        let mut bloated = Vec::new();
        for _ in 0..500 {
            let mid = Rank::between(Some(&left), Some(&hi)).expect("valid ascending bounds");
            bloated.push(mid.clone());
            left = mid;
        }
        let bloated_max = bloated.iter().map(|r| r.as_str().len()).max().unwrap();

        // Rebalance: Generate the same number of short ranks and replace them in the same order.
        let balanced = Rank::rebalance(bloated.len());
        assert_eq!(balanced.len(), bloated.len(), "same cardinality");
        // Narrowly monotone & unique (order preserving).
        for w in balanced.windows(2) {
            assert!(w[0] < w[1], "rebalanced ranks must be sorted");
        }
        let unique: std::collections::HashSet<_> = balanced.iter().collect();
        assert_eq!(
            unique.len(),
            balanced.len(),
            "rebalanced ranks must be unique"
        );
        // Rank length has been significantly shortened.
        let balanced_max = balanced.iter().map(|r| r.as_str().len()).max().unwrap();
        assert!(
            balanced_max < bloated_max,
            "rebalance must shorten: {balanced_max} < {bloated_max}"
        );
        assert!(balanced_max <= 70, "rebalanced max_len = {balanced_max}");
    }

    #[test]
    fn rebalance_zero_is_empty() {
        assert!(Rank::rebalance(0).is_empty());
    }

    #[test]
    fn rebalance_width_matches_generated_width() {
        // The cheap width predicate must agree with the width the full sequence
        // actually occupies, across fixed-width boundaries (35 -> 1 digit, 36 ->
        // 2 digits, 36^2 -> 3 digits).
        for count in [0usize, 1, 2, 35, 36, 37, 1_000, 1_296, 1_297] {
            let generated = RankStats::collect(Rank::rebalance(count).iter()).max_len;
            assert_eq!(
                Rank::rebalance_width(count),
                generated,
                "rebalance_width({count}) must equal the generated max width"
            );
        }
    }

    #[test]
    fn rebalance_uses_minimal_fixed_width_even_spacing() {
        for (count, expected_width) in [(0, 0), (1, 1), (36, 2), (1_000, 2)] {
            let ranks = Rank::rebalance(count);
            let stats = RankStats::collect(ranks.iter());

            assert_eq!(stats.count, count);
            assert_eq!(stats.max_len, expected_width);
            assert_eq!(stats.total_len, count * expected_width);
            assert_eq!(stats.average_len(), expected_width as f64);
            assert!(
                ranks
                    .iter()
                    .all(|rank| rank.as_str().len() == expected_width),
                "all ranks for {count} items must use one fixed width"
            );
            assert!(
                ranks.iter().all(|rank| Rank::parse(rank.as_str()).is_ok()),
                "all rebalanced ranks must remain canonical"
            );
            assert!(
                ranks.windows(2).all(|window| window[0] < window[1]),
                "rebalanced ranks must be strictly increasing"
            );
        }
    }

    #[test]
    fn rank_stats_reports_max_and_average() {
        //Maximum and average rank lengths can be aggregated and used for rebalancing decisions.
        let ranks = vec![r("i"), r("zz"), r("1")]; // Lengths: 1, 2, 1.
        let stats = RankStats::collect(&ranks);
        assert_eq!(stats.count, 3);
        assert_eq!(stats.max_len, 2);
        assert_eq!(stats.total_len, 4);
        assert!((stats.average_len() - 4.0 / 3.0).abs() < 1e-9);
        // Judgment based on threshold.
        assert!(stats.should_rebalance(1), "max_len 2 > 1");
        assert!(!stats.should_rebalance(2), "max_len 2 not > 2");
        // Empty set.
        let empty = RankStats::collect(std::iter::empty::<&Rank>());
        assert_eq!(empty.count, 0);
        assert_eq!(empty.average_len(), 0.0);
        assert!(!empty.should_rebalance(0));
    }

    #[test]
    fn generated_ranks_have_no_trailing_zero() {
        for _ in 0..20 {
            let a = Rank::between(None, None).expect("open bounds produce a rank");
            let b = Rank::between(None, Some(&a)).expect("valid ascending bounds");
            assert_ne!(b.as_str().as_bytes().last(), Some(&b'0'));
        }
    }

    /// Deterministic LCG so the property test needs no external crate and never flakes.
    /// (Numerical Recipes constants; only the high bits are used for better distribution.)
    struct Lcg(u64);
    impl Lcg {
        fn next_usize(&mut self, bound: usize) -> usize {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((self.0 >> 33) as usize) % bound
        }
    }

    #[test]
    fn between_property_holds_for_random_insertions() {
        // Property: inserting a rank between two adjacent, ascending ranks always yields a value
        // strictly between them (`lo < mid < hi`), in canonical form, that never collides with an
        // existing rank. Repeatedly bisecting random gaps stresses `between` far beyond the fixed
        // cases above and guards the self-implemented algorithm against regressions.
        let mut rng = Lcg(0x9E3779B97F4A7C15);
        // Start from three ordered ranks so there is always an interior gap to bisect.
        let mut ranks = vec![Rank::after(None)];
        ranks.push(Rank::after(Some(&ranks[0])));
        ranks.push(Rank::after(Some(&ranks[1])));

        for _ in 0..2_000 {
            // Pick a random adjacent pair and insert between it.
            let i = rng.next_usize(ranks.len() - 1);
            let (lo, hi) = (ranks[i].clone(), ranks[i + 1].clone());
            let mid = Rank::between(Some(&lo), Some(&hi)).expect("valid ascending bounds");

            assert!(lo < mid && mid < hi, "expected {lo} < {mid} < {hi}");
            assert_eq!(
                Rank::parse(mid.as_str()),
                Ok(mid.clone()),
                "{mid} canonical"
            );
            ranks.insert(i + 1, mid);
        }

        // The whole sequence is still strictly increasing and free of duplicates.
        for pair in ranks.windows(2) {
            assert!(pair[0] < pair[1], "order broke: {} !< {}", pair[0], pair[1]);
        }
        let unique: std::collections::BTreeSet<&str> = ranks.iter().map(Rank::as_str).collect();
        assert_eq!(unique.len(), ranks.len(), "ranks must stay unique");
    }
}
