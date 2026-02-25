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

            let (description, author, rule) = extract_metadata(&content);
            let desc_lit = opt_str_literal(description.as_deref());
            let auth_lit = opt_str_literal(author.as_deref());
            let rule_lit = opt_str_literal(rule.as_deref());

            code.push_str(&format!(
                "    LibraryEntry {{ name: \"{name}\", category: {category_expr}, rle: \"{escaped}\", \
                 description: {desc_lit}, author: {auth_lit}, rule: {rule_lit} }},\n"
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

/// Extracts description (joined `#C` lines), author (`#O`), and rule from raw RLE content.
///
/// # Arguments
/// * `content` — raw text of an `.rle` file
///
/// # Returns
/// A tuple `(description, author, rule)`, each `None` if the corresponding metadata
/// is absent from the file.
fn extract_metadata(content: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut desc_lines: Vec<String> = Vec::new();
    let mut author: Option<String> = None;
    let mut rule: Option<String> = None;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("#C") || t.starts_with("#c") {
            desc_lines.push(t[2..].trim().to_owned());
        } else if t.starts_with("#O") || t.starts_with("#o") {
            author = Some(t[2..].trim().to_owned());
        } else if t.to_ascii_lowercase().starts_with('x') && t.contains('=') {
            rule = parse_rule_from_header(t);
        }
    }
    let description = if desc_lines.is_empty() {
        None
    } else {
        Some(desc_lines.join("\n"))
    };
    (description, author, rule)
}

/// Parses the `rule = …` value from an RLE header line.
///
/// # Arguments
/// * `header` — the `x = N, y = M[, rule = …]` header line
///
/// # Returns
/// `Some(rule_string)` if a non-empty `rule` field is found, otherwise `None`.
fn parse_rule_from_header(header: &str) -> Option<String> {
    let lower = header.to_ascii_lowercase();
    let pos = lower.find("rule")?;
    let after = &header[pos + 4..];
    let eq = after.find('=')?;
    let value = after[eq + 1..].trim().split(',').next()?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

/// Emits a Rust `Option<&'static str>` literal for a string value.
///
/// Backslashes and double-quotes in `s` are escaped; embedded newlines are kept
/// as-is (valid in Rust string literals).
///
/// # Arguments
/// * `s` — the optional string to render as a literal
///
/// # Returns
/// `"None"` if `s` is `None`, or `Some("…")` with the value properly escaped.
fn opt_str_literal(s: Option<&str>) -> String {
    match s {
        None => "None".to_owned(),
        Some(v) => {
            let escaped = v.replace('\\', "\\\\").replace('"', "\\\"");
            format!("Some(\"{escaped}\")")
        }
    }
}
