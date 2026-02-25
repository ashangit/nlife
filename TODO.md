# TODO — Newlife Improvement Backlog

Items are grouped by theme. Within each section, entries are roughly ordered from
highest to lowest impact / easiest to hardest.

---

## 1. Performance — Simulation Engine

- [ ] **HashLife algorithm** — implement the quadtree-based HashLife algorithm
  (see <https://johnhw.github.io/hashlife/index.md.html>) for O(log N) amortised steps per
  generation on periodic or highly-repetitive patterns.  HashLife memoises 2^k × 2^k
  quadtree nodes by content hash, enabling exponential time-leaps; it is the standard
  algorithm for long-running complex patterns such as guns, methuselahs, and
  self-replicators where the current SWAR frontier approach scales linearly.

---

