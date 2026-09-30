import pytest

from vn_engine import Engine
from vn_engine.editor.main import NodeEditor, parse_param


def build_scene():
    ed = NodeEditor()
    ed.graph_id = "editor_scene"
    start = ed.create_node("entry")
    line = ed.create_node("ShowText")
    ed.set_node_param(line, "text", "Hello")
    choice = ed.create_node("BranchChoice")
    left = ed.create_node("SetVariable")
    ed.set_node_param(left, "name", "route")
    ed.set_node_param(left, "value", "left")
    end = ed.create_node("EndScene")
    ed.add_edge(start, line)
    ed.add_edge(line, choice)
    ed.add_edge(choice, left, 0)
    ed.add_edge(choice, end, 1)
    ed.add_edge(left, end)
    return ed


def test_exported_graph_validates_and_runs():
    ed = build_scene()
    assert ed.validate_graph() == []

    engine = Engine()
    engine.init()
    engine.load_graph_from_dict(ed.export_json())
    engine.run()
    state = engine.run_until_yield()
    assert state["last_block_args"]["text"] == "Hello"
    engine.resume()
    engine.run_until_yield()
    engine.resume(0)
    assert engine.run_until_yield() is None
    assert engine.get_variable("route") == "left"


def test_port_holds_one_edge_and_shrinking_choices_drops_edges():
    ed = build_scene()
    choice = next(i for i, n in ed.nodes.items() if n["type"] == "BranchChoice")
    end = next(i for i, n in ed.nodes.items() if n["type"] == "EndScene")
    ed.add_edge(choice, end, 0)  # replaces the edge on port 0
    assert [e["to"] for e in ed.edges if e["from"] == choice and e["port"] == 0] == [end]
    ed.set_node_param(choice, "choices", ["only"])
    assert all(e["port"] == 0 for e in ed.edges if e["from"] == choice)
    assert not ed.add_edge(choice, end, 1)  # port 1 no longer exists


def test_round_trip_through_file(tmp_path):
    ed = build_scene()
    path = tmp_path / "scene.json"
    ed.save_to_file(str(path))
    loaded = NodeEditor()
    loaded.load_from_file(str(path))
    assert loaded.export_json() == ed.export_json()


def test_validation_reports_engine_errors():
    ed = NodeEditor()
    ed.create_node("ShowText")
    ed.create_node("ShowText")
    assert any("AMBIGUOUS_ENTRY" in e for e in ed.validate_graph())


def test_parse_param():
    assert parse_param("1.5") == 1.5
    assert parse_param("true") is True
    assert parse_param("hello") == "hello"
    assert parse_param('["a", "b"]') == ["a", "b"]


def test_if_node_has_true_and_false_outputs():
    ed = NodeEditor()
    start = ed.create_node("entry")
    check = ed.create_node("If")
    ed.set_node_param(check, "condition", "score > 1")
    yes = ed.create_node("EndScene")
    no = ed.create_node("EndScene")
    ed.add_edge(start, check)
    assert ed.add_edge(check, yes, 0) and ed.add_edge(check, no, 1)
    assert not ed.add_edge(check, no, 2)
    assert ed.output_labels(ed.nodes[check]) == ["0: true", "1: false"]
    assert ed.validate_graph() == []


def test_scene_nodes_ports():
    ed = NodeEditor()
    assert ed.output_count(ed.nodes[ed.create_node("JumpScene")]) == 0
    assert ed.output_count(ed.nodes[ed.create_node("CallScene")]) == 1
    assert ed.nodes[ed.create_node("Wait")]["parameters"] == {"frames": 60}


# --- registry-driven catalog and projects ----------------------------------

from pathlib import Path  # noqa: E402

from vn_engine import _core  # noqa: E402
from vn_engine.editor.model import ProjectEditor, build_catalog  # noqa: E402

EXAMPLES = Path(__file__).parents[1] / "examples"


def test_catalog_follows_the_registry():
    registry = _core.PluginRegistry(with_defaults=True)
    registry.register_plugin(_core.PluginInfo("fx", "1", ""))
    registry.register_block("fx", _core.BlockDefinition(
        "Shake", "Shake Screen", [], [],
        [_core.ParameterDefinition("strength", "int", 3),
         _core.ParameterDefinition("label", "text")]))
    ed = NodeEditor(registry)
    assert "Shake" in ed.node_types()
    spec = build_catalog(registry)["Shake"]
    assert (spec.display_name, spec.category, spec.outputs) == ("Shake Screen", "fx", [])
    node = ed.nodes[ed.create_node("Shake")]
    assert node["parameters"] == {"strength": 3, "label": ""}
    assert ed.output_count(node) == 0


def test_errors_point_at_nodes():
    ed = NodeEditor()
    start = ed.create_node("entry")
    line = ed.create_node("ShowText")
    ed.set_node_param(line, "speaker", 5)
    ed.add_edge(start, line)
    errors = ed.validate_detailed()
    assert [(e["error_type"], e["node_id"]) for e in errors] == [("INVALID_PARAMETER", line)]


def test_project_round_trip_and_cross_scene_errors(tmp_path):
    project = ProjectEditor()
    project.load(str(EXAMPLES / "project" / "project.json"))
    assert project.scene_ids() == ["town", "shop", "ending"]
    assert project.validate() == []

    # Break a scene link, add a scene, save elsewhere and reload
    town = project.scenes["town"]
    jump = next(n for n in town.nodes.values() if n["type"] == "JumpScene")
    town.set_node_param(jump["id"], "scene", "nowhere")
    errors = project.validate()
    assert [(e["error_type"], e["graph_id"], e["node_id"]) for e in errors] == [
        ("UNKNOWN_SCENE", "town", jump["id"])
    ]
    project.add_scene("epilogue")
    project.path = tmp_path / "game" / "project.json"
    project.save()
    assert (tmp_path / "game" / "scenes" / "epilogue.json").exists()

    reloaded = ProjectEditor()
    reloaded.load(str(project.path))
    assert reloaded.scene_ids() == ["town", "shop", "ending", "epilogue"]
    assert reloaded.scenes["town"].nodes[jump["id"]]["parameters"]["scene"] == "nowhere"


def test_new_project_is_valid_and_runs(tmp_path):
    project = ProjectEditor()
    project.new(str(tmp_path / "project.json"), "demo")
    intro = project.scenes["intro"]
    line = intro.create_node("ShowText")
    intro.set_node_param(line, "text", "Hello")
    intro.add_edge("start", line)
    assert project.validate() == []
    project.save()

    engine = Engine()
    engine.init()
    engine.load_project(str(tmp_path / "project.json"))
    engine.run()
    assert engine.run_until_yield()["last_block_args"]["text"] == "Hello"


def test_project_assets_are_scanned(tmp_path):
    (tmp_path / "assets" / "voice").mkdir(parents=True)
    (tmp_path / "assets" / "voice" / "hi.ogg").write_bytes(b"")
    project = ProjectEditor()
    project.new(str(tmp_path / "project.json"), "demo")
    project.data["assets"] = "assets"
    project.scan_assets()
    assert project.asset_ids("audio") == ["voice/hi"]
    intro = project.scenes["intro"]
    voice = intro.create_node("PlayVoice")
    intro.set_node_param(voice, "voice_id", "voice/bye")
    intro.add_edge("start", voice)
    assert [e["error_type"] for e in project.validate()] == ["UNKNOWN_ASSET"]


def test_engine_scans_project_assets(tmp_path):
    (tmp_path / "assets").mkdir()
    (tmp_path / "assets" / "hi.ogg").write_bytes(b"")
    project = ProjectEditor()
    project.new(str(tmp_path / "project.json"), "demo")
    project.data["assets"] = "assets"
    intro = project.scenes["intro"]
    voice = intro.create_node("PlayVoice")
    intro.set_node_param(voice, "voice_id", "missing")
    intro.add_edge("start", voice)
    project.save()
    engine = Engine()
    engine.init()
    with pytest.raises(ValueError, match="UNKNOWN_ASSET"):
        engine.load_project(str(tmp_path / "project.json"))
    assert engine.asset_path("hi") is not None
