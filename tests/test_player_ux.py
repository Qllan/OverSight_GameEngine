import os
from pathlib import Path

import pytest

os.environ.setdefault("SDL_VIDEODRIVER", "dummy")
os.environ.setdefault("SDL_AUDIODRIVER", "dummy")
pygame = pytest.importorskip("pygame")

from vn_engine.player.app import Player  # noqa: E402
from vn_engine.player.saves import AUTO, QUICK, SaveManager  # noqa: E402

DEMO = Path(__file__).parents[1] / "examples" / "demo" / "project.json"
GREETING = "Oh, hi Alex! Isn't the weather lovely today?"


@pytest.fixture
def player(tmp_path):
    p = Player(str(DEMO), save_root=tmp_path)
    p.start()
    p.draw()
    yield p
    p.close()


def to_choice(p):
    p.advance()  # complete the greeting
    p.advance()  # continue -> choice
    assert p.waiting_for == "choice"
    p.draw()


def click_button(p, label):
    p.draw()
    labels = [lbl for lbl, _ in p._menu_buttons()] if p.screen in ("menu", "title") else []
    rects = [r for r, _ in p._buttons]
    p.click(rects[labels.index(label)].center)


def test_quick_save_and_load(player, tmp_path):
    to_choice(player)
    assert player.save_slot(QUICK)
    assert (tmp_path / "demo_park" / "quick.png").exists()
    player.choose(0)
    player.advance()
    player.advance()
    assert player.stage.background == "bg/cafe"

    player.handle_key(pygame.K_F9)
    assert player.stage.background == "bg/park"
    assert player.waiting_for == "choice"
    assert player.stage.choices == ["Want to grab a cocoa?", "Sorry, I'm in a hurry."]


def test_save_and_load_through_the_menu(player):
    player.advance()
    player.handle_key(pygame.K_ESCAPE)
    assert player.screen == "menu"
    click_button(player, "Save")
    assert player.screen == "save"
    player.draw()
    player.click(player.slot_rects()[2].center)
    assert player.screen is None and player.saves.exists("3")
    assert player.saves.meta("3")["text"] == GREETING

    to_choice_after_greeting = player
    to_choice_after_greeting.advance()
    assert player.waiting_for == "choice"
    player.handle_key(pygame.K_ESCAPE)
    click_button(player, "Load")
    player.draw()
    player.click(player.slot_rects()[2].center)
    assert player.screen is None
    assert player.waiting_for == "input" and player.stage.text == GREETING


def test_autosave_before_choices_and_on_quit(player):
    to_choice(player)
    assert player.saves.exists(AUTO)
    player.choose(0)
    player.quit()
    assert not player.running
    assert player.saves.meta(AUTO)["text"] == "I'd love to! I know a cozy place nearby."


def test_skip_only_skips_read_text(player):
    # First playthrough: take the second branch to the end
    to_choice(player)
    player.choose(1)
    player.advance()
    player.advance()
    assert player.finished

    player.start()
    player.skip = True
    for _ in range(10):
        player.update(1 / 60)
    assert player.waiting_for == "choice", "read lines are skipped up to the choice"
    player.choose(0)
    for _ in range(10):
        player.update(1 / 60)
    assert player.waiting_for == "input"
    assert player.stage.text == "I'd love to! I know a cozy place nearby.", "stops at unread text"


def test_auto_advance(player):
    player.auto = True
    for _ in range(40):
        player.update(0.25)
        if player.waiting_for == "choice":
            break
    assert player.waiting_for == "choice"


def test_rollback_with_the_mouse_wheel(player):
    to_choice(player)
    player.choose(0)
    assert player.stage.text == "I'd love to! I know a cozy place nearby."
    player.wheel(1)
    assert player.waiting_for == "choice"
    player.wheel(1)
    assert player.stage.text == GREETING


def test_backlog(player):
    to_choice(player)
    player.choose(0)
    player.handle_key(pygame.K_l)
    assert player.screen == "backlog"
    lines = [text for text, _ in player.backlog_lines()]
    assert lines[:2] == ["Mia", GREETING]
    assert "> Want to grab a cocoa?" in lines
    player.draw()
    player.handle_key(pygame.K_ESCAPE)
    assert player.screen is None


def test_title_screen(tmp_path):
    p = Player(str(DEMO), save_root=tmp_path)
    p.show_title()
    p.draw()
    assert [label for label, _ in p._menu_buttons()] == ["New Game", "Load", "Quit"]
    click_button(p, "New Game")
    assert p.started and p.screen is None and p.waiting_for == "input"
    to_choice(p)  # autosaves
    p.show_title()
    assert "Continue" in [label for label, _ in p._menu_buttons()]
    click_button(p, "Continue")
    assert p.waiting_for == "choice"
    p.close()
    assert (tmp_path / "demo_park" / "persistent.json").exists()


def test_save_manager(tmp_path):
    saves = SaveManager("g", tmp_path)
    assert [s["meta"] for s in saves.slots()] == [None] * 9
    saves.save("2", {"format": 2}, {"scene": "x"})
    assert saves.load("2") == {"format": 2}
    assert saves.meta("2")["scene"] == "x" and "saved_at" in saves.meta("2")
    saves.delete("2")
    assert not saves.exists("2")
