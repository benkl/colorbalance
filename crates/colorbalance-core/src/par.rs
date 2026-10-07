//! Row-parallel loops over disjoint output slices.
//!
//! With the `parallel` feature the work runs on rayon's global pool. Without
//! it the same closures run in order on the calling thread, which is what the
//! `wasm32` build uses. Every closure writes only its own slice and reads
//! shared immutable input, so both paths produce identical output.

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Call `f(index, chunk)` for every `chunk_len`-sized chunk of `data`.
/// The final chunk may be shorter. `chunk_len` must be nonzero.
pub fn chunks_mut<T, F>(data: &mut [T], chunk_len: usize, f: F)
where
    T: Send,
    F: Fn(usize, &mut [T]) + Sync + Send,
{
    #[cfg(feature = "parallel")]
    data.par_chunks_mut(chunk_len)
        .enumerate()
        .for_each(|(index, chunk)| f(index, chunk));
    #[cfg(not(feature = "parallel"))]
    data.chunks_mut(chunk_len)
        .enumerate()
        .for_each(|(index, chunk)| f(index, chunk));
}

/// Walk `a` and `b` in lockstep, `a_len` and `b_len` items at a time, and
/// return the sum of `f(index, a_chunk, b_chunk)`. Both slices must yield the
/// same number of chunks. The sum is over integers, so order does not matter.
pub fn zip_chunks_mut_sum<A, B, F>(
    a: &mut [A],
    a_len: usize,
    b: &mut [B],
    b_len: usize,
    f: F,
) -> usize
where
    A: Send,
    B: Send,
    F: Fn(usize, &mut [A], &mut [B]) -> usize + Sync + Send,
{
    #[cfg(feature = "parallel")]
    return a
        .par_chunks_mut(a_len)
        .zip(b.par_chunks_mut(b_len))
        .enumerate()
        .map(|(index, (a_chunk, b_chunk))| f(index, a_chunk, b_chunk))
        .sum();
    #[cfg(not(feature = "parallel"))]
    return a
        .chunks_mut(a_len)
        .zip(b.chunks_mut(b_len))
        .enumerate()
        .map(|(index, (a_chunk, b_chunk))| f(index, a_chunk, b_chunk))
        .sum();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_mut_passes_chunk_index_and_covers_the_ragged_tail() {
        let mut data = vec![0_u32; 10];
        chunks_mut(&mut data, 4, |index, chunk| {
            for value in chunk {
                *value = index as u32 + 1;
            }
        });
        assert_eq!(data, [1, 1, 1, 1, 2, 2, 2, 2, 3, 3]);
    }

    #[test]
    fn zip_sum_pairs_matching_chunks() {
        let mut a = vec![1_u32; 6];
        let mut b = vec![0_u32; 9];
        let total = zip_chunks_mut_sum(&mut a, 2, &mut b, 3, |index, a, b| {
            b.fill(a[0] + index as u32);
            index
        });
        assert_eq!(total, 3);
        assert_eq!(b, [1, 1, 1, 2, 2, 2, 3, 3, 3]);
    }
}
