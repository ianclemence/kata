//! One place that decides whether work runs in parallel.
//!
//! Native uses rayon. `wasm32` is sequential unless the `wasm-threads` feature
//! is on, which needs a nightly standard library and shared-memory atomics; the
//! browser build keeps the UI responsive by running the whole module inside a
//! Web Worker instead, so the main thread never blocks either way.

use crate::color::Rgba;

/// Split `data` into rows of `width` and run `f(row_index, row)` over each.
pub fn for_each_row_mut<T, F>(data: &mut [T], width: usize, f: F)
where
    T: Send,
    F: Fn(usize, &mut [T]) + Sync + Send,
{
    debug_assert!(width > 0);
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "wasm-threads")))]
    {
        use rayon::prelude::*;
        data.par_chunks_mut(width)
            .enumerate()
            .for_each(|(index, row)| f(index, row));
    }
    #[cfg(any(target_arch = "wasm32", feature = "wasm-threads"))]
    {
        for (index, row) in data.chunks_mut(width).enumerate() {
            f(index, row);
        }
    }
}

/// The same, over a read-only slice, yielding one value per row.
pub fn map_rows<T, U, F>(data: &[T], width: usize, f: F) -> Vec<U>
where
    T: Sync,
    U: Send,
    F: Fn(usize, &[T]) -> U + Sync + Send,
{
    debug_assert!(width > 0);
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "wasm-threads")))]
    {
        use rayon::prelude::*;
        data.par_chunks(width)
            .enumerate()
            .map(|(index, row)| f(index, row))
            .collect()
    }
    #[cfg(any(target_arch = "wasm32", feature = "wasm-threads"))]
    {
        data.chunks(width)
            .enumerate()
            .map(|(index, row)| f(index, row))
            .collect()
    }
}

/// Run `f` over every pixel of an RGBA buffer, row-parallel.
pub fn for_each_pixel_mut<F>(pixels: &mut [Rgba], width: usize, f: F)
where
    F: Fn(usize, usize, &mut Rgba) + Sync + Send,
{
    for_each_row_mut(pixels, width, |y, row| {
        for (x, pixel) in row.iter_mut().enumerate() {
            f(x, y, pixel);
        }
    });
}
