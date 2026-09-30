from pathlib import Path

import pytest

from vn_engine import Engine, VMExecutor, _core


@pytest.fixture
def engine(valid_graph_path):
    e = Engine()
    e.init()
    e.register_plugin("novel_plugin")
    e.load_graph(valid_graph_path)
    e.run()
    return e


def play(engine, choices):
    """Run the scene, answering choice yields from `choices`.
    Returns the texts shown and the final variables."""
    shown = []
    state = engine.run_until_yield()
    while state is not None:
        if state["yield_reason"] == "choice":
            engine.resume(choices.pop(0))
        else:
            shown.append(state["last_block_args"]["text"])
            engine.resume()
        state = engine.run_until_yield()
    return shown


def test_first_choice_plays_adventure(engine):
    assert play(engine, [0]) == ["Welcome to the Visual Novel!", "Your adventure begins..."]
    assert not engine.is_running
    assert engine.get_variable("__branch_result") == 0


def test_second_choice_ends_scene(engine):
    assert play(engine, [1]) == ["Welcome to the Visual Novel!"]
    assert engine.get_variable("__branch_result") == 1


def test_show_text_arguments_are_not_swapped(engine):
    state = engine.run_until_yield()
    assert state["last_block_type"] == "ShowText"
    assert state["last_block_args"] == {
        "text": "Welcome to the Visual Novel!",
        "speaker": "Narrator",
    }


def test_choice_yield_exposes_choices_and_validates(engine):
    engine.run_until_yield()
    engine.resume()
    state = engine.run_until_yield()
    assert state["yield_reason"] == "choice"
    assert state["last_block_args"] == {"choices": ["Start Adventure", "Exit"]}
    with pytest.raises(ValueError):
        engine.resume(5)
    with pytest.raises(ValueError):
        engine.resume()
    assert engine.is_yielding


def test_execution_is_deterministic(valid_graph_path):
    def run_once():
        e = Engine()
        e.init()
        e.load_graph(valid_graph_path)
        e.run()
        play(e, [0])
        return e.get_log()

    first = run_once()
    for _ in range(3):
        assert run_once() == first


def test_events_reach_subscribers(engine):
    events = []
    engine.subscribe("*", lambda kind, payload: events.append((kind, payload)))
    engine.run()  # restart so GraphStart is observed
    play(engine, [1])
    kinds = [k for k, _ in events]
    assert kinds[0] == "GraphStart"
    assert kinds[-1] == "GraphEnd"
    assert ("BlockCall", {
        "node_id": "show_title",
        "block_type": "ShowText",
        "args": {"text": "Welcome to the Visual Novel!", "speaker": "Narrator"},
    }) in events
    assert ("VariableChanged", {"name": "__branch_result", "value": 1}) in events
    assert any(k == "NodeEnter" and p == {"node_id": "exit_path", "node_type": "EndScene"}
               for k, p in events)


def test_callback_may_resume_the_vm(engine):
    # Auto-advance text from a callback: the VM must not be borrowed during events
    engine.subscribe("YieldRequest",
                     lambda p: engine.resume() if p["reason"] == "input" else None)
    state = engine.run_until_yield()
    # The VM yielded for input and the callback already resumed it
    assert state["is_running"] and not state["is_yielding"]
    assert state["last_block_type"] == "ShowText"
    state = engine.run_until_yield()
    assert state["yield_reason"] == "choice"


def test_runtime_error_stops_vm():
    vm = VMExecutor()
    vm.load({
        "graph_id": "g", "version": "1.0",
        "nodes": [{"id": "s", "type": "entry"},
                  {"id": "v", "type": "SetVariable", "parameters": {"name": "x", "value": 1}}],
        "edges": [{"from": "s", "to": "v"}],
    })
    vm.set_variable("preset", "kept")
    vm.start()
    assert vm.run_until_yield() is None
    assert vm.get_variable("x") == 1
    assert vm.get_variable("preset") == "kept"


def test_event_bus_is_shared_not_copied():
    bus = _core.EventBus()
    seen = []
    vm = _core.VMExecutor()
    vm.set_event_bus(bus)
    bus.subscribe("GraphStart", seen.append)  # subscribe after attaching
    vm.load(_core.GraphCompiler().compile_json(
        '{"graph_id":"g","version":"1.0","nodes":[{"id":"e","type":"End"}],"edges":[]}'))
    vm.start()
    assert seen == ['{"graph_id":"g"}']


def test_plugin_unregister_removes_its_blocks():
    registry = _core.PluginRegistry()
    _core.NovelPlugin().register_blocks(registry)
    assert registry.has_block("ShowText")
    assert registry.get_block_owner("ShowText") == "novel_plugin"
    registry.unregister_plugin("novel_plugin")
    assert registry.get_all_blocks() == []
    _core.NovelPlugin().register_blocks(registry)  # re-registering works
    assert registry.has_block("ShowText")


def test_tick_loop_counts_frames():
    loop = _core.FixedTickLoop()
    assert loop.tick() is False
    loop.start()
    for _ in range(3):
        assert loop.tick(False) is True
    assert loop.get_frame_count() == 3
    assert loop.fixed_delta() == pytest.approx(1 / 60)


def test_save_and_load_game(engine, valid_graph_path, tmp_path):
    engine.set_variable("affection", 5)
    engine.run_until_yield()
    engine.resume()
    assert engine.run_until_yield()["yield_reason"] == "choice"
    save = tmp_path / "slot1.json"
    engine.save_game(str(save))

    # A fresh engine picks up at the choice
    other = Engine()
    other.init()
    other.load_graph(valid_graph_path)
    other.load_game(str(save))
    assert other.is_yielding
    assert other.get_variable("affection") == 5
    assert other.get_state()["last_block_args"] == {"choices": ["Start Adventure", "Exit"]}
    assert play(other, [0]) == ["Your adventure begins..."]


def test_save_from_other_graph_is_rejected(engine):
    engine.run_until_yield()
    state = engine.save_state()
    other = Engine()
    other.init()
    other.load_graph(str(Path(__file__).parents[1] / "examples" / "menu_loop.json"))
    with pytest.raises(ValueError, match="is for graph"):
        other.load_state(state)
    state["fingerprint"] = "0" * 16
    with pytest.raises(ValueError, match="different version"):
        engine.load_state(state)


def test_expressions_and_interpolation():
    e = Engine()
    e.init()
    e.load_graph(str(Path(__file__).parents[1] / "examples" / "affection.json"))
    e.set_variable("player", "Alex")
    e.set_variable("speaker_name", "Mia")
    e.run()
    shown = []
    state = e.run_until_yield()
    while state is not None:
        args = state["last_block_args"]
        if state["yield_reason"] == "choice":
            shown.append(args["choices"])
            e.resume(0)
        else:
            shown.append(args["text"])
            e.resume()
        state = e.run_until_yield()
    assert shown == [
        "Hi, Alex! It's been a while.",
        ["Missed you, Mia", "Whatever."],
        "She smiles. (affection: 3)",
    ]
