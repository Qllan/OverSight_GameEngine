"""pygame-ce window that plays a VN Engine project.

Controls: click / Space / Enter to continue (the first press finishes the
text animation), 1-9 or arrows + Enter to pick a choice, Esc to quit.

Settings come from an optional "player" section in project.json:

    "player": {"title": "My Game", "resolution": [1280, 720],
               "text_speed": 45, "font": "fonts/main"}

Missing image assets are drawn as labelled placeholders, so a game can be
played before its art exists.
"""

import json
import logging
import zlib
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

import pygame

from vn_engine.engine import Engine
from vn_engine.player.stage import POSITIONS, AudioEffect, Stage

log = logging.getLogger(__name__)

TEXT_BOX_HEIGHT = 200
MARGIN = 40
CHOICE_WIDTH = 640
CHOICE_HEIGHT = 58
CHOICE_GAP = 16
FONT_NAMES = "dejavusans,notosans,liberationsans,arial"

BOX_COLOR = (12, 14, 24, 200)
NAME_COLOR = (255, 214, 120)
TEXT_COLOR = (240, 240, 240)
CHOICE_COLOR = (30, 34, 56, 225)
CHOICE_HOVER = (70, 80, 140, 240)


def wrap_text(font: "pygame.font.Font", text: str, width: int) -> List[str]:
    """Split text into lines that fit `width` (keeps explicit newlines)"""
    lines: List[str] = []
    for paragraph in text.split("\n"):
        line = ""
        for word in paragraph.split(" "):
            candidate = f"{line} {word}" if line else word
            if font.size(candidate)[0] <= width:
                line = candidate
                continue
            if line:
                lines.append(line)
            # A single word wider than the box is broken by characters
            while font.size(word)[0] > width and len(word) > 1:
                cut = len(word)
                while cut > 1 and font.size(word[:cut])[0] > width:
                    cut -= 1
                lines.append(word[:cut])
                word = word[cut:]
            line = word
        lines.append(line)
    return lines


def placeholder_color(name: str) -> Tuple[int, int, int]:
    h = zlib.crc32(name.encode())
    return (60 + h % 140, 60 + (h >> 8) % 140, 60 + (h >> 16) % 140)


class Player:
    def __init__(self, project_path: str, window_size: Optional[Tuple[int, int]] = None) -> None:
        self.project_path = Path(project_path)
        with open(self.project_path, "r", encoding="utf-8") as f:
            self.project_data: Dict[str, Any] = json.load(f)
        self.settings: Dict[str, Any] = self.project_data.get("player", {})
        width, height = self.settings.get("resolution", [1280, 720])
        self.size: Tuple[int, int] = (int(width), int(height))
        self.text_speed = float(self.settings.get("text_speed", 45))

        pygame.init()
        self.audio = self._init_audio()
        title = self.settings.get("title", self.project_data.get("project_id", "VN Engine"))
        pygame.display.set_caption(str(title))
        self.window = pygame.display.set_mode(window_size or self.size, pygame.RESIZABLE)
        self.canvas = pygame.Surface(self.size)
        self.clock = pygame.time.Clock()

        self.engine = Engine()
        self.engine.init()
        self.engine.subscribe("BlockCall", self._on_block)
        self.engine.load_project(str(self.project_path))

        self.stage = Stage()
        self.stage_changed = False
        self.state: Optional[Dict[str, Any]] = None
        self.finished = False
        self.running = True
        self.shown_chars = 0.0
        self.selected_choice = 0
        self.frame_count = 0
        self._images: Dict[str, Optional["pygame.Surface"]] = {}
        self._scaled: Dict[Tuple[str, int, int], "pygame.Surface"] = {}
        self._sounds: Dict[str, Optional["pygame.mixer.Sound"]] = {}
        self._voice_channel: Optional["pygame.mixer.Channel"] = None
        # Where the canvas is drawn in the window (for mouse coordinates)
        self._view = (0, 0, 1.0)
        self._load_fonts()

    # --- setup ----------------------------------------------------------

    def _init_audio(self) -> bool:
        try:
            pygame.mixer.init()
            pygame.mixer.set_reserved(1)
            return True
        except pygame.error as e:
            log.warning("Audio disabled: %s", e)
            return False

    def _load_fonts(self) -> None:
        font_asset = self.settings.get("font")
        path = self.engine.asset_path(font_asset) if font_asset else None

        def make(size: int, bold: bool = False) -> "pygame.font.Font":
            if path:
                return pygame.font.Font(path, size)
            return pygame.font.SysFont(FONT_NAMES, size, bold=bold)

        self.text_font = make(30)
        self.name_font = make(30, bold=True)
        self.choice_font = make(28)
        self.small_font = make(20)

    # --- engine ---------------------------------------------------------

    def start(self) -> None:
        self.engine.run()
        self._tick()

    def _tick(self) -> None:
        """Advance the game by one frame and sync the stage"""
        try:
            self.state = self.engine.tick()
        except RuntimeError as e:
            log.error("Game stopped: %s", e)
            self.state = None
        if self.stage_changed:
            self.engine.set_host_state(self.stage.to_json())
            self.stage_changed = False
        if self.state is None:
            self.finished = True

    def _on_block(self, call: Dict[str, Any]) -> None:
        block_type, args = call["block_type"], call["args"]
        if block_type == "ShowText":
            self.shown_chars = 0.0
        if block_type == "BranchChoice":
            self.selected_choice = 0
        effect = self.stage.apply(block_type, args)
        if block_type not in ("ShowText", "BranchChoice"):
            self.stage_changed = True
        if effect is not None:
            self._play(effect)

    def restore_stage(self) -> None:
        """Rebuild the stage after loading a save or rolling back"""
        self.stage = Stage.from_json(self.engine.host_state())
        self.state = self.engine.get_state()
        args = self.state.get("last_block_args") or {}
        if self.state["last_block_type"] == "ShowText":
            self.stage.speaker = str(args.get("speaker") or "")
            self.stage.text = str(args.get("text", ""))
        if self.state["yield_reason"] == "choice":
            self.stage.choices = [str(c) for c in args.get("choices", [])]
        self.shown_chars = float(len(self.stage.text))
        self.finished = not self.state["is_running"]
        if self.stage.music:
            self._play(("music", self.stage.music))
        else:
            self._play(("stop_music", {"fade_ms": 0}))

    # --- player actions -------------------------------------------------

    @property
    def waiting_for(self) -> Optional[str]:
        if self.state is None or not self.state["is_yielding"]:
            return None
        return "choice" if self.state["yield_reason"] == "choice" else "input"

    @property
    def text_complete(self) -> bool:
        return self.shown_chars >= len(self.stage.text)

    def advance(self) -> None:
        """Click / Space: finish the text animation, or continue"""
        if self.finished:
            self.running = False
        elif self.waiting_for == "input":
            if not self.text_complete:
                self.shown_chars = float(len(self.stage.text))
            else:
                self.engine.resume()
                self._tick()

    def choose(self, index: int) -> None:
        if self.waiting_for == "choice" and 0 <= index < len(self.stage.choices):
            self.stage.choices = []
            self.engine.resume(index)
            self._tick()

    # --- loop -----------------------------------------------------------

    def update(self, dt: float) -> None:
        if self.finished:
            return
        if self.waiting_for is None:
            self._tick()
        elif self.waiting_for == "input" and not self.text_complete:
            speed = self.text_speed if self.text_speed > 0 else 1e9
            self.shown_chars = min(len(self.stage.text), self.shown_chars + speed * dt)

    def handle_event(self, event: "pygame.event.Event") -> None:
        if event.type == pygame.QUIT:
            self.running = False
        elif event.type == pygame.KEYDOWN:
            self.handle_key(event.key)
        elif event.type == pygame.MOUSEBUTTONDOWN and event.button == 1:
            self.click(self.to_canvas(event.pos))
        elif event.type == pygame.MOUSEMOTION and self.stage.choices:
            hit = self.choice_at(self.to_canvas(event.pos))
            if hit is not None:
                self.selected_choice = hit

    def handle_key(self, key: int) -> None:
        if key == pygame.K_ESCAPE:
            self.running = False
        elif self.waiting_for == "choice":
            if pygame.K_1 <= key <= pygame.K_9:
                self.choose(key - pygame.K_1)
            elif key in (pygame.K_UP, pygame.K_DOWN):
                step = -1 if key == pygame.K_UP else 1
                self.selected_choice = (self.selected_choice + step) % len(self.stage.choices)
            elif key in (pygame.K_RETURN, pygame.K_KP_ENTER, pygame.K_SPACE):
                self.choose(self.selected_choice)
        elif key in (pygame.K_SPACE, pygame.K_RETURN, pygame.K_KP_ENTER, pygame.K_RIGHT):
            self.advance()

    def click(self, pos: Tuple[float, float]) -> None:
        if self.waiting_for == "choice":
            hit = self.choice_at(pos)
            if hit is not None:
                self.choose(hit)
        else:
            self.advance()

    def frame(self) -> None:
        """One iteration of the main loop"""
        dt = self.clock.tick(60) / 1000.0
        for event in pygame.event.get():
            self.handle_event(event)
        self.update(dt)
        self.draw()
        self.present()
        self.frame_count += 1

    def run(self, max_frames: Optional[int] = None) -> None:
        self.start()
        while self.running and (max_frames is None or self.frame_count < max_frames):
            self.frame()
        self.close()

    def close(self) -> None:
        if self.audio:
            pygame.mixer.stop()
            pygame.mixer.music.stop()

    # --- drawing --------------------------------------------------------

    def to_canvas(self, pos: Tuple[int, int]) -> Tuple[float, float]:
        x, y, scale = self._view
        return ((pos[0] - x) / scale, (pos[1] - y) / scale)

    def present(self) -> None:
        """Scale the canvas into the window, keeping the aspect ratio"""
        win_w, win_h = self.window.get_size()
        scale = min(win_w / self.size[0], win_h / self.size[1])
        w, h = int(self.size[0] * scale), int(self.size[1] * scale)
        x, y = (win_w - w) // 2, (win_h - h) // 2
        self._view = (x, y, scale)
        self.window.fill((0, 0, 0))
        if (w, h) == self.size:
            self.window.blit(self.canvas, (x, y))
        else:
            self.window.blit(pygame.transform.smoothscale(self.canvas, (w, h)), (x, y))
        pygame.display.flip()

    def draw(self) -> None:
        self._draw_background()
        for name, character in self.stage.characters.items():
            self._draw_character(name, character)
        if self.stage.choices and self.waiting_for == "choice":
            self._draw_text_box(complete=True)
            self._draw_choices()
        elif self.stage.text:
            self._draw_text_box(complete=self.text_complete)
        if self.finished:
            self._draw_center_label("The End  -  click to close")

    def image(self, asset_id: str) -> Optional["pygame.Surface"]:
        if asset_id not in self._images:
            path = self.engine.asset_path(asset_id)
            surface = None
            if path:
                try:
                    surface = pygame.image.load(path).convert_alpha()
                except (pygame.error, FileNotFoundError) as e:
                    log.warning("Cannot load %s: %s", path, e)
            self._images[asset_id] = surface
        return self._images[asset_id]

    def _scaled_image(self, asset_id: str, surface: "pygame.Surface",
                      size: Tuple[int, int]) -> "pygame.Surface":
        key = (asset_id, size[0], size[1])
        if key not in self._scaled:
            self._scaled[key] = pygame.transform.smoothscale(surface, size)
        return self._scaled[key]

    def _draw_background(self) -> None:
        w, h = self.size
        bg = self.image(self.stage.background) if self.stage.background else None
        if bg is None:
            top = placeholder_color(self.stage.background or "")
            for y in range(0, h, 8):
                t = y / h
                color = tuple(int(c * (1 - 0.6 * t)) for c in top)
                pygame.draw.rect(self.canvas, color, (0, y, w, 8))
            if self.stage.background:
                label = self.small_font.render(f"background: {self.stage.background}", True,
                                               (230, 230, 230))
                self.canvas.blit(label, (16, 12))
            return
        # Cover the canvas, cropping the overflow
        scale = max(w / bg.get_width(), h / bg.get_height())
        size = (int(bg.get_width() * scale) + 1, int(bg.get_height() * scale) + 1)
        scaled = self._scaled_image(self.stage.background or "", bg, size)
        self.canvas.blit(scaled, ((w - size[0]) // 2, (h - size[1]) // 2))

    def _draw_character(self, name: str, character: Dict[str, str]) -> None:
        w, h = self.size
        center_x = int(w * POSITIONS.get(character["position"], 0.5))
        max_h = int(h * 0.85)
        sprite = self.image(character["image"])
        if sprite is None:
            box_w, box_h = int(h * 0.3), max_h
            rect = pygame.Rect(center_x - box_w // 2, h - box_h, box_w, box_h)
            pygame.draw.rect(self.canvas, placeholder_color(name), rect, border_radius=24)
            label = self.name_font.render(name, True, (255, 255, 255))
            self.canvas.blit(label, label.get_rect(center=(center_x, h - box_h + 40)))
            return
        scale = min(max_h / sprite.get_height(), 1.0 if sprite.get_height() < max_h else 10.0)
        size = (int(sprite.get_width() * scale), int(sprite.get_height() * scale))
        scaled = self._scaled_image(character["image"], sprite, size)
        self.canvas.blit(scaled, (center_x - size[0] // 2, h - size[1]))

    def _panel(self, rect: "pygame.Rect", color: Tuple[int, int, int, int]) -> None:
        panel = pygame.Surface(rect.size, pygame.SRCALPHA)
        pygame.draw.rect(panel, color, panel.get_rect(), border_radius=14)
        self.canvas.blit(panel, rect.topleft)

    def text_box_rect(self) -> "pygame.Rect":
        w, h = self.size
        return pygame.Rect(MARGIN, h - TEXT_BOX_HEIGHT - MARGIN // 2, w - 2 * MARGIN,
                           TEXT_BOX_HEIGHT)

    def _draw_text_box(self, complete: bool) -> None:
        box = self.text_box_rect()
        self._panel(box, BOX_COLOR)
        x, y = box.x + 28, box.y + 20
        if self.stage.speaker:
            name = self.name_font.render(self.stage.speaker, True, NAME_COLOR)
            self.canvas.blit(name, (x, y))
            y += name.get_height() + 8
        visible = self.stage.text[: int(self.shown_chars)] if not complete else self.stage.text
        for line in wrap_text(self.text_font, visible, box.width - 56):
            surface = self.text_font.render(line, True, TEXT_COLOR)
            self.canvas.blit(surface, (x, y))
            y += self.text_font.get_linesize()
        if complete and self.waiting_for == "input":
            # "continue" marker, bobbing
            bob = (self.frame_count // 20) % 2 * 4
            tip = (box.right - 36, box.bottom - 26 + bob)
            pygame.draw.polygon(self.canvas, NAME_COLOR,
                                [(tip[0] - 9, tip[1] - 10), (tip[0] + 9, tip[1] - 10), tip])

    def choice_rects(self) -> List["pygame.Rect"]:
        w, h = self.size
        count = len(self.stage.choices)
        total = count * CHOICE_HEIGHT + (count - 1) * CHOICE_GAP
        top = (h - TEXT_BOX_HEIGHT - total) // 2
        return [
            pygame.Rect((w - CHOICE_WIDTH) // 2, top + i * (CHOICE_HEIGHT + CHOICE_GAP),
                        CHOICE_WIDTH, CHOICE_HEIGHT)
            for i in range(count)
        ]

    def choice_at(self, pos: Tuple[float, float]) -> Optional[int]:
        for index, rect in enumerate(self.choice_rects()):
            if rect.collidepoint(pos):
                return index
        return None

    def _draw_choices(self) -> None:
        for index, (label, rect) in enumerate(zip(self.stage.choices, self.choice_rects())):
            selected = index == self.selected_choice
            self._panel(rect, CHOICE_HOVER if selected else CHOICE_COLOR)
            text = self.choice_font.render(f"{index + 1}.  {label}", True, TEXT_COLOR)
            self.canvas.blit(text, text.get_rect(center=rect.center))

    def _draw_center_label(self, text: str) -> None:
        w, h = self.size
        surface = self.choice_font.render(text, True, TEXT_COLOR)
        rect = surface.get_rect(center=(w // 2, h // 2))
        self._panel(rect.inflate(60, 30), BOX_COLOR)
        self.canvas.blit(surface, rect)

    # --- audio ----------------------------------------------------------

    def _sound(self, asset_id: str) -> Optional["pygame.mixer.Sound"]:
        if asset_id not in self._sounds:
            path = self.engine.asset_path(asset_id)
            sound = None
            if path and self.audio:
                try:
                    sound = pygame.mixer.Sound(path)
                except (pygame.error, FileNotFoundError) as e:
                    log.warning("Cannot load %s: %s", path, e)
            self._sounds[asset_id] = sound
        return self._sounds[asset_id]

    def _play(self, effect: AudioEffect) -> None:
        if not self.audio:
            return
        kind, args = effect
        try:
            if kind == "music":
                path = self.engine.asset_path(args["id"])
                if path:
                    pygame.mixer.music.load(path)
                    pygame.mixer.music.set_volume(float(args.get("volume", 0.8)))
                    pygame.mixer.music.play(loops=-1 if args.get("loop", True) else 0)
            elif kind == "stop_music":
                pygame.mixer.music.fadeout(int(args.get("fade_ms", 500)))
            elif kind in ("sound", "voice"):
                sound = self._sound(args["id"])
                if sound is None:
                    return
                sound.set_volume(float(args.get("volume", 1.0)))
                if kind == "voice":
                    # One voice at a time on the reserved channel
                    if self._voice_channel is None:
                        self._voice_channel = pygame.mixer.Channel(0)
                    self._voice_channel.play(sound)
                else:
                    sound.play()
        except pygame.error as e:
            log.warning("Audio error: %s", e)
