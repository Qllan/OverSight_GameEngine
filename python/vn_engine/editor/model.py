"""Editor model: graphs and projects being edited, without GUI code.

The node catalog comes from the engine: the built-in control-flow nodes
plus every block in a PluginRegistry, so the palette, default parameters
and parameter widgets always match what the compiler accepts.
"""

import json
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

from vn_engine import _core


@dataclass
class ParamSpec:
    name: str
    param_type: str
    default: Any = None
    required: bool = False

    def empty_value(self) -> Any:
        """Value a new node starts with"""
        if not self.required:
            return self.default
        base = self.param_type.split(":")[0]
        return {"number": 0, "float": 0.0, "int": 0, "bool": False, "array": [], "object": {}}.get(
            base, ""
        )


@dataclass
class NodeSpec:
    node_type: str
    display_name: str
    category: str
    params: List[ParamSpec] = field(default_factory=list)
    #: Fixed output labels; None means one output per choice (BranchChoice)
    outputs: Optional[List[str]] = None
    has_input: bool = True

    def param(self, name: str) -> Optional[ParamSpec]:
        return next((p for p in self.params if p.name == name), None)


FLOW = "Flow"

#: Nodes built into the compiler (see src/core/plugin_registry.rs BUILTIN_NODES)
BUILTIN_SPECS: List[NodeSpec] = [
    NodeSpec("entry", "Entry", FLOW, [], ["next"], has_input=False),
    NodeSpec("BranchChoice", "Choice", FLOW,
             [ParamSpec("choices", "array", ["Option 1", "Option 2"])], None),
    NodeSpec("If", "If", FLOW, [ParamSpec("condition", "expression", "true")],
             ["0: true", "1: false"]),
    NodeSpec("SetVariable", "Set Variable", FLOW,
             [ParamSpec("name", "string", ""), ParamSpec("value", "any", ""),
              ParamSpec("expression", "expression", None)], ["next"]),
    NodeSpec("Wait", "Wait", FLOW, [ParamSpec("frames", "int", 60)], ["next"]),
    NodeSpec("CallScene", "Call Scene", FLOW, [ParamSpec("scene", "scene", "")], ["next"]),
    NodeSpec("JumpScene", "Jump to Scene", FLOW, [ParamSpec("scene", "scene", "")], []),
    NodeSpec("EndScene", "End Scene", FLOW, [], []),
    NodeSpec("End", "End", FLOW, [], []),
]


def build_catalog(registry: "_core.PluginRegistry") -> Dict[str, NodeSpec]:
    """Every node type the compiler accepts with this registry"""
    catalog = {spec.node_type: spec for spec in BUILTIN_SPECS}
    for block in registry.get_all_blocks():
        catalog[block.block_type] = NodeSpec(
            node_type=block.block_type,
            display_name=block.display_name,
            category=registry.get_block_owner(block.block_type) or "Blocks",
            params=[
                ParamSpec(p.name, p.param_type, p.default, p.is_required())
                for p in block.parameters
            ],
            outputs=["next"] if block.outputs else [],
        )
    return catalog


def parse_param(text: str) -> Any:
    """Interpret a typed parameter: JSON if it parses (numbers, true, lists), else a string"""
    try:
        return json.loads(text)
    except (ValueError, TypeError):
        return text


def format_param(value: Any) -> str:
    return value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)


def format_error(error: Dict[str, Any]) -> str:
    place = "/".join(p for p in (error.get("graph_id"), error.get("node_id")) if p)
    text = f"{error['error_type']}: {error['message']}"
    return f"{text} ({place})" if place else text


class NodeEditor:
    """One graph (scene) being edited."""

    def __init__(self, registry: Optional["_core.PluginRegistry"] = None) -> None:
        self.registry = registry or _core.PluginRegistry(with_defaults=True)
        self.catalog = build_catalog(self.registry)
        self.nodes: Dict[str, Dict[str, Any]] = {}
        self.edges: List[Dict[str, Any]] = []
        self.graph_id: str = "untitled"

    def spec(self, node_type: str) -> Optional[NodeSpec]:
        return self.catalog.get(node_type)

    def node_types(self) -> List[str]:
        """Palette order: flow nodes first, then blocks by name"""
        flow = [s.node_type for s in BUILTIN_SPECS if s.node_type != "End"]
        blocks = sorted(t for t, s in self.catalog.items() if s.category != FLOW)
        return flow + blocks

    def create_node(self, node_type: str, x: int = 100, y: int = 100) -> str:
        """Create a node with its default parameters"""
        node_id = f"{node_type.lower()}_{uuid.uuid4().hex[:6]}"
        self.nodes[node_id] = {
            "id": node_id,
            "type": node_type,
            "position": [x, y],
            "parameters": self._default_params_for_type(node_type),
        }
        return node_id

    def _default_params_for_type(self, node_type: str) -> Dict[str, Any]:
        spec = self.spec(node_type)
        if spec is None:
            return {}
        return {
            p.name: p.empty_value()
            for p in spec.params
            if p.required or p.default is not None
        }

    def output_labels(self, node: Dict[str, Any]) -> List[str]:
        """Labels of a node's output ports"""
        spec = self.spec(node["type"])
        if spec is None:
            return ["next"]
        if spec.outputs is None:
            return [f"{i}: {c}" for i, c in enumerate(node["parameters"].get("choices", []))]
        return list(spec.outputs)

    def output_count(self, node: Dict[str, Any]) -> int:
        """Number of output ports a node has"""
        return len(self.output_labels(node))

    def has_input(self, node: Dict[str, Any]) -> bool:
        spec = self.spec(node["type"])
        return spec.has_input if spec else True

    def delete_node(self, node_id: str) -> None:
        """Delete a node and its edges"""
        if node_id in self.nodes:
            del self.nodes[node_id]
            self.edges = [e for e in self.edges if e["from"] != node_id and e["to"] != node_id]

    def add_edge(self, from_id: str, to_id: str, port: int = 0) -> bool:
        """Connect an output port to a node. An output port holds one edge,
        so an existing edge from the same port is replaced."""
        if from_id not in self.nodes or to_id not in self.nodes or from_id == to_id:
            return False
        if not 0 <= port < self.output_count(self.nodes[from_id]):
            return False
        self.remove_edge(from_id, port)
        self.edges.append({"from": from_id, "to": to_id, "port": port})
        return True

    def remove_edge(self, from_id: str, port: int) -> None:
        """Remove the edge leaving a port"""
        self.edges = [
            e for e in self.edges if not (e["from"] == from_id and e.get("port", 0) == port)
        ]

    def set_node_param(self, node_id: str, param_name: str, value: Any) -> None:
        """Set a node parameter (None removes it). Shrinking a choice list
        drops edges from ports that no longer exist."""
        if node_id not in self.nodes:
            return
        node = self.nodes[node_id]
        if value is None:
            node["parameters"].pop(param_name, None)
        else:
            node["parameters"][param_name] = value
        count = self.output_count(node)
        self.edges = [e for e in self.edges if e["from"] != node_id or e.get("port", 0) < count]

    def set_node_position(self, node_id: str, x: int, y: int) -> None:
        if node_id in self.nodes:
            self.nodes[node_id]["position"] = [int(x), int(y)]

    def export_json(self) -> Dict[str, Any]:
        """Export graph to JSON format"""
        edges = sorted(self.edges, key=lambda e: (e["from"], e.get("port", 0)))
        return {
            "graph_id": self.graph_id,
            "version": "1.0",
            "nodes": list(self.nodes.values()),
            "edges": edges,
        }

    def import_json(self, data: Dict[str, Any]) -> None:
        """Import graph from JSON"""
        self.graph_id = data.get("graph_id", "imported")
        self.nodes = {
            n["id"]: {**n, "parameters": dict(n.get("parameters", {}))}
            for n in data.get("nodes", [])
        }
        self.edges = []
        next_port: Dict[str, int] = {}
        for edge in data.get("edges", []):
            # Edges without a port take the next free one, as the compiler does
            port = edge.get("port")
            if port is None:
                port = next_port.get(edge["from"], 0)
            next_port[edge["from"]] = port + 1
            self.edges.append({"from": edge["from"], "to": edge["to"], "port": port})

    def save_to_file(self, path: str) -> None:
        """Save graph to file"""
        with open(path, "w", encoding="utf-8") as f:
            json.dump(self.export_json(), f, indent=2, ensure_ascii=False)

    def load_from_file(self, path: str) -> None:
        """Load graph from file"""
        with open(path, "r", encoding="utf-8") as f:
            self.import_json(json.load(f))

    def validate_detailed(
        self, assets: Optional["_core.AssetManager"] = None
    ) -> List[Dict[str, Any]]:
        """Compiler errors, each with error_type, message, node_id, hint"""
        compiler = _core.GraphCompiler(self.registry)
        validator = _core.GraphValidator(compiler.registry, assets)
        try:
            ast = compiler.parse(json.dumps(self.export_json()))
        except ValueError as e:  # e.g. an edge to a missing node
            return [{"error_type": "PARSE_ERROR", "message": str(e), "node_id": None,
                     "hint": None, "graph_id": None}]
        return [
            {"error_type": e.error_type, "message": e.message, "node_id": e.node_id,
             "hint": e.hint, "graph_id": e.graph_id}
            for e in validator.validate(ast)
        ]

    def validate_graph(self) -> List[str]:
        """Validation errors as text"""
        return [format_error(e) for e in self.validate_detailed()]


def _new_scene_data(graph_id: str) -> Dict[str, Any]:
    return {
        "graph_id": graph_id,
        "version": "1.0",
        "nodes": [{"id": "start", "type": "entry", "position": [40, 120], "parameters": {}}],
        "edges": [],
    }


class ProjectEditor:
    """A project being edited: project.json plus one scene file per scene."""

    def __init__(self, registry: Optional["_core.PluginRegistry"] = None) -> None:
        self.registry = registry or _core.PluginRegistry(with_defaults=True)
        self.path: Optional[Path] = None
        self.data: Dict[str, Any] = {}
        self.scenes: Dict[str, NodeEditor] = {}
        #: graph_id → scene file, relative to the project directory
        self.scene_files: Dict[str, str] = {}
        self.assets: Optional["_core.AssetManager"] = None

    @property
    def base_dir(self) -> Path:
        return self.path.parent if self.path else Path.cwd()

    @property
    def project_id(self) -> str:
        return str(self.data.get("project_id", "untitled"))

    @property
    def start(self) -> str:
        return str(self.data.get("start", ""))

    def scene_ids(self) -> List[str]:
        return list(self.scenes)

    def new(self, path: str, project_id: str) -> None:
        """Start an empty project with one scene"""
        self.path = Path(path)
        self.data = {"project_id": project_id, "start": "", "variables": {}}
        self.scenes = {}
        self.scene_files = {}
        self.add_scene("intro")
        self.data["start"] = "intro"

    def load(self, path: str) -> None:
        self.path = Path(path)
        with open(self.path, "r", encoding="utf-8") as f:
            self.data = json.load(f)
        self.scenes = {}
        self.scene_files = {}
        for index, scene in enumerate(self.data.get("scenes", [])):
            editor = NodeEditor(self.registry)
            if isinstance(scene, str):
                editor.load_from_file(str(self.base_dir / scene))
                self.scene_files[editor.graph_id] = scene
            else:
                editor.import_json(scene)
                self.scene_files[editor.graph_id] = f"scenes/{editor.graph_id}.json"
            self.scenes[editor.graph_id] = editor
        self.scan_assets()

    def scan_assets(self) -> None:
        """Scan the project's "assets" folder, if it names one"""
        folder = self.data.get("assets")
        self.assets = None
        if folder and (self.base_dir / folder).is_dir():
            self.assets = _core.AssetManager()
            self.assets.set_base_path(str(self.base_dir / folder))
            self.assets.scan()

    def asset_ids(self, kind: str = "") -> List[str]:
        if self.assets is None:
            return []
        return [a.id for a in self.assets.get_all_assets() if not kind or a.kind == kind]

    def add_scene(self, graph_id: str) -> NodeEditor:
        if graph_id in self.scenes:
            raise ValueError(f"Scene '{graph_id}' already exists")
        editor = NodeEditor(self.registry)
        editor.import_json(_new_scene_data(graph_id))
        self.scenes[graph_id] = editor
        self.scene_files[graph_id] = f"scenes/{graph_id}.json"
        return editor

    def remove_scene(self, graph_id: str) -> None:
        self.scenes.pop(graph_id, None)
        self.scene_files.pop(graph_id, None)
        if self.start == graph_id:
            self.data["start"] = next(iter(self.scenes), "")

    def set_start(self, graph_id: str) -> None:
        if graph_id not in self.scenes:
            raise ValueError(f"Unknown scene '{graph_id}'")
        self.data["start"] = graph_id

    def export_json(self, inline: bool = False) -> Dict[str, Any]:
        """project.json content; inline=True embeds the scenes (for compiling)"""
        scenes: List[Any] = [
            s.export_json() if inline else self.scene_files[gid]
            for gid, s in self.scenes.items()
        ]
        return {**self.data, "scenes": scenes}

    def save(self) -> None:
        if self.path is None:
            raise ValueError("Project has no path")
        for graph_id, scene in self.scenes.items():
            target = self.base_dir / self.scene_files[graph_id]
            target.parent.mkdir(parents=True, exist_ok=True)
            scene.save_to_file(str(target))
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with open(self.path, "w", encoding="utf-8") as f:
            json.dump(self.export_json(), f, indent=2, ensure_ascii=False)

    def validate(self) -> List[Dict[str, Any]]:
        """Errors of every scene and of the links between scenes"""
        errors: List[Dict[str, Any]] = []
        for graph_id, scene in self.scenes.items():
            for error in scene.validate_detailed(self.assets):
                errors.append({**error, "graph_id": graph_id})
        errors.extend(self._project_errors())
        return errors

    def _project_errors(self) -> List[Dict[str, Any]]:
        """Errors that only exist at project level (start, scene links)"""
        errors: List[Dict[str, Any]] = []

        def error(kind: str, message: str, graph: Optional[str] = None,
                  node: Optional[str] = None) -> None:
            errors.append({"error_type": kind, "message": message, "node_id": node,
                           "hint": None, "graph_id": graph})

        if self.start not in self.scenes:
            error("UNKNOWN_SCENE", f"Start scene '{self.start}' is not in the project")
        for graph_id, scene in self.scenes.items():
            for node in scene.nodes.values():
                if node["type"] in ("CallScene", "JumpScene"):
                    target = node["parameters"].get("scene")
                    if target and target not in self.scenes:
                        error("UNKNOWN_SCENE", f"{node['type']} goes to unknown scene '{target}'",
                              graph_id, node["id"])
        return errors
