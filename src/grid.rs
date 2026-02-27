use rayon::prelude::*;
use rustc_hash::FxHashSet;

/// Dead-cell margin added on each side when the grid expands.
const MARGIN: usize = 20;

/// Minimum word-level frontier size at which Rayon parallel evaluation pays off.
///
/// Below this the thread-pool wakeup cost (~37 µs measured) exceeds the
/// parallelism gain; above it each additional word contributes ~2 ns of saved
/// compute time per thread.  Break-even ≈ 37 000 ns / (8 ns × (1 − 1/threads))
/// ≈ 6 200 words at 4 threads; 4 000 chosen conservatively.
const RAYON_THRESHOLD: usize = 4_000;

/// Number of rows per cache-line tile in the tiled storage layout.
///
/// Eight `u64` words (8 × 8 bytes = 64 bytes) fill exactly one cache line.
/// Storing eight consecutive row values for the same word-column in a tile
/// means vertical neighbours (row±1, wi) share a cache line with (row, wi).
const TILE_HEIGHT: usize = 8;

/// Returns the flat index into the tiled cell buffer for word `(row, wi)`.
///
/// Words at `(row-1, wi)`, `(row, wi)`, `(row+1, wi)` are consecutive within
/// the same tile when `row % TILE_HEIGHT` is not 0 or 7, keeping `compute_word`
/// vertical loads on the same cache line.
///
/// # Arguments
/// * `row` — grid row index
/// * `wi`  — word-column index (0 ≤ wi < wpr)
/// * `wpr` — words per row (`words_per_row`)
#[inline]
fn tiled_idx(row: usize, wi: usize, wpr: usize) -> usize {
    (row / TILE_HEIGHT) * (wpr * TILE_HEIGHT) + wi * TILE_HEIGHT + (row % TILE_HEIGHT)
}

/// Returns the number of `u64` words required for the tiled layout.
///
/// Rounds `height` up to the next multiple of `TILE_HEIGHT` so every tile is
/// fully allocated.  Extra slots beyond row `height-1` are always zero.
///
/// # Arguments
/// * `height` — number of grid rows
/// * `wpr`    — words per row
#[inline]
fn tiled_size(height: usize, wpr: usize) -> usize {
    height.div_ceil(TILE_HEIGHT) * TILE_HEIGHT * wpr
}

// ── Bit-manipulation helpers ──────────────────────────────────────────────────

/// Returns `true` if the bit at `(row, col)` is set in the bit-packed slice.
///
/// # Arguments
/// * `cells`         — flat bit-packed row-major buffer (`u64` words)
/// * `words_per_row` — number of `u64` words per row
/// * `row`, `col`    — cell coordinates
#[inline]
fn get_bit(cells: &[u64], words_per_row: usize, row: usize, col: usize) -> bool {
    (cells[tiled_idx(row, col / 64, words_per_row)] >> (col % 64)) & 1 != 0
}

/// Sets or clears the bit at `(row, col)` in the bit-packed slice.
///
/// # Arguments
/// * `cells`         — flat bit-packed row-major buffer (mutable)
/// * `words_per_row` — number of `u64` words per row
/// * `row`, `col`    — cell coordinates
/// * `alive`         — `true` to set the bit, `false` to clear it
#[inline]
fn set_bit(cells: &mut [u64], words_per_row: usize, row: usize, col: usize, alive: bool) {
    let idx = tiled_idx(row, col / 64, words_per_row);
    let bit = col % 64;
    if alive {
        cells[idx] |= 1u64 << bit;
    } else {
        cells[idx] &= !(1u64 << bit);
    }
}

/// Inserts the 3×3 word neighbourhood of `(row, wi)` into `frontier`.
///
/// Clamps to grid bounds.  Duplicates are silently ignored by the set;
/// no later sort or dedup is needed.
///
/// # Arguments
/// * `frontier` — destination set (word-level `(row, wi)` pairs)
/// * `row`, `wi` — centre word coordinates
/// * `wpr`       — words per row
/// * `height`    — grid height in rows
fn add_word_neighborhood(
    frontier: &mut FxHashSet<(usize, usize)>,
    row: usize,
    wi: usize,
    wpr: usize,
    height: usize,
) {
    let r_start = if row > 0 { row - 1 } else { row };
    let r_end = if row + 1 < height { row + 1 } else { row };
    let w_start = if wi > 0 { wi - 1 } else { wi };
    let w_end = if wi + 1 < wpr { wi + 1 } else { wi };
    for r in r_start..=r_end {
        for w in w_start..=w_end {
            frontier.insert((r, w));
        }
    }
}

// ── SWAR step kernel ──────────────────────────────────────────────────────────

/// Computes one Conway's GoL step for all 64 bit positions in a single word
/// simultaneously, using SWAR (SIMD Within A Register) bitwise arithmetic.
///
/// Each call replaces 64 individual `count_neighbors` + rule checks with a
/// fixed sequence of ~30 bitwise operations, independent of grid density.
///
/// ## Bit/column convention
/// Bit `b` (0 = LSB) represents the cell at column `wi * 64 + b`.
/// "Left" means decreasing column index; "right" means increasing.
///
/// ## Boundary words
/// The caller passes `0` for any adjacent word that lies outside the grid
/// (dead-cell boundary).
///
/// ## Arguments
/// * `ap`, `a`, `an`  — above row: left-adjacent word, center word, right-adjacent word
/// * `cp`, `c`, `cn`  — current row (same order)
/// * `bp`, `b`, `bn`  — below row (same order)
///
/// Returns the new alive word for the center position.
#[inline]
#[allow(clippy::too_many_arguments)]
fn step_word(ap: u64, a: u64, an: u64, cp: u64, c: u64, cn: u64, bp: u64, b: u64, bn: u64) -> u64 {
    // ── 8 neighbour contributions (one bit per cell position) ─────────────────
    //
    // left-shift  (c << 1) | (cp >> 63):
    //   result[b] = c[b-1]  for b > 0,   result[0] = cp[63]
    //   → left neighbour in the same row.
    //
    // right-shift (c >> 1) | (cn << 63):
    //   result[b] = c[b+1]  for b < 63,  result[63] = cn[0]
    //   → right neighbour in the same row.
    let n0 = (c << 1) | (cp >> 63); // left  (same row)
    let n1 = (c >> 1) | (cn << 63); // right (same row)
    let n2 = (a << 1) | (ap >> 63); // above-left
    let n3 = a; // above
    let n4 = (a >> 1) | (an << 63); // above-right
    let n5 = (b << 1) | (bp >> 63); // below-left
    let n6 = b; // below
    let n7 = (b >> 1) | (bn << 63); // below-right

    // ── Bit-parallel addition of 8 one-bit values → 4-bit sum per position ───
    //
    // Uses carry-save adders (CSA) and half-adders (HA):
    //   CSA(a,b,c) → (sum = a^b^c,  carry = (a&b)|(b&c)|(a&c))
    //   HA(a,b)    → (sum = a^b,    carry = a&b)
    //
    // The tree reduces 8 one-bit inputs to a (bit2, bit1, bit0) triplet.
    // bit3 (only set when n=8) is computed but not needed: the Conway formula
    // !bit2 & bit1 & (bit0 | alive) already yields 0 for n=8 because bit1=0.

    // Stage 1 — reduce 8 → 6 values
    let s0 = n0 ^ n1 ^ n2;
    let c0 = (n0 & n1) | (n1 & n2) | (n0 & n2); // CSA(n0,n1,n2)

    let s1 = n3 ^ n4 ^ n5;
    let c1 = (n3 & n4) | (n4 & n5) | (n3 & n5); // CSA(n3,n4,n5)

    let s2 = n6 ^ n7;
    let c2 = n6 & n7; // HA(n6,n7)

    // Stage 2 — reduce weight-1 triple and weight-2 triple
    // s0,s1,s2 at weight 1 → s3 (bit0, final), c3 at weight 2
    let s3 = s0 ^ s1 ^ s2;
    let c3 = (s0 & s1) | (s1 & s2) | (s0 & s2); // CSA

    // c0,c1,c2 at weight 2 → s4 at weight 2, c4 at weight 4
    let s4 = c0 ^ c1 ^ c2;
    let c4 = (c0 & c1) | (c1 & c2) | (c0 & c2); // CSA

    // Stage 3 — merge weight-2 pair → s5 (bit1, final), c5 at weight 4
    let s5 = s4 ^ c3; // HA
    let c5 = s4 & c3;

    // Stage 4 — merge weight-4 pair → s6 (bit2)
    let s6 = c5 ^ c4; // HA (carry = bit3, implicit)

    // ── Conway's rule: new = (n==3) | (alive & n==2)
    //                       = !bit2 & bit1 & (bit0 | alive)
    !s6 & s5 & (s3 | c)
}

// ── AVX2 4-word kernel ────────────────────────────────────────────────────────

/// AVX2 analogue of `step_word` that processes four 64-cell words simultaneously.
///
/// Each 64-bit lane of the nine `__m256i` inputs corresponds to one of four
/// consecutive word positions (`wi`, `wi+1`, `wi+2`, `wi+3`).  The carry-save
/// adder tree is identical to `step_word` but all operations use 256-bit
/// lane-wise AVX2 intrinsics, giving 4× kernel throughput on AVX2 CPUs.
///
/// # Arguments
/// * `ap`, `a`, `an`  — above-row neighbourhood (left, centre, right word)
/// * `cp`, `c`, `cn`  — current-row neighbourhood
/// * `bp`, `b`, `bn`  — below-row neighbourhood
///
/// Returns a `__m256i` whose four 64-bit lanes are the new alive words.
///
/// # Safety
/// Caller must ensure the CPU supports AVX2 (use `is_x86_feature_detected!`).
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
#[allow(clippy::too_many_arguments)]
unsafe fn step_4words_avx2(
    ap: std::arch::x86_64::__m256i,
    a: std::arch::x86_64::__m256i,
    an: std::arch::x86_64::__m256i,
    cp: std::arch::x86_64::__m256i,
    c: std::arch::x86_64::__m256i,
    cn: std::arch::x86_64::__m256i,
    bp: std::arch::x86_64::__m256i,
    b: std::arch::x86_64::__m256i,
    bn: std::arch::x86_64::__m256i,
) -> std::arch::x86_64::__m256i {
    use std::arch::x86_64::*;
    macro_rules! shl {
        ($v:expr, $n:literal) => {
            _mm256_slli_epi64($v, $n)
        };
    }
    macro_rules! shr {
        ($v:expr, $n:literal) => {
            _mm256_srli_epi64($v, $n)
        };
    }
    macro_rules! or {
        ($a:expr, $b:expr) => {
            _mm256_or_si256($a, $b)
        };
    }
    macro_rules! and {
        ($a:expr, $b:expr) => {
            _mm256_and_si256($a, $b)
        };
    }
    macro_rules! xor {
        ($a:expr, $b:expr) => {
            _mm256_xor_si256($a, $b)
        };
    }
    macro_rules! andn {
        ($a:expr, $b:expr) => {
            _mm256_andnot_si256($a, $b)
        };
    } // (~a) & b

    let n0 = or!(shl!(c, 1), shr!(cp, 63)); // left  (same row)
    let n1 = or!(shr!(c, 1), shl!(cn, 63)); // right (same row)
    let n2 = or!(shl!(a, 1), shr!(ap, 63)); // above-left
    let n3 = a; // above
    let n4 = or!(shr!(a, 1), shl!(an, 63)); // above-right
    let n5 = or!(shl!(b, 1), shr!(bp, 63)); // below-left
    let n6 = b; // below
    let n7 = or!(shr!(b, 1), shl!(bn, 63)); // below-right

    // CSA/HA tree (identical structure to scalar step_word)
    let s0 = xor!(xor!(n0, n1), n2);
    let c0 = or!(or!(and!(n0, n1), and!(n1, n2)), and!(n0, n2));
    let s1 = xor!(xor!(n3, n4), n5);
    let c1 = or!(or!(and!(n3, n4), and!(n4, n5)), and!(n3, n5));
    let s2 = xor!(n6, n7);
    let c2 = and!(n6, n7);

    let s3 = xor!(xor!(s0, s1), s2);
    let c3 = or!(or!(and!(s0, s1), and!(s1, s2)), and!(s0, s2));
    let s4 = xor!(xor!(c0, c1), c2);
    let c4 = or!(or!(and!(c0, c1), and!(c1, c2)), and!(c0, c2));

    let s5 = xor!(s4, c3);
    let c5 = and!(s4, c3);
    let s6 = xor!(c5, c4);

    // !s6 & s5 & (s3 | c)  →  andn(s6, and(s5, or(s3, c)))
    and!(andn!(s6, s5), or!(s3, c))
}

/// Loads the 3×3 word neighbourhood for four consecutive words at
/// `(row, wi..wi+3)` and runs `step_4words_avx2`, returning four new alive words.
///
/// Boundary conditions are handled identically to `compute_word`: any word
/// outside the grid is treated as all-dead (zero).  Last-word masking
/// (zeroing unused high bits) is applied to any of the four words that
/// coincides with the final word in its row.
///
/// # Arguments
/// * `cells`  — current-generation bit-packed buffer (read-only)
/// * `row`, `wi` — coordinates of the first (leftmost) of the four words
/// * `wpr`    — words per row
/// * `width`  — grid width in columns
/// * `height` — grid height in rows
///
/// # Safety
/// Caller must ensure the CPU supports AVX2.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn compute_4words(
    cells: &[u64],
    row: usize,
    wi: usize,
    wpr: usize,
    width: usize,
    height: usize,
) -> [u64; 4] {
    use std::arch::x86_64::*;

    // Helper: read word at (r, w), returning 0 for out-of-bounds.
    let gw = |r: usize, w: usize| -> u64 {
        if r < height && w < wpr {
            cells[tiled_idx(r, w, wpr)]
        } else {
            0
        }
    };

    // For each of the three rows (above, current, below) load 6 consecutive
    // words starting at wi-1 (saturating at 0) so we can assemble the three
    // __m256i vectors (left-adjacent, centre, right-adjacent).
    //
    // lane 0 = word wi+0, lane 1 = word wi+1, lane 2 = word wi+2, lane 3 = wi+3
    //   cp lane k = word wi+k-1 (the left neighbour of word wi+k)
    //    c lane k = word wi+k
    //   cn lane k = word wi+k+1

    macro_rules! pack {
        ($a:expr, $b:expr, $c_val:expr, $d:expr) => {
            _mm256_set_epi64x($d as i64, $c_val as i64, $b as i64, $a as i64)
        };
    }

    let (ra, rb) = (row.wrapping_sub(1), row + 1); // above / below row indices

    // Above-row vectors
    let ap = pack!(
        gw(ra, wi.wrapping_sub(1)),
        gw(ra, wi),
        gw(ra, wi + 1),
        gw(ra, wi + 2)
    );
    let a = pack!(gw(ra, wi), gw(ra, wi + 1), gw(ra, wi + 2), gw(ra, wi + 3));
    let an = pack!(
        gw(ra, wi + 1),
        gw(ra, wi + 2),
        gw(ra, wi + 3),
        gw(ra, wi + 4)
    );

    // Current-row vectors
    let cp = pack!(
        gw(row, wi.wrapping_sub(1)),
        gw(row, wi),
        gw(row, wi + 1),
        gw(row, wi + 2)
    );
    let c = pack!(
        gw(row, wi),
        gw(row, wi + 1),
        gw(row, wi + 2),
        gw(row, wi + 3)
    );
    let cn = pack!(
        gw(row, wi + 1),
        gw(row, wi + 2),
        gw(row, wi + 3),
        gw(row, wi + 4)
    );

    // Below-row vectors
    let bp = pack!(
        gw(rb, wi.wrapping_sub(1)),
        gw(rb, wi),
        gw(rb, wi + 1),
        gw(rb, wi + 2)
    );
    let b = pack!(gw(rb, wi), gw(rb, wi + 1), gw(rb, wi + 2), gw(rb, wi + 3));
    let bn = pack!(
        gw(rb, wi + 1),
        gw(rb, wi + 2),
        gw(rb, wi + 3),
        gw(rb, wi + 4)
    );

    let result = unsafe { step_4words_avx2(ap, a, an, cp, c, cn, bp, b, bn) };

    let mut out = [0u64; 4];
    unsafe { _mm256_storeu_si256(out.as_mut_ptr() as *mut __m256i, result) };

    // Apply last-word mask to any lane that is the final word of its row.
    if !width.is_multiple_of(64) {
        let mask = (1u64 << (width % 64)) - 1;
        for (k, word) in out.iter_mut().enumerate() {
            if wi + k + 1 == wpr {
                *word &= mask;
            }
        }
    }
    out
}

// ── Per-word step helper ──────────────────────────────────────────────────────

/// Reads the 3×3 word neighbourhood from `cells` and applies one step of
/// Conway's rules via the SWAR kernel, returning the new alive word for
/// position `(row, wi)`.
///
/// This free function is `Send + Sync`-safe (takes a shared slice reference)
/// and is called from the Rayon parallel map in [`Grid::step`].
///
/// # Arguments
/// * `cells`  — flat bit-packed row-major buffer (read-only, current generation)
/// * `row`, `wi` — word coordinates of the target word
/// * `wpr`    — words per row
/// * `width`  — grid width in columns (for last-word masking)
/// * `height` — grid height in rows (for boundary checks)
fn compute_word(
    cells: &[u64],
    row: usize,
    wi: usize,
    wpr: usize,
    width: usize,
    height: usize,
) -> u64 {
    let gw = |r: usize, w: usize| -> u64 {
        if r < height && w < wpr {
            cells[tiled_idx(r, w, wpr)]
        } else {
            0
        }
    };
    let ap = if row > 0 && wi > 0 {
        gw(row - 1, wi - 1)
    } else {
        0
    };
    let a = if row > 0 { gw(row - 1, wi) } else { 0 };
    let an = if row > 0 && wi + 1 < wpr {
        gw(row - 1, wi + 1)
    } else {
        0
    };
    let cp = if wi > 0 { gw(row, wi - 1) } else { 0 };
    let c = gw(row, wi);
    let cn = if wi + 1 < wpr { gw(row, wi + 1) } else { 0 };
    let bp = if row + 1 < height && wi > 0 {
        gw(row + 1, wi - 1)
    } else {
        0
    };
    let b = if row + 1 < height { gw(row + 1, wi) } else { 0 };
    let bn = if row + 1 < height && wi + 1 < wpr {
        gw(row + 1, wi + 1)
    } else {
        0
    };

    let mut new_word = step_word(ap, a, an, cp, c, cn, bp, b, bn);
    // Mask off unused high bits in the last word of each row.
    if wi + 1 == wpr && !width.is_multiple_of(64) {
        new_word &= (1u64 << (width % 64)) - 1;
    }
    new_word
}

// ── Grid ──────────────────────────────────────────────────────────────────────

/// A 2-D grid of cells for Conway's Game of Life with dead-cell boundaries.
///
/// ## Storage layout
/// Cells are stored in a flat bit-packed `Vec<u64>` using a **tiled layout**
/// (`TILE_HEIGHT = 8`) for cache locality.  Each row occupies
/// `words_per_row = ⌈width / 64⌉` words.  Within each word, bit `col % 64`
/// corresponds to column `col` (LSB = leftmost column of the word group).
///
/// In the tiled layout, eight consecutive row values for the same word-column
/// are stored contiguously — forming a 64-byte cache line.  The flat index
/// for word `(row, wi)` is `tiled_idx(row, wi, wpr)`.  The total buffer size
/// is `tiled_size(height, wpr)`, which rounds `height` up to the next multiple
/// of `TILE_HEIGHT`; extra padding slots beyond `height-1` are always zero.
///
/// Unused high bits in the last word of each row are always zero.
///
/// ## Double-buffer
/// A pre-allocated `next` scratch buffer avoids heap allocation per step.
/// After computing the new generation, the two buffers are swapped.
///
/// ## Word-level frontier + SWAR neighbour counting
/// `frontier` is an `FxHashSet<(row, wi)>` covering every word that contains a
/// live cell or is adjacent to one.  Duplicates are rejected at insert time
/// (O(1)), eliminating the O(n log n) sort+dedup of the previous Vec approach.
/// `step()` materialises `frontier` into `frontier_vec` for cache-friendly
/// sequential/parallel iteration, then calls `step_word` which uses SWAR
/// bitwise arithmetic to evaluate all 64 positions in a word simultaneously —
/// replacing 64 individual `count_neighbors` calls with ~30 bitwise operations.
/// `prev_written` tracks which words were written to `next` last step so stale
/// values can be zeroed efficiently via O(1) set lookups.
pub struct Grid {
    /// Number of columns.
    pub width: usize,
    /// Number of rows.
    pub height: usize,
    /// Number of `u64` words per row: `⌈width / 64⌉`.
    words_per_row: usize,
    /// Current generation cell states (read during step), bit-packed.
    cells: Vec<u64>,
    /// Scratch buffer for the next generation (written during step, then swapped).
    next: Vec<u64>,
    /// Tight bounding box of live cells: `[row_min, col_min, row_max, col_max]`
    /// (all inclusive).  `None` when the grid is empty.  Used by the renderer.
    pub live_bbox: Option<[usize; 4]>,
    /// Per-word candidates for the next step: `FxHashSet<(row, wi)>` covering
    /// every word that contains a live cell or is adjacent to one.  Duplicates
    /// are rejected at insert time (O(1)); no sort or dedup is needed.
    frontier: FxHashSet<(usize, usize)>,
    /// `(row, word_index)` pairs written to `next` in the most recent step.
    /// Zeroed at the start of the following step via O(1) set lookup to clear
    /// stale double-buffer values.
    prev_written: Vec<(usize, usize)>,
    /// Scratch buffer for per-step SWAR results; cleared and reused each step
    /// to avoid a heap allocation on every call to `step()`.
    results_buf: Vec<(usize, usize, u64)>,
    /// Scratch buffer for the next-generation frontier; cleared and reused each
    /// step to avoid a heap allocation on every call to `step()`.
    next_frontier: FxHashSet<(usize, usize)>,
    /// Materialised frontier for the current step; populated from `frontier`
    /// before evaluation, then swapped into `prev_written` at step end.
    /// Provides a contiguous, cache-friendly buffer for sequential/parallel
    /// iteration and allows clean separation from the set.
    frontier_vec: Vec<(usize, usize)>,
}

/// Merges two bounding boxes into their union, handling `None` (empty) cases.
///
/// # Arguments
/// * `a`, `b` — bounding boxes to merge, each `[row_min, col_min, row_max, col_max]`
///
/// Returns `None` only when both inputs are `None`.
fn merge_bbox(a: Option<[usize; 4]>, b: Option<[usize; 4]>) -> Option<[usize; 4]> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some([rmin1, cmin1, rmax1, cmax1]), Some([rmin2, cmin2, rmax2, cmax2])) => Some([
            rmin1.min(rmin2),
            cmin1.min(cmin2),
            rmax1.max(rmax2),
            cmax1.max(cmax2),
        ]),
    }
}

impl Grid {
    /// Creates a new all-dead grid with the given dimensions.
    ///
    /// # Arguments
    /// * `width`  — number of columns
    /// * `height` — number of rows
    pub fn new(width: usize, height: usize) -> Self {
        let words_per_row = width.div_ceil(64);
        let n = tiled_size(height, words_per_row);
        Self {
            width,
            height,
            words_per_row,
            cells: vec![0u64; n],
            next: vec![0u64; n],
            live_bbox: None,
            frontier: FxHashSet::default(),
            prev_written: Vec::new(),
            results_buf: Vec::new(),
            next_frontier: FxHashSet::default(),
            frontier_vec: Vec::new(),
        }
    }

    /// Returns the alive/dead state of the cell at `(row, col)`.
    ///
    /// Returns `false` for out-of-bounds coordinates.
    pub fn get(&self, row: usize, col: usize) -> bool {
        if row >= self.height || col >= self.width {
            return false;
        }
        get_bit(&self.cells, self.words_per_row, row, col)
    }

    /// Sets the alive/dead state of the cell at `(row, col)`.
    ///
    /// Does nothing for out-of-bounds coordinates.
    /// Always adds the 3×3 neighbourhood to the frontier so the change is
    /// accounted for in the next `step()` call.
    pub fn set(&mut self, row: usize, col: usize, alive: bool) {
        if row < self.height && col < self.width {
            set_bit(&mut self.cells, self.words_per_row, row, col, alive);
            if alive {
                self.include_in_bbox(row, col);
            }
            add_word_neighborhood(
                &mut self.frontier,
                row,
                col / 64,
                self.words_per_row,
                self.height,
            );
        }
    }

    /// Toggles the alive/dead state of the cell at `(row, col)`.
    ///
    /// Does nothing for out-of-bounds coordinates.
    /// Expands `live_bbox` when the cell becomes alive (conservative — does not
    /// shrink when the cell dies).  Always adds the 3×3 neighbourhood to the
    /// frontier.
    pub fn toggle(&mut self, row: usize, col: usize) {
        if row < self.height && col < self.width {
            let wpr = self.words_per_row;
            let idx = tiled_idx(row, col / 64, wpr);
            let bit = col % 64;
            self.cells[idx] ^= 1u64 << bit;
            let new_alive = (self.cells[idx] >> bit) & 1 != 0;
            if new_alive {
                self.include_in_bbox(row, col);
            }
            add_word_neighborhood(
                &mut self.frontier,
                row,
                col / 64,
                self.words_per_row,
                self.height,
            );
        }
    }

    /// Sets every cell to dead and resets both buffers, bounding box, and
    /// frontier tracking.
    pub fn clear(&mut self) {
        self.cells.fill(0);
        self.next.fill(0);
        self.live_bbox = None;
        self.frontier.clear();
        self.prev_written.clear();
    }

    /// Advances the simulation by one generation using Conway's rules:
    /// - A live cell with 2 or 3 live neighbours survives.
    /// - A dead cell with exactly 3 live neighbours becomes alive.
    /// - All other cells die or stay dead.
    ///
    /// Out-of-bounds neighbours are treated as dead (finite, non-wrapping boundary).
    ///
    /// ## Optimisations
    /// - **Word-level frontier**: `frontier` is an `FxHashSet<(row, wi)>` covering
    ///   every word that contains or is adjacent to a live cell — O(live + border).
    ///   Duplicates are rejected at insert time (O(1)), replacing the previous
    ///   O(n log n) `sort_unstable` + `dedup` pass.
    /// - **SWAR neighbour counting**: `step_word` evaluates 64 cells per ~30
    ///   bitwise operations instead of 64 individual neighbour-count loops.
    /// - **Bit-packed storage**: 8× less memory; improved cache utilisation.
    /// - **Double-buffer**: writes to `next` and swaps — no heap allocation.
    /// - **O(1) stale-word zeroing**: each `prev_written` entry is looked up in
    ///   `frontier` (O(1) hash) — no sorted merge required.
    /// - **Adaptive Rayon**: `compute_word` called via `par_iter` when
    ///   `frontier_vec.len() ≥ RAYON_THRESHOLD` (4 000 words); sequential `iter`
    ///   otherwise, avoiding the ~37 µs thread-pool wakeup cost on small/medium patterns.
    pub fn step(&mut self) {
        if self.frontier.is_empty() {
            return;
        }
        let width = self.width;
        let height = self.height;
        let wpr = self.words_per_row;

        // Materialise frontier into a Vec for cache-friendly sequential/parallel iteration.
        // The Vec is a persistent scratch buffer; its allocation is amortised across steps.
        self.frontier_vec.clear();
        self.frontier_vec.extend(self.frontier.iter().copied());

        // Sort when the frontier is large enough that the O(n log n) sort cost is justified:
        // it enables AVX2 4-word batching (sequential path) and improves cache locality for
        // word loads (Rayon parallel path).  Tiny frontiers (blinker: ~9 words, pulsar: ~45)
        // skip the sort entirely — the overhead would exceed any benefit.
        const AVX2_SORT_THRESHOLD: usize = 64;
        let frontier_sorted = self.frontier_vec.len() >= AVX2_SORT_THRESHOLD;
        if frontier_sorted {
            self.frontier_vec.sort_unstable();
        }

        // Zero words written last step that won't be overwritten this step.
        // O(|prev_written|) with O(1) per contains(); prev_written need not be sorted.
        for &(row, wi) in &self.prev_written {
            if !self.frontier.contains(&(row, wi)) {
                self.next[tiled_idx(row, wi, wpr)] = 0;
            }
        }

        // Pre-size next_frontier: at most 9 neighbours per live word, ~50% overlap in practice.
        self.next_frontier.clear();
        self.next_frontier.reserve(self.frontier_vec.len() * 5);

        // Evaluation: parallel above RAYON_THRESHOLD, sequential below.
        // Reads only &self.cells (Send + Sync); no aliasing with self.next.
        // results_buf is reused across calls to avoid a per-step heap allocation.
        let cells = &self.cells;
        self.results_buf.clear();
        if self.frontier_vec.len() >= RAYON_THRESHOLD {
            self.results_buf.par_extend(
                self.frontier_vec
                    .par_iter()
                    .map(|&(row, wi)| (row, wi, compute_word(cells, row, wi, wpr, width, height))),
            );
        } else {
            // Check AVX2 support once per step (not per word).
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            let has_avx2 = std::is_x86_feature_detected!("avx2");
            #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
            let has_avx2 = false;

            if has_avx2 && frontier_sorted {
                // AVX2 4-word batching: frontier is already sorted, so consecutive
                // (row, wi)..(row, wi+3) runs are adjacent in the vec.
                let n = self.frontier_vec.len();
                let mut i = 0;
                while i < n {
                    let (row, wi) = self.frontier_vec[i];
                    // Try 4-word AVX2 batch: need 4 consecutive words in the same row.
                    if i + 3 < n
                        && self.frontier_vec[i + 1] == (row, wi + 1)
                        && self.frontier_vec[i + 2] == (row, wi + 2)
                        && self.frontier_vec[i + 3] == (row, wi + 3)
                        && wi + 4 <= wpr
                    {
                        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
                        {
                            let words =
                                unsafe { compute_4words(cells, row, wi, wpr, width, height) };
                            for (k, &word) in words.iter().enumerate() {
                                self.results_buf.push((row, wi + k, word));
                            }
                            i += 4;
                            continue;
                        }
                    }
                    self.results_buf.push((
                        row,
                        wi,
                        compute_word(cells, row, wi, wpr, width, height),
                    ));
                    i += 1;
                }
            } else {
                // Scalar fallback: tiny frontier or non-AVX2 CPU.  The iterator-based
                // extend is well-optimised by the compiler and adds zero overhead vs the
                // original implementation.
                self.results_buf.extend(
                    self.frontier_vec.iter().map(|&(row, wi)| {
                        (row, wi, compute_word(cells, row, wi, wpr, width, height))
                    }),
                );
            }
        }

        // Sequential apply: write results to self.next, build next_frontier and live_bbox.
        // next_frontier is reused across calls to avoid a per-step heap allocation.
        let mut new_live_bbox: Option<[usize; 4]> = None;

        for &(row, wi, new_word) in &self.results_buf {
            self.next[tiled_idx(row, wi, wpr)] = new_word;
            if new_word != 0 {
                add_word_neighborhood(&mut self.next_frontier, row, wi, wpr, height);
            }
            let mut bits = new_word;
            while bits != 0 {
                let b_pos = bits.trailing_zeros() as usize;
                let col = wi * 64 + b_pos;
                new_live_bbox = merge_bbox(new_live_bbox, Some([row, col, row, col]));
                bits &= bits - 1;
            }
        }

        std::mem::swap(&mut self.cells, &mut self.next);
        self.live_bbox = new_live_bbox;
        // prev_written = words evaluated this step (frontier_vec already holds exactly those).
        std::mem::swap(&mut self.prev_written, &mut self.frontier_vec);
        self.frontier_vec.clear(); // old prev_written, capacity retained for reuse next step
        // frontier = next_frontier (words to evaluate next step).
        std::mem::swap(&mut self.frontier, &mut self.next_frontier);
        self.next_frontier.clear(); // capacity retained for reuse next step
    }

    /// Clears the grid and places `cells` (already-centred offsets) at the grid centre.
    ///
    /// Offsets are added to `(height/2, width/2)` and cells that fall outside
    /// the grid bounds are silently skipped.  `live_bbox` and `frontier` are
    /// rebuilt from the placed cells via repeated [`set`] calls.
    ///
    /// # Arguments
    /// * `cells` — centred `(row_offset, col_offset)` pairs, e.g. from
    ///   `decoded_library()` or `center_cells()`
    pub fn set_cells(&mut self, cells: &[(i32, i32)]) {
        self.clear();
        let origin_row = (self.height / 2) as i32;
        let origin_col = (self.width / 2) as i32;
        for &(dr, dc) in cells {
            let r = origin_row + dr;
            let c = origin_col + dc;
            if r >= 0 && c >= 0 && (r as usize) < self.height && (c as usize) < self.width {
                self.set(r as usize, c as usize, true);
            }
        }
    }

    /// Returns all live cells as centred `(row_offset, col_offset)` pairs.
    ///
    /// Each live cell at `(row, col)` is returned as
    /// `(row as i32 - height/2, col as i32 - width/2)`.  The result is
    /// compatible with [`set_cells`] so a grid can be round-tripped through
    /// the `.cells` serialisation format.
    ///
    /// Scans only within `live_bbox` — O(bbox area) instead of O(W×H).
    pub fn live_cells_offsets(&self) -> Vec<(i32, i32)> {
        let origin_row = (self.height / 2) as i32;
        let origin_col = (self.width / 2) as i32;
        let Some([rmin, cmin, rmax, cmax]) = self.live_bbox else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for row in rmin..=rmax {
            for col in cmin..=cmax {
                if self.get(row, col) {
                    out.push((row as i32 - origin_row, col as i32 - origin_col));
                }
            }
        }
        out
    }

    /// Returns the total number of live cells in the grid.
    ///
    /// Counts set bits in the packed `cells` buffer; runs in O(words) time,
    /// independent of grid density.
    pub fn live_count(&self) -> u64 {
        self.cells.iter().map(|&w| w.count_ones() as u64).sum()
    }

    /// Fills the grid randomly using an xorshift64 PRNG seeded with `seed`.
    ///
    /// Clears all existing cells first. Each cell is set alive with probability
    /// `density_pct / 100` (clamped to [0, 100]).  A seed of 0 is promoted
    /// to 1 so the PRNG never stalls.
    ///
    /// # Arguments
    /// * `density_pct` — percentage of cells to set alive (0 = none, 100 = all)
    /// * `seed`        — PRNG seed; 0 is silently treated as 1
    pub fn fill_random(&mut self, density_pct: u8, seed: u64) {
        self.clear();
        let density = density_pct.min(100) as u128;
        // Threshold: a cell is alive when the PRNG output < threshold.
        // threshold = density_pct / 100 * (u64::MAX + 1)  as u128 arithmetic.
        let threshold = density * (u64::MAX as u128 + 1) / 100;

        let mut state = if seed == 0 { 1 } else { seed };

        for row in 0..self.height {
            for col in 0..self.width {
                // xorshift64 step.
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                if (state as u128) < threshold {
                    self.set(row, col, true);
                }
            }
        }
    }

    /// Checks all four edges for live cells and, for each edge that has one,
    /// adds `MARGIN` dead rows/columns on that side.  The cells buffer is
    /// rebuilt in place.
    ///
    /// Returns `(added_top_rows, added_left_cols)` so the caller can
    /// compensate its scroll offset.  `live_bbox` and `frontier` are shifted;
    /// `prev_written` is cleared (the fresh `next` buffer has no stale data).
    pub fn expand_if_needed(&mut self) -> (usize, usize) {
        // O(1) edge detection: live_bbox is maintained precisely by set()/toggle()
        // and updated by step().  A conservative bbox never misses a live edge cell.
        let Some([rmin, cmin, rmax, cmax]) = self.live_bbox else {
            return (0, 0); // empty grid — nothing to do
        };
        let top = rmin == 0;
        let bottom = rmax == self.height.saturating_sub(1);
        let left = cmin == 0;
        let right = cmax == self.width.saturating_sub(1);

        let add_top = if top { MARGIN } else { 0 };
        let add_bottom = if bottom { MARGIN } else { 0 };
        let add_left = if left { MARGIN } else { 0 };
        let add_right = if right { MARGIN } else { 0 };

        if add_top == 0 && add_bottom == 0 && add_left == 0 && add_right == 0 {
            return (0, 0);
        }

        let new_w = self.width + add_left + add_right;
        let new_h = self.height + add_top + add_bottom;
        let new_wpr = new_w.div_ceil(64);
        let n = tiled_size(new_h, new_wpr);
        let mut new_cells = vec![0u64; n];

        // Word-level copy: O(H × wpr) instead of O(H × W).
        // Each source word shifts left by `bit_shift` bits within the destination
        // row; overflow spills into the next word when `bit_shift > 0`.
        let bit_shift = add_left % 64;
        let word_shift = add_left / 64;
        let old_wpr = self.words_per_row; // capture before update
        for row in 0..self.height {
            let src_prefix = (row / TILE_HEIGHT) * (old_wpr * TILE_HEIGHT) + (row % TILE_HEIGHT);
            let dst_row = row + add_top;
            let dst_prefix =
                (dst_row / TILE_HEIGHT) * (new_wpr * TILE_HEIGHT) + (dst_row % TILE_HEIGHT);
            for wi in 0..old_wpr {
                let src = self.cells[src_prefix + wi * TILE_HEIGHT];
                if src == 0 {
                    continue;
                }
                let dst_wi = wi + word_shift;
                if bit_shift == 0 {
                    new_cells[dst_prefix + dst_wi * TILE_HEIGHT] |= src;
                } else {
                    // Handle separately to avoid `src >> 64` (undefined for u64).
                    new_cells[dst_prefix + dst_wi * TILE_HEIGHT] |= src << bit_shift;
                    if dst_wi + 1 < new_wpr {
                        new_cells[dst_prefix + (dst_wi + 1) * TILE_HEIGHT] |=
                            src >> (64 - bit_shift);
                    }
                }
            }
        }
        self.width = new_w;
        self.height = new_h;
        self.words_per_row = new_wpr;
        self.cells = new_cells;
        self.next = vec![0u64; tiled_size(new_h, new_wpr)]; // fresh zero buffer — no stale data

        fn shift(bbox: [usize; 4], dr: usize, dc: usize) -> [usize; 4] {
            [bbox[0] + dr, bbox[1] + dc, bbox[2] + dr, bbox[3] + dc]
        }
        self.live_bbox = self.live_bbox.map(|b| shift(b, add_top, add_left));

        // Shift frontier word entries: row indices shift by add_top, word indices
        // by word_shift (= add_left / 64).
        let word_shift = add_left / 64;
        self.frontier = self
            .frontier
            .iter()
            .map(|&(r, wi)| (r + add_top, wi + word_shift))
            .collect();

        // next is freshly zeroed, so prev_written is irrelevant.
        self.prev_written.clear();

        (add_top, add_left)
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Expands `live_bbox` to include the cell at `(row, col)`.
    fn include_in_bbox(&mut self, row: usize, col: usize) {
        self.live_bbox = Some(match self.live_bbox {
            None => [row, col, row, col],
            Some([rmin, cmin, rmax, cmax]) => {
                [rmin.min(row), cmin.min(col), rmax.max(row), cmax.max(col)]
            }
        });
    }

    /// Scans `cells` within `[r0..=r1, c0..=c1]` and returns the tight
    /// bounding box of all live cells found, or `None` if all cells are dead.
    ///
    /// Used by tests for brute-force reference comparisons.
    ///
    /// # Arguments
    /// * `r0`, `c0` — inclusive start of scan region
    /// * `r1`, `c1` — inclusive end of scan region
    #[cfg(test)]
    fn scan_live_bbox(&self, r0: usize, c0: usize, r1: usize, c1: usize) -> Option<[usize; 4]> {
        let wpr = self.words_per_row;
        let mut rmin = usize::MAX;
        let mut cmin = usize::MAX;
        let mut rmax = 0usize;
        let mut cmax = 0usize;
        let mut any = false;
        for row in r0..=r1 {
            for col in c0..=c1 {
                if get_bit(&self.cells, wpr, row, col) {
                    any = true;
                    rmin = rmin.min(row);
                    cmin = cmin.min(col);
                    rmax = rmax.max(row);
                    cmax = cmax.max(col);
                }
            }
        }
        if any {
            Some([rmin, cmin, rmax, cmax])
        } else {
            None
        }
    }
}

/// Counts live neighbours of cell `(row, col)` in a flat bit-packed slice.
///
/// Out-of-bounds neighbours are treated as dead (finite, non-wrapping boundary).
/// Retained as a utility for tests; production `step()` uses `step_word` instead.
///
/// # Arguments
/// * `cells`         — flat bit-packed row-major buffer (`u64` words)
/// * `words_per_row` — number of `u64` words per row
/// * `width`         — grid width in columns
/// * `height`        — grid height in rows
/// * `row`, `col`    — cell to evaluate
#[cfg(test)]
pub(crate) fn count_neighbors(
    cells: &[u64],
    words_per_row: usize,
    width: usize,
    height: usize,
    row: usize,
    col: usize,
) -> u8 {
    let mut count = 0u8;
    for dr in [-1i32, 0, 1] {
        for dc in [-1i32, 0, 1] {
            if dr == 0 && dc == 0 {
                continue;
            }
            let r = row as i32 + dr;
            let c = col as i32 + dc;
            if r >= 0 && c >= 0 && (r as usize) < height && (c as usize) < width {
                count += get_bit(cells, words_per_row, r as usize, c as usize) as u8;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a grid from a list of `(row, col)` live cells.
    fn make_grid(width: usize, height: usize, live: &[(usize, usize)]) -> Grid {
        let mut g = Grid::new(width, height);
        for &(r, c) in live {
            g.set(r, c, true);
        }
        g
    }

    /// Collect all live `(row, col)` pairs from a grid (sorted row-major).
    fn live_cells(g: &Grid) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for r in 0..g.height {
            for c in 0..g.width {
                if g.get(r, c) {
                    v.push((r, c));
                }
            }
        }
        v
    }

    #[test]
    fn test_step_word_all_neighbor_counts() {
        // Verify step_word against count_neighbors for all n in 0..=8.
        // Place n live cells in specific positions around a center and check
        // the center bit of the result.
        let wpr = 1usize;
        let w = 64usize;
        let h = 3usize;

        // Center cell: row 1, col 32 (bit 32 of word 0).
        // We'll put neighbors at positions around it in a 3-row, 64-col grid.
        // Neighbors of (1, 32): (0,31),(0,32),(0,33),(1,31),(1,33),(2,31),(2,32),(2,33)
        let neighbor_positions: &[(usize, usize)] = &[
            (0, 31),
            (0, 32),
            (0, 33),
            (1, 31),
            (1, 33),
            (2, 31),
            (2, 32),
            (2, 33),
        ];

        for n in 0u8..=8 {
            // Build cell words with exactly `n` of the 8 neighbors alive.
            let mut cells = vec![0u64; h * wpr];
            for &(r, c) in neighbor_positions.iter().take(n as usize) {
                cells[r * wpr + c / 64] |= 1u64 << (c % 64);
            }

            // Center alive
            let center_alive = cells[wpr] & (1u64 << 32) != 0;
            let expected_alive = n == 3 || (center_alive && n == 2);

            let a = cells[0];
            let c = cells[wpr];
            let b = cells[2 * wpr];
            let result = step_word(0, a, 0, 0, c, 0, 0, b, 0);
            let got = (result >> 32) & 1 != 0;

            assert_eq!(
                got, expected_alive,
                "step_word: n={n}, center_alive={center_alive}: expected={expected_alive} got={got}"
            );

            // Also cross-check with count_neighbors for the center cell.
            let cn = count_neighbors(&cells, wpr, w, h, 1, 32);
            assert_eq!(cn, n, "count_neighbors disagrees: expected {n}, got {cn}");
        }
    }

    #[test]
    fn test_step_word_word_boundary() {
        // Live cell at bit 63 of word 0 should influence bit 0 of word 1.
        // Three cells: (0,63), (0,64), (0,65) — a horizontal triplet spanning 2 words.
        // Cell (1, 64) is the center of the triplet → should become alive.
        let width = 128usize;
        let height = 3usize;
        let wpr = width.div_ceil(64);
        let mut cells = vec![0u64; height * wpr];

        // Row 0: bits 63, 64, 65 set (spanning words 0 and 1).
        cells[0] |= 1u64 << 63; // col 63
        cells[1] |= 1u64 << 0; // col 64
        cells[1] |= 1u64 << 1; // col 65

        // Compute word 1 of row 1 using step_word.
        let ap = cells[0]; // above-left word (word 0 of row 0)
        let a = cells[1]; // above word (word 1 of row 0)
        let an = 0u64; // above-right word (word 2, off-grid for col 128)
        let cp = cells[wpr];
        let c = cells[wpr + 1];
        let cn = 0u64;
        let bp = cells[2 * wpr];
        let b = cells[2 * wpr + 1];
        let bn = 0u64;

        let result = step_word(ap, a, an, cp, c, cn, bp, b, bn);
        // Cell (1, 64) = bit 0 of word 1: has 3 alive neighbors (0,63),(0,64),(0,65).
        assert!(
            result & 1 != 0,
            "cell (1,64) should be alive (3 above-neighbours)"
        );
        // Cell (1, 65) = bit 1: has 2 alive neighbors (0,64),(0,65) → stays dead.
        assert!(
            result & 2 == 0,
            "cell (1,65) should be dead (only 2 neighbours)"
        );
    }

    #[test]
    fn test_empty_grid_stays_empty() {
        let mut g = Grid::new(10, 10);
        g.step();
        assert!(live_cells(&g).is_empty());
    }

    #[test]
    fn test_blinker_oscillates() {
        let mut g = make_grid(20, 20, &[(5, 4), (5, 5), (5, 6)]);
        g.step();
        assert!(g.get(4, 5));
        assert!(g.get(5, 5));
        assert!(g.get(6, 5));
        assert_eq!(live_cells(&g).len(), 3);

        g.step();
        assert!(g.get(5, 4));
        assert!(g.get(5, 5));
        assert!(g.get(5, 6));
        assert_eq!(live_cells(&g).len(), 3);
    }

    #[test]
    fn test_block_still_life() {
        let mut g = make_grid(10, 10, &[(4, 4), (4, 5), (5, 4), (5, 5)]);
        g.step();
        assert!(g.get(4, 4));
        assert!(g.get(4, 5));
        assert!(g.get(5, 4));
        assert!(g.get(5, 5));
        assert_eq!(live_cells(&g).len(), 4);
    }

    #[test]
    fn test_glider_moves() {
        let mut g = make_grid(40, 40, &[(10, 11), (11, 12), (12, 10), (12, 11), (12, 12)]);
        for _ in 0..4 {
            g.step();
        }
        assert!(g.get(11, 12));
        assert!(g.get(12, 13));
        assert!(g.get(13, 11));
        assert!(g.get(13, 12));
        assert!(g.get(13, 13));
        assert_eq!(live_cells(&g).len(), 5);
    }

    #[test]
    fn test_expand_if_needed() {
        let mut g = make_grid(20, 20, &[(5, 5), (5, 6), (6, 5), (6, 6)]);
        let (t, l) = g.expand_if_needed();
        assert_eq!((t, l), (0, 0));
        assert_eq!(g.width, 20);
        assert_eq!(g.height, 20);

        let mut g2 = make_grid(20, 20, &[(0, 10)]);
        let (t2, l2) = g2.expand_if_needed();
        assert_eq!(t2, 20);
        assert_eq!(l2, 0);
        assert!(g2.get(20, 10));
        assert_eq!(g2.height, 40);

        let mut g3 = make_grid(20, 20, &[(10, 0)]);
        let (t3, l3) = g3.expand_if_needed();
        assert_eq!(t3, 0);
        assert_eq!(l3, 20);
        assert!(g3.get(10, 20));
        assert_eq!(g3.width, 40);
    }

    #[test]
    fn test_toggle() {
        let mut g = Grid::new(5, 5);
        assert!(!g.get(2, 2));
        g.toggle(2, 2);
        assert!(g.get(2, 2));
        g.toggle(2, 2);
        assert!(!g.get(2, 2));
    }

    #[test]
    fn test_clear() {
        let mut g = make_grid(5, 5, &[(0, 0), (1, 1), (2, 2)]);
        g.clear();
        assert!(live_cells(&g).is_empty());
    }

    #[test]
    fn test_underpopulation() {
        let mut g = make_grid(10, 10, &[(5, 5)]);
        g.step();
        assert!(!g.get(5, 5));
    }

    #[test]
    fn test_overpopulation() {
        let mut g = make_grid(10, 10, &[(1, 2), (2, 1), (2, 2), (2, 3), (3, 2)]);
        g.step();
        assert!(!g.get(2, 2), "centre cell should die from overpopulation");
    }

    #[test]
    fn test_set_cells_clears_first() {
        let mut g = Grid::new(40, 40);
        g.set(0, 0, true);
        // Glider offsets (centred)
        g.set_cells(&[(-1, 0), (0, 1), (1, -1), (1, 0), (1, 1)]);
        assert!(
            !g.get(0, 0),
            "sentinel cell should be cleared after set_cells"
        );
    }

    #[test]
    fn test_toad_is_period_2() {
        // Toad (p2): two offset rows of 3 — .OOO / OOO.
        let mut g = make_grid(
            20,
            20,
            &[(9, 9), (9, 10), (9, 11), (10, 8), (10, 9), (10, 10)],
        );
        let before = live_cells(&g);
        g.step();
        g.step();
        assert_eq!(live_cells(&g), before, "Toad should have period 2");
    }

    #[test]
    fn test_beacon_is_period_2() {
        // Beacon (p2): two touching 2×2 blocks
        let mut g = make_grid(
            20,
            20,
            &[
                (7, 7),
                (7, 8),
                (8, 7),
                (8, 8),
                (9, 9),
                (9, 10),
                (10, 9),
                (10, 10),
            ],
        );
        let before = live_cells(&g);
        g.step();
        g.step();
        assert_eq!(live_cells(&g), before, "Beacon should have period 2");
    }

    #[test]
    fn test_live_bbox_tracks_cells() {
        let mut g = Grid::new(20, 20);
        assert!(g.live_bbox.is_none(), "new grid should have no bbox");

        g.set(5, 8, true);
        assert_eq!(g.live_bbox, Some([5, 8, 5, 8]));

        g.set(3, 2, true);
        assert_eq!(g.live_bbox, Some([3, 2, 5, 8]));

        g.clear();
        assert!(g.live_bbox.is_none());
    }

    #[test]
    fn test_bit_packing_roundtrip() {
        let mut g = Grid::new(130, 4);
        g.set(0, 63, true);
        g.set(0, 64, true);
        g.set(1, 0, true);
        g.set(1, 127, true);

        assert!(g.get(0, 63));
        assert!(g.get(0, 64));
        assert!(!g.get(0, 62));
        assert!(!g.get(0, 65));
        assert!(g.get(1, 0));
        assert!(g.get(1, 127));
        assert!(!g.get(1, 1));
        assert!(!g.get(1, 126));
    }

    #[test]
    fn test_frontier_tracks_state() {
        let mut g = Grid::new(10, 10);
        assert!(g.frontier.is_empty(), "new grid frontier should be empty");

        // Grid is 10×10 → wpr = 1.  After set(5, 5), add_word_neighborhood is
        // called with (row=5, wi=0, wpr=1, height=10), which pushes rows {4,5,6}
        // × word {0} = (4,0), (5,0), (6,0).
        g.set(5, 5, true);
        assert!(
            !g.frontier.is_empty(),
            "frontier should be non-empty after set"
        );
        assert!(
            g.frontier.contains(&(4, 0)),
            "(4,0) missing from frontier after set(5,5,true)"
        );
        assert!(
            g.frontier.contains(&(5, 0)),
            "(5,0) missing from frontier after set(5,5,true)"
        );
        assert!(
            g.frontier.contains(&(6, 0)),
            "(6,0) missing from frontier after set(5,5,true)"
        );

        g.clear();
        assert!(
            g.frontier.is_empty(),
            "frontier should be empty after clear"
        );
    }

    #[test]
    fn test_live_cells_offsets_empty() {
        let g = Grid::new(20, 20);
        assert!(
            g.live_cells_offsets().is_empty(),
            "empty grid should return no offsets"
        );
    }

    #[test]
    fn test_live_cells_offsets_roundtrip() {
        // Place a horizontal blinker, extract offsets, reload onto a new grid,
        // and confirm the live cell set is identical.
        let size = 40;
        let blinker = &[(5, 4), (5, 5), (5, 6)];
        let g1 = make_grid(size, size, blinker);
        let offsets = g1.live_cells_offsets();
        let mut g2 = Grid::new(size, size);
        g2.set_cells(&offsets);
        assert_eq!(
            live_cells(&g1),
            live_cells(&g2),
            "round-tripped grid must have the same live cells"
        );
    }

    #[test]
    fn test_live_cells_offsets_corner() {
        // Place a single live cell at (0, 0) — top-left corner — on a 40×40 grid.
        // live_bbox == [0,0,0,0]; bbox-bounded scan must still find it.
        let mut g = Grid::new(40, 40);
        g.set(0, 0, true);
        let offsets = g.live_cells_offsets();
        assert_eq!(offsets.len(), 1, "expected exactly one offset");
        let origin_row = 40_i32 / 2;
        let origin_col = 40_i32 / 2;
        assert_eq!(
            offsets[0],
            (-origin_row, -origin_col),
            "corner cell offset must be (-origin_row, -origin_col)"
        );
    }

    #[test]
    fn test_live_count_empty() {
        let g = Grid::new(20, 20);
        assert_eq!(g.live_count(), 0, "empty grid should have 0 live cells");
    }

    #[test]
    fn test_live_count_known() {
        // A 2×2 block has exactly 4 live cells.
        let g = make_grid(20, 20, &[(5, 5), (5, 6), (6, 5), (6, 6)]);
        assert_eq!(g.live_count(), 4);
    }

    #[test]
    fn test_live_count_after_step() {
        // A horizontal blinker has 3 live cells; after one step it is vertical
        // but still has 3 live cells.
        let mut g = make_grid(20, 20, &[(5, 4), (5, 5), (5, 6)]);
        assert_eq!(g.live_count(), 3);
        g.step();
        assert_eq!(
            g.live_count(),
            3,
            "blinker should still have 3 live cells after one step"
        );
    }

    #[test]
    fn test_expand_fires_when_bbox_touches_edge() {
        // A live cell at row 0 touches the top edge → expand_if_needed must
        // prepend MARGIN rows and return (MARGIN, 0).
        let mut g = Grid::new(100, 100);
        g.set(0, 50, true);
        let (add_top, add_left) = g.expand_if_needed();
        assert_eq!(add_top, MARGIN, "expected MARGIN rows added at top");
        assert_eq!(add_left, 0, "left edge untouched — no cols should be added");
    }

    #[test]
    fn test_expand_no_fire_when_bbox_interior() {
        // A live cell well away from all edges must not trigger any expansion.
        let mut g = Grid::new(100, 100);
        g.set(50, 50, true);
        let result = g.expand_if_needed();
        assert_eq!(result, (0, 0), "interior cell must not trigger expansion");
    }

    #[test]
    fn test_fill_random_density() {
        // 50% density on a 100×100 grid should be within ±5% of 5000 live cells.
        let mut g = Grid::new(100, 100);
        g.fill_random(50, 42);
        let count = live_cells(&g).len();
        let expected = 5000usize;
        let tolerance = 500usize; // 5%
        assert!(
            count >= expected - tolerance && count <= expected + tolerance,
            "fill_random(50) produced {count} live cells, expected ~{expected} ±{tolerance}"
        );
    }

    #[test]
    fn test_fill_random_seed_zero() {
        // seed=0 must not panic and should leave some live cells (density=50).
        let mut g = Grid::new(50, 50);
        g.fill_random(50, 0);
        let count = live_cells(&g).len();
        assert!(count > 0, "fill_random with seed=0 left no live cells");
    }

    #[test]
    fn test_fill_random_all_dead() {
        let mut g = Grid::new(20, 20);
        g.fill_random(0, 1);
        assert!(
            live_cells(&g).is_empty(),
            "fill_random(0) should leave an empty grid"
        );
    }

    #[test]
    fn test_fill_random_all_alive() {
        let mut g = Grid::new(20, 20);
        g.fill_random(100, 1);
        assert_eq!(
            live_cells(&g).len(),
            20 * 20,
            "fill_random(100) should fill every cell"
        );
    }

    #[test]
    fn test_frontier_step_correctness() {
        // Run a 50-step glider and compare with a brute-force reference.
        let live: &[(usize, usize)] = &[(10, 11), (11, 12), (12, 10), (12, 11), (12, 12)];
        let mut optimised = make_grid(60, 60, live);
        let mut reference = make_grid(60, 60, live);

        for _ in 0..50 {
            optimised.step();

            let w = reference.width;
            let h = reference.height;
            let mut snapshot = Vec::with_capacity(w * h);
            for r in 0..h {
                for c in 0..w {
                    snapshot.push(reference.get(r, c));
                }
            }
            for row in 0..h {
                for col in 0..w {
                    let mut n = 0u8;
                    for dr in [-1i32, 0, 1] {
                        for dc in [-1i32, 0, 1] {
                            if dr == 0 && dc == 0 {
                                continue;
                            }
                            let r = row as i32 + dr;
                            let c = col as i32 + dc;
                            if r >= 0 && c >= 0 && (r as usize) < h && (c as usize) < w {
                                n += snapshot[r as usize * w + c as usize] as u8;
                            }
                        }
                    }
                    let alive = snapshot[row * w + col];
                    reference.set(
                        row,
                        col,
                        matches!((alive, n), (true, 2) | (true, 3) | (false, 3)),
                    );
                }
            }
            reference.live_bbox = reference.scan_live_bbox(0, 0, h - 1, w - 1);
        }

        assert_eq!(
            live_cells(&optimised),
            live_cells(&reference),
            "SWAR frontier-based and brute-force states diverged after 50 steps"
        );
    }

    /// Verifies that `step_4words_avx2` produces the same output as four independent
    /// `step_word` calls for representative inputs: all-dead, blinker neighbourhood,
    /// and dense/mixed patterns.
    #[test]
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn test_avx2_matches_scalar() {
        if !std::is_x86_feature_detected!("avx2") {
            // Skip on machines without AVX2.
            return;
        }
        use std::arch::x86_64::*;

        // Helper: pack four u64 values into a __m256i (lane0=a, lane1=b, lane2=c, lane3=d).
        let pack = |a: u64, b: u64, c: u64, d: u64| -> __m256i {
            unsafe { _mm256_set_epi64x(d as i64, c as i64, b as i64, a as i64) }
        };

        // Helper: extract the four 64-bit lanes of a __m256i.
        let unpack = |v: __m256i| -> [u64; 4] {
            let mut out = [0u64; 4];
            unsafe { _mm256_storeu_si256(out.as_mut_ptr() as *mut __m256i, v) };
            out
        };

        // Test cases: each is a (ap, a, an, cp, c, cn, bp, b, bn) tuple of
        // four u64 values per position. We run AVX2 and compare with 4× scalar.
        struct Case {
            name: &'static str,
            // Nine arrays of four u64s: [ap0,ap1,ap2,ap3], [a0..], etc.
            ap: [u64; 4],
            a: [u64; 4],
            an: [u64; 4],
            cp: [u64; 4],
            c: [u64; 4],
            cn: [u64; 4],
            bp: [u64; 4],
            b: [u64; 4],
            bn: [u64; 4],
        }

        let cases = [
            Case {
                name: "all_dead",
                ap: [0; 4],
                a: [0; 4],
                an: [0; 4],
                cp: [0; 4],
                c: [0; 4],
                cn: [0; 4],
                bp: [0; 4],
                b: [0; 4],
                bn: [0; 4],
            },
            Case {
                // Horizontal blinker: three consecutive bits in the centre word.
                name: "blinker_neighbourhood",
                ap: [0; 4],
                a: [0; 4],
                an: [0; 4],
                cp: [0; 4],
                c: [0b111, 0b111, 0b111, 0b111],
                cn: [0; 4],
                bp: [0; 4],
                b: [0; 4],
                bn: [0; 4],
            },
            Case {
                // Dense: all ones in c, partial in neighbours.
                name: "dense_centre",
                ap: [0xAAAA_AAAA_AAAA_AAAA; 4],
                a: [0xFFFF_FFFF_FFFF_FFFF; 4],
                an: [0x5555_5555_5555_5555; 4],
                cp: [0xFFFF_FFFF_FFFF_FFFF; 4],
                c: [0xFFFF_FFFF_FFFF_FFFF; 4],
                cn: [0xFFFF_FFFF_FFFF_FFFF; 4],
                bp: [0xAAAA_AAAA_AAAA_AAAA; 4],
                b: [0xFFFF_FFFF_FFFF_FFFF; 4],
                bn: [0x5555_5555_5555_5555; 4],
            },
            Case {
                // Mixed: varying values per lane to test lane independence.
                name: "mixed_lanes",
                ap: [
                    0x0000_0000_0000_0001,
                    0xDEAD_BEEF_0000_0000,
                    0x0,
                    0xFFFF_0000_FFFF_0000,
                ],
                a: [
                    0x0000_0000_0000_0007,
                    0x0000_0000_1234_5678,
                    0x0,
                    0x0000_FFFF_0000_FFFF,
                ],
                an: [
                    0x0000_0000_0000_000E,
                    0xCAFE_BABE_0000_0000,
                    0x0,
                    0xFFFF_FFFF_0000_0000,
                ],
                cp: [0x0, 0x0, 0x0, 0x0],
                c: [
                    0x0000_0000_0000_0007,
                    0x0000_0000_0000_FFFF,
                    0x0,
                    0xAAAA_BBBB_CCCC_DDDD,
                ],
                cn: [0x0, 0x0, 0x0, 0x0],
                bp: [0x0, 0x0, 0x0, 0x0],
                b: [0x0000_0000_0000_0007, 0x0, 0x0, 0xAAAA_BBBB_CCCC_DDDD],
                bn: [0x0, 0x0, 0x0, 0x0],
            },
        ];

        for case in &cases {
            let avx_result = unsafe {
                let result = step_4words_avx2(
                    pack(case.ap[0], case.ap[1], case.ap[2], case.ap[3]),
                    pack(case.a[0], case.a[1], case.a[2], case.a[3]),
                    pack(case.an[0], case.an[1], case.an[2], case.an[3]),
                    pack(case.cp[0], case.cp[1], case.cp[2], case.cp[3]),
                    pack(case.c[0], case.c[1], case.c[2], case.c[3]),
                    pack(case.cn[0], case.cn[1], case.cn[2], case.cn[3]),
                    pack(case.bp[0], case.bp[1], case.bp[2], case.bp[3]),
                    pack(case.b[0], case.b[1], case.b[2], case.b[3]),
                    pack(case.bn[0], case.bn[1], case.bn[2], case.bn[3]),
                );
                unpack(result)
            };

            for k in 0..4 {
                let scalar = step_word(
                    case.ap[k], case.a[k], case.an[k], case.cp[k], case.c[k], case.cn[k],
                    case.bp[k], case.b[k], case.bn[k],
                );
                assert_eq!(
                    avx_result[k], scalar,
                    "AVX2 lane {k} mismatch for case '{}': got {:#018x}, expected {:#018x}",
                    case.name, avx_result[k], scalar
                );
            }
        }
    }

    /// Verifies `tiled_idx` and `tiled_size` produce the expected flat indices
    /// for known (row, wi, wpr) inputs, ensuring the tile formula is correct.
    #[test]
    fn test_tiled_idx_formula() {
        // tiled_idx(row, wi, wpr) = (row / 8) * (wpr * 8) + wi * 8 + (row % 8)
        assert_eq!(
            tiled_idx(0, 0, 2),
            0,
            "row 0, wi 0: first element of first tile"
        );
        assert_eq!(
            tiled_idx(1, 0, 2),
            1,
            "row 1, wi 0: within same tile as row 0"
        );
        assert_eq!(
            tiled_idx(8, 0, 2),
            16,
            "row 8, wi 0: first element of second tile"
        );
        assert_eq!(
            tiled_idx(0, 1, 2),
            8,
            "row 0, wi 1: second word-column in same tile"
        );

        // tiled_size(height, wpr) = div_ceil(height, 8) * 8 * wpr
        assert_eq!(
            tiled_size(16, 2),
            32,
            "16 rows × wpr 2: exact multiple, 2 tiles = 32"
        );
        assert_eq!(
            tiled_size(10, 2),
            32,
            "10 rows × wpr 2: padded up to 16 rows = 32"
        );
    }
}
