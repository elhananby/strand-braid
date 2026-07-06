// Copyright (C) The Strand-Braid Authors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! End-to-end chessboard detection — wiring stages 1-4 together.
//!
//! Mirrors the structure of OpenCV's `findChessboardCorners` with the
//! `ADAPTIVE_THRESH | NORMALIZE_IMAGE` flags: equalize the image, then for an
//! increasing number of dilations, binarize, generate quads, link them into a
//! board graph, and try to extract a board of the requested size. The first
//! dilation level that yields a complete, monotone board wins.
//!
//! Corner *order* matches the lattice row-major readout of [`extract_board`];
//! canonicalizing to OpenCV's exact start corner/direction is handled by the
//! caller's cross-check for now.

use super::binarize::{adaptive_threshold_mean, equalize_hist};
use super::board::extract_board;
use super::contour::find_contours;
use super::link::{connected_components, link_quads};
use super::order::{assign_grid, order_all_corners};
use super::quad::{Quad, contour_area, find_quads};

/// Maximum number of dilation iterations to try (matches OpenCV's range).
const MAX_DILATIONS: usize = 7;

/// 3x3 dilation (max filter) of a binary image, out-of-bounds treated as 0,
/// matching OpenCV's default `dilate` with a 3x3 rectangular kernel.
///
/// Implemented as two 1D passes (row-wise then column-wise). This is exact,
/// not an approximation: a 3x3 max filter is the max over a Cartesian
/// product of row and column offsets, and max distributes over that product
/// the same way regardless of grouping, including the zero-padding at the
/// image border (an out-of-range row contributes 0 to every column in it,
/// so treating it as an all-zero intermediate row is equivalent to skipping
/// it). The separable form does roughly half the comparisons of the naive
/// 3x3-window version and, away from the image edges, needs no per-neighbor
/// bounds check.
fn dilate3x3(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    if w == 0 || h == 0 {
        return Vec::new();
    }

    // Row pass: row_max[y][x] = max(src[y][x-1], src[y][x], src[y][x+1]),
    // treating x-1/x+1 outside [0, w) as 0.
    let mut row_max = vec![0u8; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let out = &mut row_max[y * w..(y + 1) * w];
        if w == 1 {
            out[0] = row[0];
        } else {
            out[0] = row[0].max(row[1]);
            for x in 1..w - 1 {
                out[x] = row[x - 1].max(row[x]).max(row[x + 1]);
            }
            out[w - 1] = row[w - 2].max(row[w - 1]);
        }
    }

    // Column pass over the row-maxed intermediate: dst[y][x] =
    // max(row_max[y-1][x], row_max[y][x], row_max[y+1][x]), treating y-1/y+1
    // outside [0, h) as 0.
    let mut dst = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut m = row_max[y * w + x];
            if y > 0 {
                m = m.max(row_max[(y - 1) * w + x]);
            }
            if y + 1 < h {
                m = m.max(row_max[(y + 1) * w + x]);
            }
            dst[y * w + x] = m;
        }
    }
    dst
}

/// Paint a `thickness`-pixel border of `value` around the image, as OpenCV does
/// to close squares that touch the image edge.
fn draw_border(img: &mut [u8], w: usize, h: usize, value: u8, thickness: usize) {
    for y in 0..h {
        for x in 0..w {
            if x < thickness || y < thickness || x + thickness >= w || y + thickness >= h {
                img[y * w + x] = value;
            }
        }
    }
}

/// Detect a `pattern_w x pattern_h` (inner corners) chessboard in a grayscale
/// image. Returns the inner corners row-major, or `None` if no board is found.
///
/// `pattern_w`/`pattern_h` are the inner-corner counts (e.g. 9x6).
pub fn find_chessboard_corners(
    gray: &[u8],
    w: usize,
    h: usize,
    pattern_w: usize,
    pattern_h: usize,
) -> Option<Vec<(f32, f32)>> {
    assert_eq!(gray.len(), w * h);
    let eq = equalize_hist(gray);

    // Adaptive-threshold block sizes scaled to the image (odd). Several scales
    // are tried because the right neighborhood depends on the square size and
    // perspective, as in OpenCV's multi-attempt loop.
    let smaller = w.min(h);
    let block_sizes: Vec<usize> = [smaller / 5, smaller / 9, smaller / 15]
        .iter()
        .map(|b| (b | 1).max(3))
        .collect();
    let deltas = [0.0f64, 5.0, 9.0];
    // Reject tiny noise quads and the whole-image background quad.
    let min_area = 25.0;
    let max_area = (w as f64) * (h as f64) * 0.5;

    // Precompute each (block_size, delta) binarization once, then dilate all
    // of them incrementally as `dilations` increases below, instead of
    // rebuilding the threshold and redoing every dilation pass from scratch
    // at each level. Dilating an already-dilated image by one more pass is
    // bit-identical to redoing all passes from the original threshold (the
    // 3x3 max filter composes with itself), so this reaches the exact same
    // bin image at every (dilations, block_size, delta) combination as the
    // naive from-scratch loop -- for 1/8th the threshold computations and
    // 1/4 the dilation passes in the worst (no-board) case.
    let mut bins: Vec<Vec<u8>> = Vec::with_capacity(block_sizes.len() * deltas.len());
    for &block_size in &block_sizes {
        for &delta in &deltas {
            let mut bin = adaptive_threshold_mean(&eq, w, h, block_size, delta);
            draw_border(&mut bin, w, h, 255, 1);
            bins.push(bin);
        }
    }

    for dilations in 0..=MAX_DILATIONS {
        if dilations > 0 {
            for bin in bins.iter_mut() {
                *bin = dilate3x3(bin, w, h);
            }
        }

        for bin in &bins {
            let contours = find_contours(bin, w, h);
            let all_quads = find_quads(&contours, min_area);
            let quads: Vec<Quad> = all_quads
                .into_iter()
                .filter(|q| {
                    let corners = [q.corners[0], q.corners[1], q.corners[2], q.corners[3]];
                    contour_area(&corners) <= max_area
                })
                .collect();
            if quads.len() < pattern_w * pattern_h / 4 {
                continue;
            }

            let mut linked = link_quads(&quads);
            order_all_corners(&mut linked);
            for comp in connected_components(&linked) {
                let grid = assign_grid(&linked, &comp);
                if let Some(corners) = extract_board(&linked, &grid, pattern_w, pattern_h) {
                    return Some(corners);
                }
                if let Some(corners) = extract_board(&linked, &grid, pattern_h, pattern_w) {
                    return Some(corners);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Naive full-window 3x3 max filter (the pre-optimization implementation),
    /// kept here only to cross-check the separable [`dilate3x3`] above.
    fn dilate3x3_naive(src: &[u8], w: usize, h: usize) -> Vec<u8> {
        let mut dst = vec![0u8; src.len()];
        for y in 0..h {
            for x in 0..w {
                let mut m = 0u8;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                            m = m.max(src[ny as usize * w + nx as usize]);
                        }
                    }
                }
                dst[y * w + x] = m;
            }
        }
        dst
    }

    /// Small, dependency-free xorshift PRNG so the property tests below don't
    /// need a `rand` dev-dependency.
    struct Xorshift(u64);
    impl Xorshift {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn next_u8(&mut self) -> u8 {
            (self.next_u64() & 0xff) as u8
        }
    }

    #[test]
    fn dilate3x3_matches_naive_reference() {
        let mut rng = Xorshift(0x9E3779B97F4A7C15);
        for &(w, h) in &[(1, 1), (1, 5), (5, 1), (2, 2), (7, 5), (17, 13), (32, 32)] {
            for _ in 0..20 {
                let img: Vec<u8> = (0..w * h).map(|_| rng.next_u8()).collect();
                let fast = dilate3x3(&img, w, h);
                let naive = dilate3x3_naive(&img, w, h);
                assert_eq!(fast, naive, "mismatch at {w}x{h}");
            }
        }
    }

    #[test]
    fn dilate3x3_binary_image_grows_foreground() {
        #[rustfmt::skip]
        let img = [
            0u8, 0, 0, 0, 0,
            0, 0, 0, 0, 0,
            0, 0, 255, 0, 0,
            0, 0, 0, 0, 0,
            0, 0, 0, 0, 0,
        ];
        let out = dilate3x3(&img, 5, 5);
        // The single foreground pixel at (2,2) dilates into its full 3x3
        // neighborhood.
        for y in 1..=3 {
            for x in 1..=3 {
                assert_eq!(out[y * 5 + x], 255, "expected dilation at ({x},{y})");
            }
        }
        assert_eq!(out[0], 0, "corner should remain background");
    }

    #[test]
    fn incremental_dilation_matches_recompute_from_base() {
        // `find_chessboard_corners` keeps one dilation state per
        // (block_size, delta) combo and advances it by a single
        // `dilate3x3` call as the dilation level increases, instead of the
        // pre-refactor approach of recomputing `dilate3x3` N times from the
        // original base at every level. Verify these produce bit-identical
        // images at every level, which is the property the refactor depends
        // on for being behavior-preserving.
        let mut rng = Xorshift(0xD1B54A32D192ED03);
        let (w, h) = (23, 19);
        let base: Vec<u8> = (0..w * h)
            .map(|_| if rng.next_u8() > 200 { 255 } else { 0 })
            .collect();

        let mut incremental = base.clone();
        for n in 0..=MAX_DILATIONS {
            let mut from_scratch = base.clone();
            for _ in 0..n {
                from_scratch = dilate3x3(&from_scratch, w, h);
            }
            assert_eq!(incremental, from_scratch, "mismatch after {n} dilations");

            incremental = dilate3x3(&incremental, w, h);
        }
    }
}
