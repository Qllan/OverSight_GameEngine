import pytest

from vn_engine import GraphCompiler


def test_valid_graph_has_no_errors(valid_graph):
    assert GraphCompiler().validate(valid_graph) == []


def test_parse_reports_structure(valid_graph):
    info = GraphCompiler().parse(valid_graph)
    assert info["graph_id"] == "intro_scene"
    assert info["node_count"] == 5
    assert info["entry_nodes"] == ["start"]
    assert info["is_dag"] is True


def test_compile_resolves_all_jumps(valid_graph):
    result = GraphCompiler().compile(valid_graph)
    assert result["opcode_count"] == len(result["opcodes"])
    for op in result["opcodes"]:
        if op.startswith(("JUMP(", "IF_JUMP(")):
            target = int(op[op.index("(") + 1 : -1])
            assert 0 < target < result["opcode_count"]
    assert "EQ" in result["opcodes"]


def test_compile_is_deterministic(valid_graph):
    first = GraphCompiler().compile(valid_graph)["disassembly"]
    for _ in range(5):
        assert GraphCompiler().compile(valid_graph)["disassembly"] == first


def test_broken_graph_reports_cycle_and_mismatch(broken_graph):
    errors = GraphCompiler().validate_detailed(broken_graph)
    types = {e["error_type"] for e in errors}
    assert types == {"CYCLE_DETECTED", "BRANCH_MISMATCH"}
    mismatch = next(e for e in errors if e["error_type"] == "BRANCH_MISMATCH")
    assert mismatch["node_id"] == "choice"
    assert mismatch["hint"]


def test_compile_rejects_broken_graph(broken_graph):
    compiler = GraphCompiler()
    with pytest.raises(ValueError, match="Validation failed"):
        compiler.compile(broken_graph)
    assert compiler.has_errors()


def test_errors_clear_after_successful_compile(broken_graph, valid_graph):
    compiler = GraphCompiler()
    with pytest.raises(ValueError):
        compiler.compile(broken_graph)
    compiler.compile(valid_graph)
    assert not compiler.has_errors()


def test_edge_to_missing_node_is_a_parse_error(valid_graph):
    valid_graph["edges"].append({"from": "start", "to": "nowhere"})
    with pytest.raises(ValueError, match="non-existent node"):
        GraphCompiler().parse(valid_graph)
