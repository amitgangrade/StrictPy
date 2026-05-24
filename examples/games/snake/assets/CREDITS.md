# Snake game asset credits

| File | Source | Licence |
|---|---|---|
| `eat.wav` | Generated in-tree by `_generate_assets.py` (40ms+40ms square-wave double-beep) | CC0 / public domain |
| `die.wav` | Generated in-tree by `_generate_assets.py` (400ms descending square-wave sweep) | CC0 / public domain |
| `font.ttf` | Copy of `vm/tests/fixtures/games/DejaVuSansMono.ttf` (Bitstream Vera / DejaVu) | Bitstream Vera licence — effectively public domain for redistribution; see https://dejavu-fonts.github.io/License.html |
| `_generate_assets.py` | This repo | MIT/Apache-2.0 (matches workspace) |

The two WAVs are regenerated deterministically by `_generate_assets.py`, so
swapping in nicer SFX is a one-file change.  Drop a replacement WAV in this
directory and the game picks it up at next run.
