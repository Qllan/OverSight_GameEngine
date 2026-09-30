"""Export a project as a standalone game (PyInstaller).

    python -m vn_engine.export path/to/project.json --name MyGame [--onefile]

The project is validated first. Its scenes are inlined into one
project.json and its asset folder is copied, then PyInstaller bundles
the player, the Rust extension, pygame and the game data. The result is
dist/<name>/<name>[.exe] (a folder to ship) or, with --onefile, a single
executable. Players' saves go to their user data folder, not the game
folder. Build on each target OS; PyInstaller does not cross-compile.
"""

import argparse
import json
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Any, Dict, List, Optional

LAUNCHER = '''\
import sys
from pathlib import Path

from vn_engine.player.__main__ import main

# Bundled data lives next to the code (PyInstaller sets _MEIPASS)
base = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent))
sys.exit(main([str(base / "game" / "project.json"), *sys.argv[1:]]))
'''


class ExportError(Exception):
    pass


def stage_project(project_path: Path, destination: Path) -> Path:
    """Copy a project into `destination` as one project.json (scenes
    inlined) plus its asset folder. Returns the new project.json."""
    project_path = project_path.resolve()
    base = project_path.parent
    with open(project_path, "r", encoding="utf-8") as f:
        data: Dict[str, Any] = json.load(f)

    scenes: List[Any] = []
    for scene in data.get("scenes", []):
        if isinstance(scene, str):
            with open(base / scene, "r", encoding="utf-8") as f:
                scene = json.load(f)
        scenes.append(scene)
    staged: Dict[str, Any] = {**data, "scenes": scenes}

    destination.mkdir(parents=True, exist_ok=True)
    assets = data.get("assets")
    if isinstance(assets, str) and (base / assets).is_dir():
        shutil.copytree(base / assets, destination / "assets", dirs_exist_ok=True)
        staged["assets"] = "assets"

    target = destination / "project.json"
    with open(target, "w", encoding="utf-8") as f:
        json.dump(staged, f, ensure_ascii=False)
    return target


def validate(project_path: Path) -> None:
    """Compile the project the way the player will; raise ExportError on problems"""
    from vn_engine.engine import Engine

    engine = Engine()
    engine.init()
    try:
        engine.load_project(str(project_path))
    except (ValueError, RuntimeError, OSError) as e:
        details = "\n".join(engine.get_last_errors()) or str(e)
        raise ExportError(f"The project has problems:\n{details}") from e


def build(
    project_path: str,
    output: str = "dist",
    name: Optional[str] = None,
    onefile: bool = False,
) -> Path:
    """Build the game; returns the path of the executable"""
    try:
        import PyInstaller.__main__ as pyinstaller
    except ImportError as e:
        raise ExportError("PyInstaller is not installed: pip install pyinstaller") from e

    source = Path(project_path)
    validate(source)
    with open(source, "r", encoding="utf-8") as f:
        project_id = str(json.load(f).get("project_id", "game"))
    name = name or project_id
    output_dir = Path(output).resolve()

    with tempfile.TemporaryDirectory(prefix="vn_export_") as tmp:
        work = Path(tmp)
        stage_project(source, work / "game")
        launcher = work / f"{name}.py"
        launcher.write_text(LAUNCHER, encoding="utf-8")
        separator = ";" if sys.platform == "win32" else ":"
        args = [
            str(launcher),
            "--name", name,
            "--noconfirm",
            "--clean",
            "--windowed",
            "--onefile" if onefile else "--onedir",
            "--distpath", str(output_dir),
            "--workpath", str(work / "build"),
            "--specpath", str(work),
            "--add-data", f"{work / 'game'}{separator}game",
            "--hidden-import", "vn_engine._core",
            "--log-level", "WARN",
        ]
        pyinstaller.run(args)

    exe_name = name + (".exe" if sys.platform == "win32" else "")
    executable = output_dir / exe_name if onefile else output_dir / name / exe_name
    if sys.platform == "darwin" and not executable.exists():
        executable = output_dir / f"{name}.app"
    if not executable.exists():
        raise ExportError(f"PyInstaller finished but {executable} is missing")
    return executable


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(prog="vn_engine.export", description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("project", help="path to project.json")
    parser.add_argument("--name", help="name of the game executable (default: project_id)")
    parser.add_argument("--output", default="dist", help="output folder (default: dist)")
    parser.add_argument("--onefile", action="store_true", help="build a single executable")
    args = parser.parse_args(argv)
    try:
        executable = build(args.project, args.output, args.name, args.onefile)
    except ExportError as e:
        print(e, file=sys.stderr)
        return 1
    print(f"Built {executable}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
