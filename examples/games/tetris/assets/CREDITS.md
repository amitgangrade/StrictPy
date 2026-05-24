# Tetris game asset credits

| File | Source | Licence |
|---|---|---|
| `move.wav` | Generated in-tree by `_generate_assets.py` (30 ms low-pitch click) | CC0 / public domain |
| `rotate.wav` | Generated in-tree by `_generate_assets.py` (40 ms two-tone chirp) | CC0 / public domain |
| `clear.wav` | Generated in-tree by `_generate_assets.py` (200 ms triad chord) | CC0 / public domain |
| `tetris.wav` | Generated in-tree by `_generate_assets.py` (350 ms ascending arpeggio + sustain) | CC0 / public domain |
| `gameover.wav` | Generated in-tree by `_generate_assets.py` (600 ms descending sweep) | CC0 / public domain |
| `font.ttf` | Copy of `vm/tests/fixtures/games/DejaVuSansMono.ttf` (Bitstream Vera / DejaVu) | Public domain for redistribution; see https://dejavu-fonts.github.io/License.html |

All five WAVs regenerate deterministically — drop in nicer SFX by overwriting the same paths.
Background music (`music.ogg`) is documented as optional in GAMES_PLAN.md M56 and not shipped in v1.
