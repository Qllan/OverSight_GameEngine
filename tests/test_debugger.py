from pathlib import Path

from vn_engine.editor.debugger import DebugSession
from vn_engine.editor.model import NodeEditor, ProjectEditor

EXAMPLES = Path(__file__).parents[1] / "examples"


def affection_graph() -> NodeEditor:
    ed = NodeEditor()
    ed.load_from_file(str(EXAMPLES / "affection.json"))
    return ed


def test_runtime_errors_end_the_session_with_a_message():
    session = DebugSession(affection_graph())
    session.start()
    # {player} is never set in this graph
    assert session.error == "VAR_GET: Undefined variable 'player'"
    assert session.finished
    assert session.transcript[-1] == "!! VAR_GET: Undefined variable 'player'"


def test_breakpoints_and_stepping():
    project = ProjectEditor()
    project.load(str(EXAMPLES / "project" / "project.json"))
    session = DebugSession(project.scenes["town"], project, breakpoints={("shop", "buy")})
    session.start()
    assert session.transcript == ["== town ==", "Narrator: Market day! You have 10 gold."]
    assert session.waiting_for == "input"

    session.next()
    assert session.waiting_for == "choice"
    assert session.choices == ["Buy bread (4 gold)", "Just looking"]
    session.choose(0)
    assert session.paused and session.location == ("shop", "buy")
    assert session.inspect()["variables"]["gold"] == 10
    assert session.inspect()["call_stack"] == ["town"]

    session.step_node()
    assert session.paused and session.location == ("shop", "thanks")
    assert session.inspect()["variables"]["gold"] == 6

    session.continue_()
    assert session.waiting_for == "input"
    assert session.transcript[-1] == "Baker: Thanks for buying!"

    assert not session.toggle_breakpoint("shop", "buy")
    for _ in range(5):
        if session.finished:
            break
        session.next()
    assert session.finished
    assert session.transcript[-2:] == ["Narrator: The sun sets. Fin.", "-- end --"]


def test_compile_errors_are_reported_not_raised():
    ed = NodeEditor()
    ed.create_node("ShowText")
    ed.create_node("ShowText")  # two starts: AMBIGUOUS_ENTRY
    session = DebugSession(ed)
    session.start()
    assert session.error is not None and "Validation failed" in session.error
    assert not session.started
