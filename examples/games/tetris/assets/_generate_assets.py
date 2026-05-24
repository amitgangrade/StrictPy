"""Generate Tetris SFX as PCM WAV files.

Run from the repo root:  python3 examples/games/tetris/assets/_generate_assets.py

move.wav     — 30ms low-pitch click (piece moves left/right)
rotate.wav   — 40ms two-tone chirp (piece rotates)
clear.wav    — 200ms ascending chord (1-3 lines cleared)
tetris.wav   — 350ms triumphant fanfare (4 lines cleared at once)
gameover.wav — 600ms descending dirge (game over)
"""

import struct
import wave
from pathlib import Path

HERE = Path(__file__).parent
SR = 44100


def square_wave(freq_hz, duration_ms, amp=0.4):
    n = int(SR * duration_ms / 1000)
    samples = []
    period = SR / max(1.0, freq_hz)
    fade_n = int(SR * 0.005)
    for i in range(n):
        phase = (i % period) / period
        v = 1.0 if phase < 0.5 else -1.0
        env = 1.0
        if i < fade_n:
            env = i / fade_n
        elif i > n - fade_n:
            env = max(0.0, (n - i) / fade_n)
        samples.append(int(v * amp * env * 32767))
    return samples


def sweep(start_hz, end_hz, duration_ms, amp=0.4):
    n = int(SR * duration_ms / 1000)
    samples = []
    phase = 0.0
    fade_n = int(SR * 0.005)
    for i in range(n):
        t = i / n
        f = start_hz + (end_hz - start_hz) * t
        phase += f / SR
        v = 1.0 if (phase % 1.0) < 0.5 else -1.0
        env = 1.0
        if i < fade_n:
            env = i / fade_n
        elif i > n - fade_n:
            env = max(0.0, (n - i) / fade_n)
        samples.append(int(v * amp * env * 32767))
    return samples


def mix(a, b):
    """Sample-wise average of two equal-length lists (mono->mono mixdown)."""
    n = min(len(a), len(b))
    return [(a[i] + b[i]) // 2 for i in range(n)]


def write_wav(path, samples):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", s) for s in samples))


def main():
    write_wav(HERE / "move.wav", square_wave(180.0, 30))
    write_wav(HERE / "rotate.wav",
              square_wave(440.0, 20) + square_wave(660.0, 20))
    # Triad: root + third + fifth ringing together briefly.
    triad = mix(
        mix(square_wave(523.0, 200), square_wave(659.0, 200)),
        square_wave(784.0, 200),
    )
    write_wav(HERE / "clear.wav", triad)
    # Fanfare: ascending arpeggio then sustained high note.
    fanfare = (
        square_wave(523.0, 60)
        + square_wave(659.0, 60)
        + square_wave(784.0, 60)
        + square_wave(1046.0, 170)
    )
    write_wav(HERE / "tetris.wav", fanfare)
    write_wav(HERE / "gameover.wav", sweep(330.0, 60.0, 600))
    for name in ("move.wav", "rotate.wav", "clear.wav", "tetris.wav", "gameover.wav"):
        p = HERE / name
        print(f"wrote {p} ({p.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
