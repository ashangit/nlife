//! Criterion microbenchmarks for `Grid::step()` and `Grid::expand_if_needed()`.
//!
//! The two pure-std modules are pulled in via `#[path]` so this binary crate
//! can access them without a `src/lib.rs`.  `library.rs` is deliberately
//! excluded because it depends on `crate::rle` and `env!("OUT_DIR")`; the four
//! patterns needed here are embedded as inline RLE constants instead.

#![allow(dead_code)]

#[path = "../src/grid.rs"]
mod grid;
#[allow(unused_imports)] // rle's #[cfg(test)] module imports super::* which is unused in bench context
#[path = "../src/rle.rs"]
mod rle;

use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use grid::Grid;
use rle::{center_cells, parse_rle};

// ── Embedded RLE constants ────────────────────────────────────────────────────

/// Blinker: 3 live cells, period-2 oscillator.
const BLINKER_RLE: &str = "3o!";

/// Beacon: 8 live cells, period-2 oscillator.
const BEACON_RLE: &str = "2o$2o$2b2o$2b2o!";

/// Pulsar: 48 live cells, period-3 oscillator; stresses the SWAR kernel.
const PULSAR_RLE: &str = "2b3o3b3o2$o4bobo4bo$o4bobo4bo$o4bobo4bo$2b3o3b3o2$2b3o3b3o$o4bobo4bo$o4bobo4bo$o4bobo4bo2$2b3o3b3o!";

/// Gosper Glider Gun: 36 live cells; produces a glider stream.
const GOSPER_GUN_RLE: &str = "24bo$22bobo$12b2o6b2o12b2o$11bo3bo4b2o12b2o$2o8bo5bo3b2o$2o8bo3bob2o4bobo$10bo5bo7bo$11bo3bo$12b2o!";

// ── Helper ────────────────────────────────────────────────────────────────────

/// Parse `rle` and place the resulting cells into a fresh `width × height` grid,
/// centred at `(height/2, width/2)`.
///
/// # Panics
/// Panics if `rle` fails to parse.
fn make_grid(width: usize, height: usize, rle: &str) -> Grid {
    let cells = center_cells(parse_rle(rle).expect("valid RLE").cells);
    let origin_row = (height / 2) as i32;
    let origin_col = (width / 2) as i32;
    let mut g = Grid::new(width, height);
    for (dr, dc) in &cells {
        let r = (origin_row + dr) as usize;
        let c = (origin_col + dc) as usize;
        g.set(r, c, true);
    }
    g
}

// ── Benchmark group: Grid::step() ────────────────────────────────────────────

/// Benchmark `Grid::step()` for four patterns of increasing complexity.
fn bench_grid_step(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_step");

    // ── blinker ───────────────────────────────────────────────────────────────
    // Period-2 oscillator; tiny frontier — measures the lower bound of step().
    {
        let mut g = make_grid(200, 200, BLINKER_RLE);
        group.bench_function("blinker", |b| {
            b.iter(|| black_box(&mut g).step());
        });
    }

    // ── pulsar ────────────────────────────────────────────────────────────────
    // 48-cell period-3 oscillator; ~3 words wide — stresses the SWAR kernel.
    {
        let mut g = make_grid(200, 200, PULSAR_RLE);
        group.bench_function("pulsar", |b| {
            b.iter(|| black_box(&mut g).step());
        });
    }

    // ── gosper_gun_fresh ──────────────────────────────────────────────────────
    // 36-cell gun on a fresh grid; iter_batched re-creates the start state
    // before each timed sample so glider drift does not compound across iters.
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(50);
    group.bench_function("gosper_gun_fresh", |b| {
        b.iter_batched(
            || make_grid(500, 400, GOSPER_GUN_RLE),
            |mut g| {
                g.step();
                black_box(());
            },
            BatchSize::SmallInput,
        );
    });

    // ── gosper_gun_300step_prewarm ─────────────────────────────────────────────
    // Same gun but setup runs 300 steps first so ~10 gliders are in flight.
    // This exercises a non-trivial, growing frontier.
    group.bench_function("gosper_gun_300step_prewarm", |b| {
        b.iter_batched(
            || {
                let mut g = make_grid(500, 400, GOSPER_GUN_RLE);
                for _ in 0..300 {
                    g.step();
                    g.expand_if_needed();
                }
                g
            },
            |mut g| {
                g.step();
                black_box(());
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ── Benchmark group: Grid::expand_if_needed() ────────────────────────────────

/// Benchmark `Grid::expand_if_needed()` for the trivial (no-op) and
/// expansion-needed (cell at top edge) cases.
fn bench_grid_expand(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_expand");

    // ── no_expand ─────────────────────────────────────────────────────────────
    // Single live cell at centre; returns (0, 0) immediately.
    group.bench_function("no_expand", |b| {
        b.iter_batched(
            || {
                let mut g = Grid::new(200, 200);
                g.set(100, 100, true);
                g
            },
            |mut g| black_box(g.expand_if_needed()),
            BatchSize::SmallInput,
        );
    });

    // ── expand_top ────────────────────────────────────────────────────────────
    // Single live cell at row 0; triggers top-margin prepend path.
    group.bench_function("expand_top", |b| {
        b.iter_batched(
            || {
                let mut g = Grid::new(200, 200);
                g.set(0, 100, true);
                g
            },
            |mut g| black_box(g.expand_if_needed()),
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_grid_step, bench_grid_expand);
criterion_main!(benches);
