"""python -m vn_engine.player project.json [options]"""

import argparse
import logging
import sys
from pathlib import Path
from typing import List, Optional


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(prog="vn_engine.player", description=__doc__)
    parser.add_argument("project", help="path to project.json")
    parser.add_argument("--size", help="window size, e.g. 1280x720")
    parser.add_argument("--no-title", action="store_true", help="start the game right away")
    parser.add_argument("--save-dir", help="folder for saves (default: user data folder)")
    parser.add_argument("--frames", type=int, help="quit after this many frames (testing)")
    parser.add_argument("--autoplay", action="store_true",
                        help="start right away, continue automatically and pick the first "
                             "choice (testing)")
    parser.add_argument("--screenshot", help="save the last frame to this PNG file")
    args = parser.parse_args(argv)
    logging.basicConfig(level=logging.WARNING)

    from vn_engine.player.app import Player

    size = tuple(int(v) for v in args.size.split("x")) if args.size else None
    save_root = Path(args.save_dir) if args.save_dir else None
    player = Player(args.project, window_size=size, save_root=save_root)  # type: ignore[arg-type]
    if args.no_title or args.autoplay:
        player.start()
    else:
        player.show_title()
    while player.running and (args.frames is None or player.frame_count < args.frames):
        player.frame()
        if args.autoplay and player.frame_count % 10 == 0:
            if player.waiting_for == "choice":
                player.choose(0)
            elif player.waiting_for == "input" or player.finished:
                player.advance()
    if args.screenshot:
        import pygame

        pygame.image.save(player.canvas, args.screenshot)
    player.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
