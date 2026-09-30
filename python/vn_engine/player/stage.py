"""What is on stage, as a plain data model (no pygame).

The stage is changed by the novel_plugin blocks and restored from the
engine's host state after loading a save or rolling back.
"""

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple

#: Horizontal centre of a character, as a fraction of the screen width
POSITIONS: Dict[str, float] = {
    "far_left": 0.12,
    "left": 0.27,
    "center": 0.5,
    "right": 0.73,
    "far_right": 0.88,
}

#: Sound to play: ("music" | "stop_music" | "sound" | "voice", arguments)
AudioEffect = Tuple[str, Dict[str, Any]]


@dataclass
class Stage:
    background: Optional[str] = None
    #: name -> {"image", "position"}, in drawing order
    characters: Dict[str, Dict[str, str]] = field(default_factory=dict)
    #: {"id", "volume", "loop"} while music plays
    music: Optional[Dict[str, Any]] = None
    speaker: str = ""
    text: str = ""
    choices: List[str] = field(default_factory=list)

    def apply(self, block_type: str, args: Dict[str, Any]) -> Optional[AudioEffect]:
        """Update the stage for a block call; returns a sound to play, if any"""
        if block_type == "ShowText":
            self.speaker = str(args.get("speaker") or "")
            self.text = str(args.get("text", ""))
            self.choices = []
        elif block_type == "BranchChoice":
            self.choices = [str(c) for c in args.get("choices", [])]
        elif block_type == "ShowBackground":
            self.background = args["image"]
        elif block_type == "ShowCharacter":
            name = args["character"]
            self.characters.pop(name, None)  # re-shown characters come to the front
            self.characters[name] = {
                "image": args["image"],
                "position": args.get("position", "center"),
            }
        elif block_type == "HideCharacter":
            self.characters.pop(args["character"], None)
        elif block_type == "PlayMusic":
            self.music = {
                "id": args["music"],
                "volume": args.get("volume", 0.8),
                "loop": args.get("loop", True),
            }
            return ("music", dict(self.music))
        elif block_type == "StopMusic":
            self.music = None
            return ("stop_music", {"fade_ms": args.get("fade_ms", 500)})
        elif block_type == "PlaySound":
            return ("sound", {"id": args["sound"], "volume": args.get("volume", 1.0)})
        elif block_type == "PlayVoice":
            return ("voice", {"id": args["voice_id"], "volume": args.get("volume", 1.0)})
        return None

    def to_json(self) -> Dict[str, Any]:
        """The lasting part of the stage (text and choices come from the VM)"""
        return {
            "background": self.background,
            "characters": [{"name": n, **c} for n, c in self.characters.items()],
            "music": self.music,
        }

    @classmethod
    def from_json(cls, data: Any) -> "Stage":
        stage = cls()
        if isinstance(data, dict):
            stage.background = data.get("background")
            for character in data.get("characters", []):
                stage.characters[character["name"]] = {
                    "image": character["image"],
                    "position": character.get("position", "center"),
                }
            stage.music = data.get("music")
        return stage
