"""Keep python/vn_engine/_core.pyi in sync with the compiled extension."""

import ast
from pathlib import Path

from vn_engine import _core

STUB = Path(__file__).parents[1] / "python" / "vn_engine" / "_core.pyi"


def stub_classes():
    tree = ast.parse(STUB.read_text())
    classes = {}
    for node in tree.body:
        if isinstance(node, ast.ClassDef):
            members = set()
            for item in node.body:
                if isinstance(item, ast.FunctionDef) and item.name != "__init__":
                    members.add(item.name)
                elif isinstance(item, ast.AnnAssign) and isinstance(item.target, ast.Name):
                    members.add(item.target.id)
            classes[node.name] = members
    return classes


def runtime_classes():
    return {
        name: {m for m in dir(obj) if not m.startswith("_")}
        for name, obj in vars(_core).items()
        if isinstance(obj, type)
    }


def test_stub_matches_extension():
    stub, runtime = stub_classes(), runtime_classes()
    assert set(stub) == set(runtime)
    for name in runtime:
        assert stub[name] == runtime[name], name
