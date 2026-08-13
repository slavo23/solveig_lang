//! SplitMix64 state transitions and runtime-local fresh-seed generation.

/// The odd increment chosen for root generators in Steele, Lea, and Flood,
/// "Fast Splittable Pseudorandom Number Generators", OOPSLA 2014, Fig. 16.
const GOLDEN_GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;
const FULL_U64_WIDTH: u128 = 1_u128 << u64::BITS;

/// Opaque immutable state for one source-level `StdGen` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StdGen(u64);

impl StdGen {
    pub(crate) fn from_seed(seed: i64) -> Self {
        Self(u64::from_ne_bytes(seed.to_ne_bytes()))
    }

    pub(crate) fn next_int(self) -> (i64, Self) {
        let (word, next) = self.next_word();
        (i64::from_ne_bytes(word.to_ne_bytes()), next)
    }

    pub(crate) fn next_range(self, lower: i64, upper: i64) -> Option<(i64, Self)> {
        if lower > upper {
            return None;
        }

        let width = u128::try_from(i128::from(upper) - i128::from(lower) + 1)
            .unwrap_or_else(|_| solveig_base::ice!("valid random range had a negative width"));
        if width == FULL_U64_WIDTH {
            return Some(self.next_int());
        }

        let bound = u64::try_from(width)
            .unwrap_or_else(|_| solveig_base::ice!("bounded random width exceeded u64"));
        Some(self.next_bounded(lower, bound))
    }

    fn next_word(self) -> (u64, Self) {
        let next = Self(self.0.wrapping_add(GOLDEN_GAMMA));
        (mix64(next.0), next)
    }

    fn next_bounded(self, lower: i64, bound: u64) -> (i64, Self) {
        let mut generator = self;
        loop {
            let (word, next) = generator.next_word();
            generator = next;

            // Lemire 2019, "Fast Random Integer Generation in an Interval",
            // Algorithm 5: multiply-high with rejection removes modulo bias.
            let product = u128::from(word) * u128::from(bound);
            let low = u64::try_from(product & u128::from(u64::MAX))
                .unwrap_or_else(|_| solveig_base::ice!("masked random product exceeded u64"));
            if low < bound {
                let threshold = bound.wrapping_neg() % bound;
                if low < threshold {
                    continue;
                }
            }

            let offset = u64::try_from(product >> u64::BITS)
                .unwrap_or_else(|_| solveig_base::ice!("random product high word exceeded u64"));
            let value = i128::from(lower) + i128::from(offset);
            let value = i64::try_from(value)
                .unwrap_or_else(|_| solveig_base::ice!("bounded random result exceeded Int"));
            return (value, generator);
        }
    }
}

/// Produces distinct generator seeds without process-global mutable state.
pub(crate) struct RandomSeeder {
    generator: StdGen,
}

impl RandomSeeder {
    pub(crate) fn from_seed(seed: i64) -> Self {
        Self {
            generator: StdGen::from_seed(seed),
        }
    }

    pub(crate) fn next_generator(&mut self) -> StdGen {
        let (seed, next) = self.generator.next_int();
        self.generator = next;
        StdGen::from_seed(seed)
    }
}

/// The paper's `mix64` permutation from Fig. 16.
fn mix64(mut value: u64) -> u64 {
    value = (value ^ (value >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
    value = (value ^ (value >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    value ^ (value >> 33)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_seed_matches_the_published_splitmix_transition() {
        let mut generator = StdGen::from_seed(0);
        for expected in [
            -7_160_610_219_483_255_062,
            -3_239_489_724_241_199_657,
            -2_886_436_458_091_595_389,
            7_702_497_325_448_497_534,
        ] {
            let (actual, next) = generator.next_int();
            assert_eq!(actual, expected);
            generator = next;
        }
    }

    #[test]
    fn inclusive_ranges_cover_single_and_full_widths() {
        let generator = StdGen::from_seed(7);
        let (single, next) = generator.next_range(4, 4).expect("valid range");
        assert_eq!(single, 4);

        let (full, _) = next.next_range(i64::MIN, i64::MAX).expect("valid range");
        let (expected, _) = next.next_int();
        assert_eq!(full, expected);
        assert!(generator.next_range(1, 0).is_none());
    }
}
