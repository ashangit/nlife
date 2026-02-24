use std::sync::OnceLock;

use crate::rle::{center_cells, parse_rle};

/// Category grouping for a built-in library pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Patterns that never change from one generation to the next.
    StillLife,
    /// Patterns that oscillate with a fixed period ≥ 2.
    Oscillator,
    /// Patterns that translate across the grid over time.
    Spaceship,
    /// Patterns that take a large number of generations to stabilise.
    Methuselah,
    /// User-saved patterns loaded from `~/.config/newlife/patterns/`.
    Custom,
}

/// A single entry in the built-in pattern library.
///
/// The `rle` field stores a raw RLE string (comments and header optional).
/// Call [`decoded_library`] to get the parsed, centred cell lists.
#[derive(Clone)]
pub struct LibraryEntry {
    /// Human-readable pattern name shown in the browser.
    pub name: &'static str,
    /// Behavioural category used for filtering.
    pub category: Category,
    /// Raw RLE string; decoded lazily via [`decoded_library`].
    pub rle: &'static str,
}

// ── Built-in pattern data ─────────────────────────────────────────────────────

/// All hard-coded library patterns, stored as RLE strings.
///
/// RLE origin does not need to be `(0,0)` — [`decoded_library`] applies
/// [`center_cells`] after parsing so every returned slice is centred.
static LIBRARY: &[LibraryEntry] = &[
    // ── Still Lifes ──────────────────────────────────────────────────────────
    LibraryEntry {
        name: "Block",
        category: Category::StillLife,
        rle: "2o$2o!",
    },
    LibraryEntry {
        name: "Beehive",
        category: Category::StillLife,
        rle: "b2o$o2bo$b2o!",
    },
    LibraryEntry {
        name: "Loaf",
        category: Category::StillLife,
        rle: "b2o$o2bo$bobo$2bo!",
    },
    LibraryEntry {
        name: "Boat",
        category: Category::StillLife,
        rle: "2o$obo$bo!",
    },
    LibraryEntry {
        name: "Tub",
        category: Category::StillLife,
        rle: "bo$obo$bo!",
    },
    LibraryEntry {
        name: "Pond",
        category: Category::StillLife,
        rle: "b2o$o2bo$o2bo$b2o!",
    },
    LibraryEntry {
        name: "Ship",
        category: Category::StillLife,
        rle: "2o$obo$b2o!",
    },
    LibraryEntry {
        name: "Long Boat",
        category: Category::StillLife,
        rle: "2o$obo$bobo$2bo!",
    },
    // ── Oscillators ──────────────────────────────────────────────────────────
    LibraryEntry {
        name: "Blinker",
        category: Category::Oscillator,
        rle: "3o!",
    },
    LibraryEntry {
        name: "Toad",
        category: Category::Oscillator,
        rle: "b3o$3o!",
    },
    LibraryEntry {
        name: "Beacon",
        category: Category::Oscillator,
        rle: "2o$2o$2b2o$2b2o!",
    },
    LibraryEntry {
        name: "Pulsar",
        category: Category::Oscillator,
        rle: "b3o7b3o$o4bo3bo4bo$o4bo3bo4bo$o4bo3bo4bo$2$b3o7b3o$4$b3o7b3o$2$o4bo3bo4bo$o4bo3bo4bo$o4bo3bo4bo$b3o7b3o!",
    },
    LibraryEntry {
        name: "Pentadecathlon",
        category: Category::Oscillator,
        rle: "10o!",
    },
    LibraryEntry {
        name: "Figure Eight",
        category: Category::Oscillator,
        rle: "3o$3o$3o$3b3o$3b3o$3b3o!",
    },
    LibraryEntry {
        name: "Queen Bee Shuttle",
        category: Category::Oscillator,
        rle: "9b2o$9bobo$4b2o6bo7b2o$2obo2bo2bo2bo7b2o$2o2b2o6bo$9bobo$9b2o!",
    },
    LibraryEntry {
        name: "Gosper Glider Gun",
        category: Category::Oscillator,
        rle: "24bo$22bobo$12b2o6b2o12b2o$11bo3bo4b2o12b2o$2o8bo5bo3b2o$2o8bo3bob2o4bobo$10bo5bo7bo$11bo3bo$12b2o!",
    },
    // ── Spaceships ───────────────────────────────────────────────────────────
    LibraryEntry {
        name: "Glider",
        category: Category::Spaceship,
        rle: "bo$2bo$3o!",
    },
    LibraryEntry {
        name: "LWSS",
        category: Category::Spaceship,
        rle: "bo2bo$o$o3bo$b4o!",
    },
    LibraryEntry {
        name: "MWSS",
        category: Category::Spaceship,
        rle: "2bo$o4bo$5bo$o4bo$b5o!",
    },
    LibraryEntry {
        name: "HWSS",
        category: Category::Spaceship,
        rle: "b2o$o4bo$6bo$o5bo$b6o!",
    },
    LibraryEntry {
        name: "Copperhead",
        category: Category::Spaceship,
        rle: "b2o2b2o$3b2o$3b2o$obo2bobo$o6bo2$o6bo$b2o2b2o$2b4o2$3b2o$3b2o!",
    },
    // ── Methuselahs ──────────────────────────────────────────────────────────
    LibraryEntry {
        name: "R-Pentomino",
        category: Category::Methuselah,
        rle: "b2o$2o$bo!",
    },
    LibraryEntry {
        name: "Acorn",
        category: Category::Methuselah,
        rle: "bo$3bo$2o2b3o!",
    },
    LibraryEntry {
        name: "Diehard",
        category: Category::Methuselah,
        rle: "6bo$2o$bo3b3o!",
    },
];

// ── Decoded library (OnceLock) ────────────────────────────────────────────────

/// Decoded library entry: a cloned header plus the centred cell list.
type DecodedEntry = (LibraryEntry, Vec<(i32, i32)>);

static DECODED: OnceLock<Vec<DecodedEntry>> = OnceLock::new();

/// Returns all built-in library entries paired with their decoded, centred cell lists.
///
/// Each RLE string is parsed once via [`parse_rle`] and the result is centred
/// via [`center_cells`].  Entries whose RLE strings fail to parse are silently
/// omitted.  The decoded result is cached for the lifetime of the process.
///
/// # Returns
/// A slice of `(LibraryEntry, cells)` pairs where `cells` contains
/// `(row_offset, col_offset)` pairs centred at `(0, 0)`.
pub fn decoded_library() -> &'static [DecodedEntry] {
    DECODED.get_or_init(|| {
        LIBRARY
            .iter()
            .filter_map(|entry| {
                parse_rle(entry.rle)
                    .ok()
                    .map(|cells| (entry.clone(), center_cells(cells)))
            })
            .collect()
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Grid;

    /// Asserts that a named library entry loads correctly into a 200×120 grid:
    /// - The entry is found in [`decoded_library`].
    /// - The parsed cell count equals `expected_count`.
    /// - All live cells are within grid bounds.
    /// - The centroid of live cells lies within ±2 cells of the grid centre.
    ///
    /// # Arguments
    /// * `name`           — entry name as in [`LIBRARY`]
    /// * `expected_count` — expected live-cell count after loading
    fn assert_library_entry_valid(name: &str, expected_count: usize) {
        let lib = decoded_library();
        let (_, cells) = lib
            .iter()
            .find(|(e, _)| e.name == name)
            .unwrap_or_else(|| panic!("library entry '{name}' not found"));

        assert_eq!(
            cells.len(),
            expected_count,
            "'{name}': expected {expected_count} cells, got {}",
            cells.len()
        );

        let width = 200usize;
        let height = 120usize;
        let mut g = Grid::new(width, height);
        g.set_cells(cells);

        let live: Vec<(usize, usize)> = (0..height)
            .flat_map(|r| (0..width).map(move |c| (r, c)))
            .filter(|&(r, c)| g.get(r, c))
            .collect();

        assert_eq!(
            live.len(),
            expected_count,
            "'{name}': grid has {} live cells after set_cells, expected {expected_count}",
            live.len()
        );

        for &(r, c) in &live {
            assert!(r < height, "'{name}': row {r} out of bounds");
            assert!(c < width, "'{name}': col {c} out of bounds");
        }

        if !live.is_empty() {
            let sum_r: f64 = live.iter().map(|&(r, _)| r as f64).sum();
            let sum_c: f64 = live.iter().map(|&(_, c)| c as f64).sum();
            let centroid_r = sum_r / live.len() as f64;
            let centroid_c = sum_c / live.len() as f64;
            assert!(
                (centroid_r - height as f64 / 2.0).abs() <= 2.0,
                "'{name}': centroid row {centroid_r:.2} not within ±2 of centre"
            );
            assert!(
                (centroid_c - width as f64 / 2.0).abs() <= 2.0,
                "'{name}': centroid col {centroid_c:.2} not within ±2 of centre"
            );
        }
    }

    #[test]
    fn test_decoded_library_non_empty() {
        assert!(
            !decoded_library().is_empty(),
            "decoded_library() must return at least one entry"
        );
    }

    #[test]
    fn test_all_entries_parse() {
        let lib = decoded_library();
        assert_eq!(
            lib.len(),
            LIBRARY.len(),
            "every LIBRARY entry must parse successfully"
        );
    }

    // ── Still Lifes ──────────────────────────────────────────────────────────
    #[test]
    fn test_library_block() {
        assert_library_entry_valid("Block", 4);
    }
    #[test]
    fn test_library_beehive() {
        assert_library_entry_valid("Beehive", 6);
    }
    #[test]
    fn test_library_loaf() {
        assert_library_entry_valid("Loaf", 7);
    }
    #[test]
    fn test_library_boat() {
        assert_library_entry_valid("Boat", 5);
    }
    #[test]
    fn test_library_tub() {
        assert_library_entry_valid("Tub", 4);
    }
    #[test]
    fn test_library_pond() {
        assert_library_entry_valid("Pond", 8);
    }
    #[test]
    fn test_library_ship() {
        assert_library_entry_valid("Ship", 6);
    }
    #[test]
    fn test_library_long_boat() {
        assert_library_entry_valid("Long Boat", 7);
    }

    // ── Oscillators ──────────────────────────────────────────────────────────
    #[test]
    fn test_library_blinker() {
        assert_library_entry_valid("Blinker", 3);
    }
    #[test]
    fn test_library_toad() {
        assert_library_entry_valid("Toad", 6);
    }
    #[test]
    fn test_library_beacon() {
        assert_library_entry_valid("Beacon", 8);
    }
    #[test]
    fn test_library_pulsar() {
        assert_library_entry_valid("Pulsar", 48);
    }
    #[test]
    fn test_library_pentadecathlon() {
        assert_library_entry_valid("Pentadecathlon", 10);
    }
    #[test]
    fn test_library_figure_eight() {
        assert_library_entry_valid("Figure Eight", 18);
    }
    #[test]
    fn test_library_queen_bee_shuttle() {
        assert_library_entry_valid("Queen Bee Shuttle", 26);
    }
    #[test]
    fn test_library_gosper_glider_gun() {
        assert_library_entry_valid("Gosper Glider Gun", 36);
    }

    // ── Spaceships ───────────────────────────────────────────────────────────
    #[test]
    fn test_library_glider() {
        assert_library_entry_valid("Glider", 5);
    }
    #[test]
    fn test_library_lwss() {
        assert_library_entry_valid("LWSS", 9);
    }
    #[test]
    fn test_library_mwss() {
        assert_library_entry_valid("MWSS", 11);
    }
    #[test]
    fn test_library_hwss() {
        assert_library_entry_valid("HWSS", 13);
    }
    #[test]
    fn test_library_copperhead() {
        assert_library_entry_valid("Copperhead", 28);
    }

    // ── Methuselahs ──────────────────────────────────────────────────────────
    #[test]
    fn test_library_r_pentomino() {
        assert_library_entry_valid("R-Pentomino", 5);
    }
    #[test]
    fn test_library_acorn() {
        assert_library_entry_valid("Acorn", 7);
    }
    #[test]
    fn test_library_diehard() {
        assert_library_entry_valid("Diehard", 7);
    }
}
