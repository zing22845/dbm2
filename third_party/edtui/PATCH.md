# edtui (vendored patch)

Based on [edtui 0.11.3](https://github.com/preiter93/edtui) (MIT).

## Why

Upstream wrap mode scrolls only by **logical** lines. A single long line that wraps
taller than the viewport never advances `viewport.y`, so the cursor can move past
the visible area while the text stays put.

## Change

When `wrap(true)`:

- `viewport.y` is the first visible **visual** (wrapped) row
- render skips wrap segments (including mid-line) to that offset
- cursor motion past the bottom of the pane scrolls to show later wrap rows
- when a logical line's head is scrolled away, paint blue `<<<` at the start of
  the first visible content row, and **keep the line number** in the gutter
  (so the gutter does not disappear with the scrolled-off wrap segments)

Wired via workspace `[patch.crates-io]` in the root `Cargo.toml`.
