"""Generate Space Shooter SFX as PCM WAV files.

Run this from the repo root:
    python3 examples/games/space_shooter/assets/_generate_assets.py

shoot.wav     — 70ms rising square-wave blip, 900Hz -> 1500Hz (laser pew)
explosion.wav — 320ms descending square-wave sweep, 300Hz -> 50Hz (boom)
hit.wav       — 180ms two-tone square-wave buzz, 200Hz then 120Hz (player hit)
gameover.wav  — 600ms long descending sweep, 330Hz -> 40Hz (defeat)

Modeled on examples/games/snake/assets/_generate_assets.py (same
square_wave / sweep / write_wav helpers).
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
    # shoot.wav: short rising laser blip.
    shoot = sweep(900.0, 1500.0, 70)
    write_wav(HERE / "shoot.wav", shoot)
    print(f"wrote {HERE / 'shoot.wav'} ({len(shoot)} samples)")

    # explosion.wav: descending boom sweep.
    explosion = sweep(300.0, 50.0, 320)
    write_wav(HERE / "explosion.wav", explosion)
    print(f"wrote {HERE / 'explosion.wav'} ({len(explosion)} samples)")

    # hit.wav: two-tone low buzz when the player takes damage.
    hit = square_wave(200.0, 90) + square_wave(120.0, 90)
    write_wav(HERE / "hit.wav", hit)
    print(f"wrote {HERE / 'hit.wav'} ({len(hit)} samples)")

    # gameover.wav: long descending defeat sweep.
    gameover = sweep(330.0, 40.0, 600)
    write_wav(HERE / "gameover.wav", gameover)
    print(f"wrote {HERE / 'gameover.wav'} ({len(gameover)} samples)")


if __name__ == "__main__":
    main()
