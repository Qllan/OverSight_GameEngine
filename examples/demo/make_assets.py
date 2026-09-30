"""Generate the demo's placeholder art and sounds (run once; output is committed).

python examples/demo/make_assets.py
"""

import math
import os
import struct
import wave
from pathlib import Path

os.environ.setdefault("SDL_VIDEODRIVER", "dummy")
import pygame  # noqa: E402

ROOT = Path(__file__).parent / "assets"
W, H = 1280, 720


def gradient(top, bottom, path, extras=None):
    surface = pygame.Surface((W, H))
    for y in range(H):
        t = y / H
        color = [int(a + (b - a) * t) for a, b in zip(top, bottom)]
        pygame.draw.line(surface, color, (0, y), (W, y))
    if extras:
        extras(surface)
    path.parent.mkdir(parents=True, exist_ok=True)
    pygame.image.save(surface, str(path))


def park(surface):
    pygame.draw.circle(surface, (255, 236, 170), (1050, 140), 70)
    pygame.draw.rect(surface, (86, 150, 80), (0, 470, W, 250))
    for x in range(40, W, 180):
        pygame.draw.rect(surface, (95, 70, 50), (x + 40, 330, 24, 150))
        pygame.draw.circle(surface, (60, 120, 60), (x + 52, 310), 70)


def cafe(surface):
    pygame.draw.rect(surface, (120, 80, 60), (0, 500, W, 220))
    for x in range(80, W, 260):
        pygame.draw.rect(surface, (250, 220, 150), (x, 120, 160, 200), border_radius=8)
        pygame.draw.rect(surface, (90, 60, 45), (x, 120, 160, 200), 6, border_radius=8)
    pygame.draw.ellipse(surface, (80, 55, 40), (480, 520, 320, 60))


def character(color, hair, path, smile=True):
    w, h = 420, 640
    s = pygame.Surface((w, h), pygame.SRCALPHA)
    pygame.draw.ellipse(s, color, (70, 300, 280, 360))           # body
    pygame.draw.circle(s, (250, 220, 195), (210, 190), 110)       # face
    pygame.draw.ellipse(s, hair, (90, 60, 240, 130))              # hair
    for x in (170, 250):
        pygame.draw.circle(s, (40, 40, 60), (x, 190), 12)         # eyes
    mouth = (175, 215, 70, 50)
    pygame.draw.arc(s, (150, 60, 60), mouth, math.pi * (1.1 if smile else 0.1),
                    math.pi * (1.9 if smile else 0.9), 5)
    path.parent.mkdir(parents=True, exist_ok=True)
    pygame.image.save(s, str(path))


def tone(path, notes, seconds, volume=0.3, rate=22050):
    frames = bytearray()
    for i in range(int(seconds * rate)):
        t = i / rate
        envelope = min(1.0, t * 8) * min(1.0, (seconds - t) * 4)
        value = sum(math.sin(2 * math.pi * f * t) for f in notes) / len(notes)
        frames += struct.pack("<h", int(value * envelope * volume * 32767))
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(rate)
        f.writeframes(bytes(frames))


if __name__ == "__main__":
    pygame.init()
    gradient((130, 190, 240), (230, 240, 250), ROOT / "bg" / "park.png", park)
    gradient((70, 45, 35), (150, 100, 70), ROOT / "bg" / "cafe.png", cafe)
    character((230, 110, 140), (90, 50, 30), ROOT / "characters" / "mia_happy.png")
    character((230, 110, 140), (90, 50, 30), ROOT / "characters" / "mia_sad.png", smile=False)
    tone(ROOT / "audio" / "theme.wav", [261.6, 329.6, 392.0], 3.0, volume=0.15)
    tone(ROOT / "audio" / "chime.wav", [880.0, 1318.5], 0.4)
    print("assets written to", ROOT)
