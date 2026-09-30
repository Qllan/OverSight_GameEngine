import json
import os
import subprocess
from pathlib import Path

import pytest

from vn_engine.export import ExportError, build, stage_project, validate

EXAMPLES = Path(__file__).parents[1] / "examples"


def test_stage_inlines_scenes_and_copies_assets(tmp_path):
    staged = stage_project(EXAMPLES / "demo" / "project.json", tmp_path / "game")
    data = json.loads(staged.read_text())
    assert [s["graph_id"] for s in data["scenes"]] == ["park", "cafe"]
    assert data["assets"] == "assets"
    assert (tmp_path / "game" / "assets" / "bg" / "park.png").exists()
    validate(staged)  # the staged copy is a valid project on its own


def test_invalid_projects_are_not_exported(tmp_path):
    project = tmp_path / "project.json"
    project.write_text(json.dumps({
        "project_id": "broken", "start": "a",
        "scenes": [{"graph_id": "a", "version": "1.0",
                    "nodes": [{"id": "s", "type": "entry"},
                              {"id": "t", "type": "ShowTxt", "parameters": {"text": "hi"}}],
                    "edges": [{"from": "s", "to": "t"}]}],
    }))
    with pytest.raises(ExportError, match="UNKNOWN_BLOCK"):
        build(str(project), str(tmp_path / "dist"))


@pytest.mark.skipif(not os.environ.get("VN_ENGINE_TEST_EXPORT"),
                    reason="slow: set VN_ENGINE_TEST_EXPORT=1 to build with PyInstaller")
def test_exported_game_runs(tmp_path):
    executable = build(str(EXAMPLES / "demo" / "project.json"), str(tmp_path / "dist"), "ParkDemo")
    env = {**os.environ, "SDL_VIDEODRIVER": "dummy", "SDL_AUDIODRIVER": "dummy"}
    env.pop("PYTHONPATH", None)
    shot = tmp_path / "last.png"
    result = subprocess.run(
        [str(executable), "--autoplay", "--frames", "300", "--save-dir", str(tmp_path / "saves"),
         "--screenshot", str(shot)],
        env=env, cwd=tmp_path, capture_output=True, text=True, timeout=120,
    )
    assert result.returncode == 0, result.stderr
    assert shot.exists()
    assert (tmp_path / "saves" / "demo_park" / "persistent.json").exists()
