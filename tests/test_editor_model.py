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
