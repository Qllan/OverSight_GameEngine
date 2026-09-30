import json
from pathlib import Path

import pytest

FIXTURES = Path(__file__).parent / "fixtures"


@pytest.fixture
def valid_graph():
    return json.loads((FIXTURES / "valid_graph.json").read_text())


@pytest.fixture
def broken_graph():
    return json.loads((FIXTURES / "broken_graph.json").read_text())


@pytest.fixture
def valid_graph_path():
    return str(FIXTURES / "valid_graph.json")
