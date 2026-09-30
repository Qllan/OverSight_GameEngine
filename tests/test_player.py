import os
from pathlib import Path

import pytest

os.environ.setdefault("SDL_VIDEODRIVER", "dummy")
os.environ.setdefault("SDL_AUDIODRIVER", "dummy")
pygame = pytest.importorskip("pygame")

from vn_engine.player import Stage  # noqa: E402
from vn_engine.player.app import Player, wrap_text  # noqa: E402

DEMO = Path(__file__).parents[1] / "examples" / "demo" / "project.json"


@pytest.fixture
def player(tmp_path):
    p = Player(str(DEMO), save_root=tmp_path)
    p.start()
    yield p
    p.close()


def run_until(player, predicate, frames=600):
    for _ in range(frames):
        if predicate():
            return
        player.update(1 / 60)
        player.draw()
    raise AssertionError("condition not reached")


def test_opening_scene_is_on_stage(player):
    assert player.waiting_for == "input"
    assert player.stage.background == "bg/park"
    assert player.stage.characters == {
        "Mia": {"image": "characters/mia_happy", "position": "center"}
    }
    assert player.stage.music["id"] == "audio/theme"
    assert player.stage.text == "Oh, hi Alex! Isn't the weather lovely today?"
    # Text appears letter by letter; the first click completes it
    assert not player.text_complete
    player.advance()
    assert player.text_complete and player.waiting_for == "input"
    player.draw()
    # The sky is drawn from the background image, not a placeholder
    sky = player.canvas.get_at((20, 20))[:3]
    assert all(abs(a - b) <= 6 for a, b in zip(sky, (130, 190, 240))), sky


def test_choice_leads_to_the_cafe(player):
    player.advance()
    player.advance()
    assert player.waiting_for == "choice"
    assert player.stage.choices == ["Want to grab a cocoa?", "Sorry, I'm in a hurry."]
    player.draw()
    # Click the first choice button (in canvas coordinates)
    player.click(player.choice_rects()[0].center)
    assert player.stage.text == "I'd love to! I know a cozy place nearby."
    player.advance()
    player.advance()
    assert player.state["current_graph_id"] == "cafe"
    assert player.stage.background == "bg/cafe"
    assert player.stage.characters["Mia"]["position"] == "left"


def test_keyboard_choice_and_ending(player):
    player.advance()
    player.advance()
    player.handle_key(pygame.K_DOWN)
    player.handle_key(pygame.K_RETURN)
    assert player.stage.characters["Mia"]["image"] == "characters/mia_sad"
    player.advance()
    player.advance()
    assert player.finished
    player.draw()
    player.advance()
    assert not player.running


def test_stage_survives_save_and_rollback(player):
    player.advance()
    player.advance()
    player.choose(0)
    player.advance()
    player.advance()
    assert player.stage.background == "bg/cafe"
    save = player.engine.save_state()

    player.engine.rollback()
    player.restore_stage()
    assert player.stage.background == "bg/park"
    assert player.stage.text == "I'd love to! I know a cozy place nearby."

    player.engine.load_state(save)
    player.restore_stage()
    assert player.stage.background == "bg/cafe"
    assert player.stage.text == "They make the best cocoa in town here."


def test_window_scaling_maps_mouse_to_canvas(player):
    player.window = pygame.display.set_mode((640, 480))
    player.draw()
    player.present()
    x, y, scale = player._view
    assert scale == pytest.approx(0.5) and (x, y) == (0, 60)
    assert player.to_canvas((320, 240)) == pytest.approx((640, 360))


def test_stage_model_round_trip():
    stage = Stage()
    assert stage.apply("PlayMusic", {"music": "m", "volume": 0.5, "loop": False}) == (
        "music", {"id": "m", "volume": 0.5, "loop": False})
    stage.apply("ShowCharacter", {"character": "A", "image": "a", "position": "left"})
    stage.apply("ShowCharacter", {"character": "B", "image": "b"})
    stage.apply("ShowCharacter", {"character": "A", "image": "a2"})  # A comes to the front
    assert list(stage.characters) == ["B", "A"]
    assert Stage.from_json(stage.to_json()).characters == stage.characters
    assert stage.apply("StopMusic", {"fade_ms": 10}) == ("stop_music", {"fade_ms": 10})
    assert stage.music is None


def test_wrap_text():
    pygame.font.init()
    font = pygame.font.SysFont("dejavusans", 20)
    lines = wrap_text(font, "one two three four five six seven\nnew line", 120)
    assert all(font.size(line)[0] <= 120 for line in lines)
    assert "new line" in lines
    assert "".join(wrap_text(font, "x" * 80, 100)) == "x" * 80


def test_missing_assets_draw_placeholders(tmp_path):
    project = tmp_path / "project.json"
    scene = {"graph_id": "s", "version": "1.0",
             "nodes": [{"id": "e", "type": "entry"},
                       {"id": "bg", "type": "ShowBackground", "parameters": {"image": "bg/none"}},
                       {"id": "c", "type": "ShowCharacter",
                        "parameters": {"character": "Ghost", "image": "ghost"}},
                       {"id": "t", "type": "ShowText", "parameters": {"text": "Boo"}}],
             "edges": [{"from": "e", "to": "bg"}, {"from": "bg", "to": "c"}, {"from": "c", "to": "t"}]}
    import json
    project.write_text(json.dumps({"project_id": "p", "start": "s", "scenes": [scene]}))
    p = Player(str(project), save_root=tmp_path / "saves")
    p.start()
    p.draw()
    assert p.stage.text == "Boo" and p.image("ghost") is None
    p.close()
