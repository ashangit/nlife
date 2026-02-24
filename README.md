# newlife

A Conway's Game of Life desktop app written in Rust, built with [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui/tree/master/crates/eframe) and rendered via wgpu on Wayland.

## Features

- **Paint & erase** — left-click or drag to toggle cells; right-click or drag to erase
- **Run / Pause / Step** — play at configurable speed (1–1000 gen/s) or advance one generation at a time
- **Auto-expanding grid** — infinite canvas: the grid grows automatically as live cells reach any edge
- **Pattern browser** — left panel with category filter, name search, and miniature previews for 25 built-in patterns:
  - *Still lifes*: Block, Beehive, Loaf, Boat, Tub, Pond, Ship, Long Boat
  - *Oscillators*: Blinker, Toad, Beacon, Pulsar, Pentadecathlon, Figure Eight, Queen Bee Shuttle, Gosper Glider Gun
  - *Spaceships*: Glider, LWSS, MWSS, HWSS, Copperhead, Canada Goose
  - *Methuselahs*: R-Pentomino, Acorn, Diehard
- **User patterns** — save the current grid as a `.cells` file; reload from `~/.config/newlife/patterns/`
- **Zoom** — `+` / `-` keys, `Ctrl+scroll`, or pinch-to-zoom; `0` resets to 100 %

## Build & Run

```bash
cargo run                # debug build + launch
cargo build --release    # optimised binary → target/release/newlife
cargo test               # run all tests
```

Requires a Rust toolchain (edition 2021) and a Wayland compositor.

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Space` | Toggle play / pause |
| `S` | Step one generation (paused only) |
| `R` | Clear the grid |
| `+` / `=` | Zoom in |
| `-` | Zoom out |
| `0` | Reset zoom to 100 % |
| `Ctrl+scroll` | Zoom (mouse-anchored) |

## Architecture

See [CLAUDE.md](CLAUDE.md) for a detailed module map and internals reference.

## License

MIT
