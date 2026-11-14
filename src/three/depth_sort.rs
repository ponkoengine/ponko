//! Sorting see-through things far to near: keys of a 32-bit depth (in
//! the upper half, already ordered so that smaller is drawn first) and a
//! 32-bit index (the lower half, ascending as pushed). Particles and
//! see-through instance batches sort up to millions of them a frame.

/// Below this many keys a comparison sort is faster than the radix passes.
const RADIX_MIN: usize = 2048;
/// Bits a radix pass takes: three passes cover the 32-bit depth.
const DIGIT: u32 = 11;
const BINS: usize = 1 << DIGIT;

/// Sorts `keys` by their upper 32 bits, and equal ones by their lower 32
/// bits, given that the lower bits ascend as `keys` holds them (each key's
/// index): the order `keys.sort_unstable()` gives. Large lists take a
/// stable radix sort over the upper bits (three passes), which keeps that
/// index order; `scratch` is its second buffer, kept by the caller so a
/// frame allocates nothing once it is big enough.
pub(crate) fn sort_keys(keys: &mut Vec<u64>, scratch: &mut Vec<u64>) {
    if keys.len() < RADIX_MIN {
        keys.sort_unstable();
        return;
    }
    let mut counts = [[0u32; BINS]; 3];
    for &key in keys.iter() {
        for (pass, count) in counts.iter_mut().enumerate() {
            count[digit(key, pass)] += 1;
        }
    }
    scratch.clear();
    scratch.resize(keys.len(), 0);
    for (pass, count) in counts.iter_mut().enumerate() {
        // A digit every key shares moves nothing.
        if count.contains(&(keys.len() as u32)) {
            continue;
        }
        let mut at = 0;
        for c in count.iter_mut() {
            (*c, at) = (at, at + *c);
        }
        for &key in keys.iter() {
            let bin = &mut count[digit(key, pass)];
            scratch[*bin as usize] = key;
            *bin += 1;
        }
        std::mem::swap(keys, scratch);
    }
}

/// Radix pass `pass`'s digit of `key`'s upper 32 bits.
fn digit(key: u64, pass: usize) -> usize {
    ((key >> 32 >> (pass as u32 * DIGIT)) as usize) & (BINS - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(count: usize, spread: u64) -> Vec<u64> {
        let mut rng = crate::random::Rng::new(5);
        (0..count as u64).map(|i| (rng.u64() % spread) << 32 | i).collect()
    }

    #[test]
    fn radix_and_comparison_sorts_agree_with_ties_in_index_order() {
        for (count, spread) in [(10, 3), (5000, 7), (5000, u64::from(u32::MAX)), (70_000, 1 << 20)] {
            let mut sorted = keys(count, spread);
            let mut expected = sorted.clone();
            expected.sort_unstable();
            sort_keys(&mut sorted, &mut Vec::new());
            assert_eq!(sorted, expected, "{count} keys over {spread} depths");
        }
    }

    #[test]
    fn sorting_again_allocates_nothing() {
        let mut sorted = keys(10_000, 1 << 30);
        let mut scratch = Vec::new();
        sort_keys(&mut sorted.clone(), &mut scratch);
        let ((), allocations) = crate::three::lights::tests::count_allocations(|| sort_keys(&mut sorted, &mut scratch));
        assert_eq!(allocations, 0);
    }
}
