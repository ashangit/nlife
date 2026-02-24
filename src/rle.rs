// Items used by library.rs; broader application usage wired in Item 3+.
#![allow(dead_code)]

/// Errors that can occur when parsing a Game of Life pattern file.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// The RLE header line is malformed or missing when one was expected.
    InvalidHeader,
    /// An unexpected character was encountered in the pattern body.
    UnexpectedChar(char),
    /// The input contains no live cells (empty body or all-dead).
    Empty,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::InvalidHeader => write!(f, "invalid RLE header"),
            ParseError::UnexpectedChar(c) => write!(f, "unexpected character '{c}'"),
            ParseError::Empty => write!(f, "pattern is empty"),
        }
    }
}

/// Parse an RLE-encoded Conway's Game of Life pattern into live cell coordinates.
///
/// The top-left of the parsed bounding box is at `(0, 0)`. Optional `#` comment
/// lines and an optional `x = N, y = M[, rule = …]` header line are silently skipped.
///
/// Body tokens:
/// - Digit prefix: repeat count for the next token (omitted = 1)
/// - `b` or `.`: dead cell(s) — advance column by count
/// - `o` or `O`: alive cell(s) — emit `count` cells, advance column
/// - `$`: end of row — advance row by count, reset column to 0
/// - `!`: end of pattern — stop parsing
///
/// Trailing dead cells per row are optional and ignored.
///
/// # Arguments
/// * `input` — raw RLE string (may include comment/header lines)
///
/// # Returns
/// `Ok(cells)` with `(row, col)` pairs (top-left = `(0, 0)`), or
/// `Err(ParseError)` if the body is empty or contains illegal characters.
pub fn parse_rle(input: &str) -> Result<Vec<(i32, i32)>, ParseError> {
    let mut cells: Vec<(i32, i32)> = Vec::new();
    let mut row = 0i32;
    let mut col = 0i32;
    let mut count_acc = 0u32;
    let mut found_body = false;

    for line in input.lines() {
        let trimmed = line.trim();

        // Skip comment lines.
        if trimmed.starts_with('#') {
            continue;
        }

        // Skip the optional header line (`x = N, y = M[, rule = ...]`).
        if trimmed.to_ascii_lowercase().starts_with('x') && trimmed.contains('=') {
            continue;
        }

        // Anything else is body content.
        found_body = true;

        for ch in trimmed.chars() {
            match ch {
                '0'..='9' => {
                    count_acc = count_acc * 10 + (ch as u32 - '0' as u32);
                }
                'b' | '.' => {
                    let n = if count_acc == 0 { 1 } else { count_acc };
                    col += n as i32;
                    count_acc = 0;
                }
                'o' | 'O' => {
                    let n = if count_acc == 0 { 1 } else { count_acc };
                    for i in 0..n as i32 {
                        cells.push((row, col + i));
                    }
                    col += n as i32;
                    count_acc = 0;
                }
                '$' => {
                    let n = if count_acc == 0 { 1 } else { count_acc };
                    row += n as i32;
                    col = 0;
                    count_acc = 0;
                }
                '!' => {
                    if cells.is_empty() {
                        return Err(ParseError::Empty);
                    }
                    return Ok(cells);
                }
                ' ' | '\t' | '\r' => {
                    // Whitespace within a body line is allowed.
                }
                other => {
                    return Err(ParseError::UnexpectedChar(other));
                }
            }
        }
    }

    if !found_body || cells.is_empty() {
        return Err(ParseError::Empty);
    }

    Ok(cells)
}

/// Parse a `.cells` plaintext Conway's Game of Life pattern into live cell coordinates.
///
/// The `.cells` format uses `!` to introduce comment lines (the rest of the line is
/// ignored), `.` for dead cells, and `O` (capital letter O, or lowercase `o`, or `*`)
/// for alive cells. Each non-comment line represents one grid row; all rows are
/// consumed even if they contain only dead cells.
///
/// The top-left of the parsed bounding box is at `(0, 0)`.
///
/// # Arguments
/// * `input` — raw `.cells` string
///
/// # Returns
/// `Ok(cells)` with `(row, col)` pairs, or `Err(ParseError)` if the input contains
/// illegal characters or has no live cells.
pub fn parse_cells(input: &str) -> Result<Vec<(i32, i32)>, ParseError> {
    let mut cells: Vec<(i32, i32)> = Vec::new();
    let mut row = 0i32;

    for line in input.lines() {
        // Lines starting with '!' are comments.
        if line.starts_with('!') {
            continue;
        }

        let mut col = 0i32;
        for ch in line.chars() {
            match ch {
                '.' => {
                    col += 1;
                }
                'O' | 'o' | '*' => {
                    cells.push((row, col));
                    col += 1;
                }
                '\r' => {}
                other => {
                    return Err(ParseError::UnexpectedChar(other));
                }
            }
        }
        row += 1;
    }

    if cells.is_empty() {
        return Err(ParseError::Empty);
    }

    Ok(cells)
}

/// Shift cells so their bounding-box centre aligns with `(0, 0)`.
///
/// The centre is computed as `(row_min + row_max) / 2` and
/// `(col_min + col_max) / 2` using integer division, so for odd-dimension
/// bounding boxes the centre rounds towards the top-left.
///
/// # Arguments
/// * `cells` — list of `(row, col)` cell coordinates
///
/// # Returns
/// A new `Vec` with each coordinate shifted so the bounding-box centre is at `(0, 0)`.
pub fn center_cells(cells: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    if cells.is_empty() {
        return cells;
    }
    let row_min = cells.iter().map(|&(r, _)| r).min().unwrap();
    let row_max = cells.iter().map(|&(r, _)| r).max().unwrap();
    let col_min = cells.iter().map(|&(_, c)| c).min().unwrap();
    let col_max = cells.iter().map(|&(_, c)| c).max().unwrap();
    let dr = (row_min + row_max) / 2;
    let dc = (col_min + col_max) / 2;
    cells.into_iter().map(|(r, c)| (r - dr, c - dc)).collect()
}

/// Serialise live cells as `.cells` plaintext, with top-left normalised to `(0, 0)`.
///
/// The output starts with a `!Name: <name>` comment line, followed by one grid
/// row per line using `.` for dead and `O` for alive cells. The bounding box of
/// `cells` determines the output dimensions.
///
/// # Arguments
/// * `cells` — list of `(row, col)` live cell coordinates (any origin)
/// * `name`  — name written into the header comment line
///
/// # Returns
/// A `String` containing the `.cells` plaintext representation.
pub fn write_cells(cells: &[(i32, i32)], name: &str) -> String {
    if cells.is_empty() {
        return format!("!Name: {name}\n");
    }

    let row_min = cells.iter().map(|&(r, _)| r).min().unwrap();
    let row_max = cells.iter().map(|&(r, _)| r).max().unwrap();
    let col_min = cells.iter().map(|&(_, c)| c).min().unwrap();
    let col_max = cells.iter().map(|&(_, c)| c).max().unwrap();

    let rows = (row_max - row_min + 1) as usize;
    let cols = (col_max - col_min + 1) as usize;

    // Build a flat bool grid for fast lookup.
    let mut grid = vec![false; rows * cols];
    for &(r, c) in cells {
        grid[(r - row_min) as usize * cols + (c - col_min) as usize] = true;
    }

    let mut out = format!("!Name: {name}\n");
    for r in 0..rows {
        let line: String = (0..cols)
            .map(|c| if grid[r * cols + c] { 'O' } else { '.' })
            .collect();
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Load user-defined patterns from a directory of `.cells` files.
///
/// Scans `dir` for files with the `.cells` extension, parses each with
/// [`parse_cells`], and centres the result with [`center_cells`].  Files
/// that fail to parse are silently skipped.  Returns an empty `Vec` if the
/// directory does not exist or cannot be read.
///
/// # Arguments
/// * `dir` — path to the directory to scan
///
/// # Returns
/// A `Vec` of `(name, cells)` pairs, where `name` is the file stem and
/// `cells` is the centred list of live-cell coordinates.
pub fn load_user_patterns(dir: &str) -> Vec<(String, Vec<(i32, i32)>)> {
    let path = std::path::Path::new(dir);
    let Ok(read_dir) = std::fs::read_dir(path) else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for entry in read_dir.flatten() {
        let fpath = entry.path();
        if fpath.extension().and_then(|e| e.to_str()) != Some("cells") {
            continue;
        }
        let name = fpath
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_owned();
        let Ok(content) = std::fs::read_to_string(&fpath) else {
            continue;
        };
        if let Ok(cells) = parse_cells(&content) {
            result.push((name, center_cells(cells)));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Glider RLE — should parse to exactly 5 cells at canonical positions.
    #[test]
    fn test_parse_rle_glider() {
        let rle = "x = 3, y = 3, rule = B3/S23\nbo$2bo$3o!";
        let cells = parse_rle(rle).expect("glider RLE should parse");
        assert_eq!(cells.len(), 5, "glider has 5 live cells");
        // Canonical glider relative to top-left=(0,0):
        // (0,1), (1,2), (2,0), (2,1), (2,2)
        let expected: std::collections::HashSet<(i32, i32)> =
            [(0, 1), (1, 2), (2, 0), (2, 1), (2, 2)]
                .into_iter()
                .collect();
        let got: std::collections::HashSet<(i32, i32)> = cells.into_iter().collect();
        assert_eq!(got, expected);
    }

    /// A 3×3 all-alive block in .cells format should yield 9 cells.
    #[test]
    fn test_parse_cells_3x3_block() {
        let input = "OOO\nOOO\nOOO";
        let cells = parse_cells(input).expect("3×3 block should parse");
        assert_eq!(cells.len(), 9);
    }

    /// center_cells should shift an off-centre list so the bounding-box centre is at (0,0).
    #[test]
    fn test_center_cells() {
        // 2×2 block at (10,20)–(12,22) — bbox centre = (11,21)
        let cells = vec![(10, 20), (10, 22), (12, 20), (12, 22)];
        let centred = center_cells(cells);
        let row_min = centred.iter().map(|&(r, _)| r).min().unwrap();
        let row_max = centred.iter().map(|&(r, _)| r).max().unwrap();
        let col_min = centred.iter().map(|&(_, c)| c).min().unwrap();
        let col_max = centred.iter().map(|&(_, c)| c).max().unwrap();
        assert_eq!((row_min + row_max) / 2, 0);
        assert_eq!((col_min + col_max) / 2, 0);
    }

    /// Completely empty input should return ParseError::Empty.
    #[test]
    fn test_parse_rle_empty() {
        assert_eq!(parse_rle(""), Err(ParseError::Empty));
    }

    /// Round-trip: write_cells then parse_cells should recover the same cell set.
    #[test]
    fn test_write_then_parse_roundtrip() {
        let original: Vec<(i32, i32)> = vec![(0, 1), (1, 0), (1, 2), (2, 0), (2, 1), (2, 2)];
        let written = write_cells(&original, "test");
        let parsed = parse_cells(&written).expect("should parse back");
        let orig_set: std::collections::HashSet<_> = original.into_iter().collect();
        let parsed_set: std::collections::HashSet<_> = parsed.into_iter().collect();
        assert_eq!(orig_set, parsed_set);
    }

    /// load_user_patterns returns empty vec gracefully when directory doesn't exist.
    #[test]
    fn test_load_user_patterns_missing_dir() {
        let result = load_user_patterns("/nonexistent/path/that/does/not/exist/42");
        assert!(result.is_empty());
    }

    /// RLE with comment and multi-line body.
    #[test]
    fn test_parse_rle_with_comments() {
        let rle = "# Comment line\n# Another comment\nx = 2, y = 2\n2o$\n2o!";
        let cells = parse_rle(rle).expect("2×2 block RLE should parse");
        assert_eq!(cells.len(), 4);
    }

    /// parse_cells skips comment lines starting with '!'.
    #[test]
    fn test_parse_cells_skips_comments() {
        let input = "!Name: test\n!Author: someone\nOO\nOO";
        let cells = parse_cells(input).expect("should parse");
        assert_eq!(cells.len(), 4);
    }

    /// write_cells on empty slice produces only the header comment.
    #[test]
    fn test_write_cells_empty() {
        let out = write_cells(&[], "empty");
        assert_eq!(out, "!Name: empty\n");
    }
}
