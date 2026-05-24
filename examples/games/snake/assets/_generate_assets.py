"""Generate snake game SFX as PCM WAV files.

Run this from the repo root:  python3 examples/games/snake/assets/_generate_assets.py

eat.wav  — 80ms ascending double-beep, 800Hz then 1200Hz (square wave)
die.wav  — 400ms descending sweep, 220Hz -> 60Hz (square wave)
"""

import struct
import wave
from pathlib import Path

HERE = Path(__file__).parent
SR = 44100


def square_wave(freq_hz, duration_ms, amp=0.45):
    """Return a list of 16-bit signed PCM samples for one mono square wave."""
    n = int(SR * duration_ms / 1000)
    samples = []
    period = SR / max(1.0, freq_hz)
    for i in range(n):
        phase = (i % period) / period
        v = 1.0 if phase < 0.5 else -1.0
        # Linear fade-in/fade-out 5ms to avoid clicks
        fade_n = int(SR * 0.005)
        env = 1.0
        if i < fade_n:
            env = i / fade_n
        elif i > n - fade_n:
            env = max(0.0, (n - i) / fade_n)
        samples.append(int(v * amp * env * 32767))
    return samples


def sweep(start_hz, end_hz, duration_ms, amp=0.45):
    """Linear-sweep square wave from start_hz to end_hz."""
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


def write_wav(path, samples):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", s) for s in samples))


def main():
    # eat.wav: short rising double-beep
    eat = square_wave(800.0, 40) + square_wave(1200.0, 40)
    write_wav(HERE / "eat.wav", eat)
    print(f"wrote {HERE / 'eat.wav'} ({len(eat)} samples)")

    # die.wav: descending sweep
    die = sweep(220.0, 60.0, 400)
    write_wav(HERE / "die.wav", die)
    print(f"wrote {HERE / 'die.wav'} ({len(die)} samples)")


if __name__ == "__main__":
    main()
