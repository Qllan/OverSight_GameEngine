"""pygame-ce player for VN Engine projects.

Run with: python -m vn_engine.player path/to/project.json
"""

from .stage import Stage

__all__ = ["Stage", "Player"]


def __getattr__(name: str) -> object:
    # Player needs pygame; import it only when asked for
    if name == "Player":
        from .app import Player

        return Player
    raise AttributeError(name)
