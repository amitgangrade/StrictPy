# Space Shooter game asset credits

| File | Source | Licence |
|---|---|---|
| `shoot.wav` | Generated in-tree by `_generate_assets.py` (70ms 900→1500Hz square-wave laser sweep) | CC0 / public domain |
| `explosion.wav` | Generated in-tree by `_generate_assets.py` (320ms 300→50Hz descending square-wave sweep) | CC0 / public domain |
| `hit.wav` | Generated in-tree by `_generate_assets.py` (90ms+90ms 200Hz/120Hz square-wave buzz) | CC0 / public domain |
| `gameover.wav` | Generated in-tree by `_generate_assets.py` (600ms 330→40Hz descending square-wave sweep) | CC0 / public domain |
| `font.ttf` | Copy of `vm/tests/fixtures/games/DejaVuSansMono.ttf` (Bitstream Vera / DejaVu) | Bitstream Vera licence — effectively public domain for redistribution; see https://dejavu-fonts.github.io/License.html |
| `_generate_assets.py` | This repo | MIT/Apache-2.0 (matches workspace) |

All artwork is drawn at runtime with `gfx` primitives (filled
triangles, rectangles, lines, points) — there are no image/sprite
assets, so there is nothing to license on the visual side.

The WAVs are regenerated deterministically by `_generate_assets.py`, so
swapping in nicer SFX is a one-file change.  Drop a replacement WAV in
this directory and the game picks it up at next run.  `gameover.wav` is
generated but not yet wired into the game (reserved for a future
defeat-jingle on game over).
