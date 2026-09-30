"""Save slots on disk (no pygame).

Saves live in a per-user folder, not next to the game, because an
exported game's folder may be read-only:

- Linux:   $XDG_DATA_HOME/vn_engine/<project_id> (~/.local/share/...)
- macOS:   ~/Library/Application Support/vn_engine/<project_id>
- Windows: %APPDATA%\\vn_engine\\<project_id>

VN_ENGINE_SAVE_DIR overrides the base folder (used by tests).

Each slot is <slot>.json ({"meta": ..., "engine": engine.save_state()})
plus an optional <slot>.png thumbnail. Slots are "1".."9", "quick" and
"auto"; persistent data goes to persistent.json.
"""

import json
import os
import sys
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, List, Optional

NUMBERED_SLOTS = [str(n) for n in range(1, 10)]
QUICK = "quick"
AUTO = "auto"


def default_save_root() -> Path:
    override = os.environ.get("VN_ENGINE_SAVE_DIR")
    if override:
        return Path(override)
    if sys.platform == "win32":
        base = Path(os.environ.get("APPDATA", Path.home() / "AppData" / "Roaming"))
    elif sys.platform == "darwin":
        base = Path.home() / "Library" / "Application Support"
    else:
        base = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share"))
    return base / "vn_engine"


class SaveManager:
    def __init__(self, project_id: str, root: Optional[Path] = None) -> None:
        self.folder = (root or default_save_root()) / project_id

    def _json(self, slot: str) -> Path:
        return self.folder / f"{slot}.json"

    def thumbnail_path(self, slot: str) -> Path:
        return self.folder / f"{slot}.png"

    def exists(self, slot: str) -> bool:
        return self._json(slot).exists()

    def save(self, slot: str, engine_state: Dict[str, Any], meta: Dict[str, Any]) -> Path:
        """Write a slot; returns the thumbnail path the caller may fill"""
        self.folder.mkdir(parents=True, exist_ok=True)
        meta = {**meta, "saved_at": datetime.now().isoformat(timespec="seconds")}
        tmp = self._json(slot).with_suffix(".tmp")
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump({"meta": meta, "engine": engine_state}, f, ensure_ascii=False)
        tmp.replace(self._json(slot))  # never leave a half-written save
        return self.thumbnail_path(slot)

    def load(self, slot: str) -> Dict[str, Any]:
        """The engine state stored in a slot"""
        with open(self._json(slot), "r", encoding="utf-8") as f:
            state: Dict[str, Any] = json.load(f)["engine"]
        return state

    def meta(self, slot: str) -> Optional[Dict[str, Any]]:
        try:
            with open(self._json(slot), "r", encoding="utf-8") as f:
                meta: Dict[str, Any] = json.load(f)["meta"]
            return meta
        except (OSError, ValueError, KeyError):
            return None

    def delete(self, slot: str) -> None:
        for path in (self._json(slot), self.thumbnail_path(slot)):
            path.unlink(missing_ok=True)

    def slots(self) -> List[Dict[str, Any]]:
        """Numbered slots with their metadata (None for empty ones)"""
        return [{"slot": s, "meta": self.meta(s)} for s in NUMBERED_SLOTS]

    @property
    def persistent_path(self) -> Path:
        return self.folder / "persistent.json"
