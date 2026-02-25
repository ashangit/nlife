# newlife

A Conway's Game of Life desktop app written in Rust, built with [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui/tree/master/crates/eframe) and rendered via wgpu on Wayland.

## Features

- **Paint & erase** — left-click or drag to toggle cells; right-click or drag to erase
- **Run / Pause / Step** — play at configurable speed (1–60 gen/s) or advance one generation at a time
- **Auto-expanding grid** — infinite canvas: the grid grows automatically as live cells reach any edge; also auto-resizes when a large pattern is loaded
- **Pattern browser** — left panel with category filter, name search, and miniature previews for 25 built-in patterns:
  - *Still lifes*: Block, Beehive, Loaf, Boat, Tub, Pond, Ship, Long Boat
  - *Oscillators*: Blinker, Toad, Beacon, Pulsar, Pentadecathlon, Figure Eight, Queen Bee Shuttle, Gosper Glider Gun
  - *Spaceships*: Glider, LWSS, MWSS, HWSS, Copperhead, Canada Goose
  - *Methuselahs*: R-Pentomino, Acorn, Diehard
- **User patterns** — save the current grid as a `.cells` file; reload from `~/.config/newlife/patterns/`; native file-dialog save (💾) and load (📂) buttons
- **Zoom** — `+` / `-` keys, `Ctrl+scroll`, or pinch-to-zoom; `0` resets to 100 %; all zoom actions use a smooth animation
- **Grid lines** — toggle with `G` when cell size ≥ 4 px
- **Random fill** — 🎲 button fills the grid with a configurable density (1–100 %)
- **Population counter & sparkline** — live cell count and rolling 128-sample history bar chart shown in the top panel
- **Coordinate tooltip** — hover over the canvas to see the `(row, col)` of any cell
- **Keyboard cheat-sheet** — press `F1` to show / hide a shortcut reference overlay

## Build & Run

```bash
cargo run                # debug build + launch
cargo build --release    # optimised binary → target/release/newlife
cargo test               # run all tests
make release bump=patch  # bump version (patch/minor/major), commit, and tag
```

Requires a Rust toolchain (edition 2021) and a Wayland compositor.

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Space` | Toggle play / pause |
| `S` | Step one generation (paused only) |
| `R` | Clear the grid |
| `G` | Toggle grid lines |
| `+` / `=` | Zoom in |
| `-` | Zoom out |
| `0` | Reset zoom to 100 % |
| `Ctrl+scroll` | Zoom (mouse-anchored) |
| `F1` | Show / hide keyboard cheat-sheet |

## Architecture

See [CLAUDE.md](CLAUDE.md) for a detailed module map and internals reference.

## License

MIT
