//! Build script: scans `src/patterns/<category>/` at compile time,
//! derives each pattern's display name from its file stem, and writes
//! `$OUT_DIR/library_entries.rs` containing the `static LIBRARY` array
//! included by `src/library.rs`.
//!
//! Adding a new built-in pattern is as simple as dropping a `.rle` file in the
//! appropriate category subfolder — no changes to `library.rs` are required.

use std::fs;
use std::path::{Path, PathBuf};

/// Ordered list of `(subdirectory_name, Category variant expression)` pairs.
///
/// The order determines how categories appear in the library; within each
/// category, patterns are sorted alphabetically by file name.
const CATEGORIES: &[(&str, &str)] = &[
    ("still_life", "Category::StillLife"),
    ("oscillator", "Category::Oscillator"),
    ("gun", "Category::Gun"),
    ("spaceship", "Category::Spaceship"),
    ("methuselah", "Category::Methuselah"),
    ("puffer", "Category::Puffer"),
    ("wick", "Category::Wick"),
];

fn main() {
    // Rerun if the top-level patterns directory itself changes (files added/removed).
    println!("cargo:rerun-if-changed=src/patterns");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR must be set by Cargo");
    let out_path = Path::new(&out_dir).join("library_entries.rs");

    let mut code = String::from("static LIBRARY: &[LibraryEntry] = &[\n");

    for &(dir_name, category_expr) in CATEGORIES {
        let dir_path = PathBuf::from("src/patterns").join(dir_name);

        // Rerun if files are added to or removed from this category subdirectory.
        println!("cargo:rerun-if-changed={}", dir_path.display());

        let mut files: Vec<PathBuf> = fs::read_dir(&dir_path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir_path.display()))
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("rle"))
            .map(|entry| entry.path())
            .collect();
        files.sort();

        for file_path in files {
            // Rerun if this specific file's content changes.
            println!("cargo:rerun-if-changed={}", file_path.display());

            let stem = file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_else(|| panic!("non-UTF-8 stem: {}", file_path.display()));
            let name = stem_to_name(stem);

            let content = fs::read_to_string(&file_path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", file_path.display()));

            // Escape backslashes and double-quotes for a Rust string literal.
            let escaped = content.replace('\\', "\\\\").replace('"', "\\\"");

            code.push_str(&format!(
                "    LibraryEntry {{ name: \"{name}\", category: {category_expr}, rle: \"{escaped}\" }},\n"
            ));
        }
    }

    code.push_str("];\n");
    fs::write(&out_path, &code).expect("failed to write library_entries.rs");
}

/// Converts a file stem to a human-readable pattern name.
///
/// The stem is split on `_` into segments; each segment is transformed by the
/// first matching rule below, then all segments are joined with a single space.
///
/// Rules applied per segment:
/// 1. **Contains `-`**: split on `-`, title-case each sub-part, rejoin with `-`.
///    e.g. `r-pentomino` → `"R-Pentomino"`
/// 2. **All chars are ASCII uppercase**: keep as-is.
///    e.g. `LWSS` → `"LWSS"`
/// 3. **Otherwise**: capitalise the first character, leave the rest unchanged.
///    e.g. `boat` → `"Boat"`, `gosper` → `"Gosper"`
fn stem_to_name(stem: &str) -> String {
    stem.split('_')
        .map(|seg| {
            if seg.contains('-') {
                seg.split('-').map(capitalize).collect::<Vec<_>>().join("-")
            } else if !seg.is_empty() && seg.chars().all(|c| c.is_ascii_uppercase()) {
                seg.to_string()
            } else {
                capitalize(seg)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Returns `s` with its first character converted to uppercase; the rest is unchanged.
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}
