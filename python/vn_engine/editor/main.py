"""DearPyGui-based Visual Node Editor for VN Engine

Run with: python -m vn_engine.editor.main [graph.json]
"""

import json
import sys
import uuid
from typing import Any, Dict, List, Optional, Tuple

NODE_TYPES = ["entry", "ShowText", "BranchChoice", "If", "SetVariable", "PlayVoice", "EndScene"]


class NodeEditor:
    """Graph model behind the editor (no GUI dependencies)."""

    def __init__(self) -> None:
        self.nodes: Dict[str, Dict[str, Any]] = {}
        self.edges: List[Dict[str, Any]] = []
        self.graph_id: str = "untitled"

    def create_node(self, node_type: str, x: int = 100, y: int = 100) -> str:
        """Create a new node"""
        node_id = f"{node_type.lower()}_{uuid.uuid4().hex[:6]}"
        self.nodes[node_id] = {
            "id": node_id,
            "type": node_type,
            "position": [x, y],
            "parameters": self._default_params_for_type(node_type),
        }
        return node_id

    @staticmethod
    def _default_params_for_type(node_type: str) -> Dict[str, Any]:
        """Get default parameters for a node type"""
        defaults: Dict[str, Dict[str, Any]] = {
            "entry": {},
            "ShowText": {"text": "", "speaker": ""},
            "BranchChoice": {"choices": ["Option 1", "Option 2"]},
            "If": {"condition": "true"},
            "SetVariable": {"name": "", "value": ""},
            "PlayVoice": {"voice_id": "", "volume": 1.0},
            "EndScene": {},
        }
        return dict(defaults.get(node_type, {}))

    @staticmethod
    def output_labels(node: Dict[str, Any]) -> List[str]:
        """Labels of a node's output ports"""
        if node["type"] == "BranchChoice":
            return [f"{i}: {c}" for i, c in enumerate(node["parameters"].get("choices", []))]
        if node["type"] == "If":
            return ["0: true", "1: false"]
        if node["type"] in ("EndScene", "End"):
            return []
        return ["next"]

    @classmethod
    def output_count(cls, node: Dict[str, Any]) -> int:
        """Number of output ports a node has"""
        return len(cls.output_labels(node))

    @staticmethod
    def has_input(node: Dict[str, Any]) -> bool:
        return bool(node["type"] != "entry")

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
        self.edges = [e for e in self.edges if not (e["from"] == from_id and e.get("port", 0) == port)]

    def set_node_param(self, node_id: str, param_name: str, value: Any) -> None:
        """Set a node parameter. Shrinking a choice list drops edges from
        ports that no longer exist."""
        if node_id not in self.nodes:
            return
        node = self.nodes[node_id]
        node["parameters"][param_name] = value
        count = self.output_count(node)
        self.edges = [
            e for e in self.edges if e["from"] != node_id or e.get("port", 0) < count
        ]

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
        self.nodes = {n["id"]: {**n, "parameters": dict(n.get("parameters", {}))}
                      for n in data.get("nodes", [])}
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

    def validate_graph(self) -> List[str]:
        """Validate with the engine compiler when it is built, else with
        basic structural checks."""
        try:
            from vn_engine.compiler import GraphCompiler
        except ImportError:
            return self._basic_validation()
        try:
            errors = GraphCompiler().validate_detailed(self.export_json())
        except ValueError as e:  # parse error
            return [str(e)]
        return [
            f"{e['error_type']}: {e['message']}" + (f" (node {e['node_id']})" if e["node_id"] else "")
            for e in errors
        ]

    def _basic_validation(self) -> List[str]:
        errors = []
        if not any(n["type"] == "entry" for n in self.nodes.values()):
            errors.append("No entry node found")
        connected = {e["from"] for e in self.edges} | {e["to"] for e in self.edges}
        for node_id, node in self.nodes.items():
            if node["type"] != "entry" and node_id not in connected:
                errors.append(f"Node '{node_id}' is not connected")
        return errors


def parse_param(text: str) -> Any:
    """Interpret a typed parameter: JSON if it parses (numbers, true, lists), else a string"""
    try:
        return json.loads(text)
    except (ValueError, TypeError):
        return text


def format_param(value: Any) -> str:
    return value if isinstance(value, str) else json.dumps(value, ensure_ascii=False)


class EditorApp:
    """DearPyGui front end for NodeEditor"""

    def __init__(self, path: Optional[str] = None) -> None:
        import dearpygui.dearpygui as dpg

        self.dpg = dpg
        self.model = NodeEditor()
        self.path: Optional[str] = path
        # DearPyGui item ids → model
        self.node_items: Dict[str, int] = {}
        self.item_nodes: Dict[int, str] = {}
        self.output_attrs: Dict[int, Tuple[str, int]] = {}
        self.input_attrs: Dict[int, str] = {}
        self.link_edges: Dict[int, Tuple[str, int]] = {}
        if path:
            self.model.load_from_file(path)

    # --- building -------------------------------------------------------

    def run(self) -> None:
        dpg = self.dpg
        dpg.create_context()
        dpg.create_viewport(title="VN Engine - Visual Node Editor", width=1280, height=800)

        with dpg.file_dialog(show=False, callback=self._on_open_file, tag="open_dialog",
                             width=600, height=400):
            dpg.add_file_extension(".json")
        with dpg.file_dialog(show=False, callback=self._on_save_file, tag="save_dialog",
                             width=600, height=400):
            dpg.add_file_extension(".json")

        with dpg.window(tag="main"):
            with dpg.group(horizontal=True):
                dpg.add_button(label="New", callback=lambda: self._new_graph())
                dpg.add_button(label="Open", callback=lambda: dpg.show_item("open_dialog"))
                dpg.add_button(label="Save", callback=lambda: self._save())
                dpg.add_button(label="Save As", callback=lambda: dpg.show_item("save_dialog"))
                dpg.add_button(label="Validate", callback=lambda: self._validate())
                dpg.add_button(label="Delete Selected", callback=lambda: self._delete_selected())
                dpg.add_input_text(tag="graph_id", width=160, default_value=self.model.graph_id,
                                   callback=lambda s, a: setattr(self.model, "graph_id", a))
            with dpg.group(horizontal=True):
                with dpg.child_window(width=180):
                    dpg.add_text("Add node:")
                    for node_type in NODE_TYPES:
                        dpg.add_button(label=node_type, width=-1,
                                       callback=lambda s, a, u: self._add_node(u),
                                       user_data=node_type)
                with dpg.group():
                    dpg.add_node_editor(tag="canvas", callback=self._on_link,
                                        delink_callback=self._on_delink, height=-60)
                    dpg.add_text("", tag="status", wrap=1000)

        dpg.set_primary_window("main", True)
        self._rebuild_canvas()
        dpg.setup_dearpygui()
        dpg.show_viewport()
        dpg.start_dearpygui()
        dpg.destroy_context()

    def _rebuild_canvas(self) -> None:
        dpg = self.dpg
        dpg.delete_item("canvas", children_only=True)
        self.node_items.clear()
        self.item_nodes.clear()
        self.output_attrs.clear()
        self.input_attrs.clear()
        self.link_edges.clear()
        for node_id in self.model.nodes:
            self._draw_node(node_id)
        for edge in self.model.edges:
            self._draw_link(edge["from"], edge.get("port", 0), edge["to"])
        dpg.set_value("graph_id", self.model.graph_id)

    def _draw_node(self, node_id: str) -> None:
        dpg = self.dpg
        node = self.model.nodes[node_id]
        item = dpg.add_node(label=node["type"], parent="canvas", pos=node.get("position", [100, 100]))
        self.node_items[node_id] = item
        self.item_nodes[item] = node_id

        if self.model.has_input(node):
            attr = dpg.add_node_attribute(parent=item, attribute_type=dpg.mvNode_Attr_Input)
            dpg.add_text(node_id, parent=attr)
            self.input_attrs[attr] = node_id
        else:
            attr = dpg.add_node_attribute(parent=item, attribute_type=dpg.mvNode_Attr_Static)
            dpg.add_text(node_id, parent=attr)

        for name, value in node["parameters"].items():
            attr = dpg.add_node_attribute(parent=item, attribute_type=dpg.mvNode_Attr_Static)
            if name == "choices":
                dpg.add_input_text(label="choices (; separated)", parent=attr, width=180,
                                   default_value="; ".join(map(str, value)), on_enter=True,
                                   callback=self._on_choices_edit, user_data=node_id)
            else:
                dpg.add_input_text(label=name, parent=attr, width=180,
                                   default_value=format_param(value),
                                   callback=self._on_param_edit, user_data=(node_id, name))

        for port, label in enumerate(self.model.output_labels(node)):
            attr = dpg.add_node_attribute(parent=item, attribute_type=dpg.mvNode_Attr_Output)
            dpg.add_text(label, parent=attr)
            self.output_attrs[attr] = (node_id, port)

    def _draw_link(self, from_id: str, port: int, to_id: str) -> None:
        out_attr = next((a for a, v in self.output_attrs.items() if v == (from_id, port)), None)
        in_attr = next((a for a, v in self.input_attrs.items() if v == to_id), None)
        if out_attr is None or in_attr is None:
            return
        link = self.dpg.add_node_link(out_attr, in_attr, parent="canvas")
        self.link_edges[link] = (from_id, port)

    # --- callbacks ------------------------------------------------------

    def _status(self, text: str) -> None:
        self.dpg.set_value("status", text)

    def _sync_positions(self) -> None:
        for node_id, item in self.node_items.items():
            x, y = self.dpg.get_item_pos(item)
            self.model.set_node_position(node_id, x, y)

    def _add_node(self, node_type: str) -> None:
        self._sync_positions()
        offset = 20 * (len(self.model.nodes) % 10)
        node_id = self.model.create_node(node_type, 60 + offset, 60 + offset)
        self._draw_node(node_id)

    def _on_link(self, sender: int, app_data: Tuple[int, int]) -> None:
        a, b = app_data
        # Links can be dragged in either direction
        if a in self.input_attrs:
            a, b = b, a
        if a not in self.output_attrs or b not in self.input_attrs:
            return
        from_id, port = self.output_attrs[a]
        to_id = self.input_attrs[b]
        if self.model.add_edge(from_id, to_id, port):
            self._sync_positions()
            self._rebuild_canvas()  # replaces any previous link from this port

    def _on_delink(self, sender: int, link: int) -> None:
        if link in self.link_edges:
            from_id, port = self.link_edges.pop(link)
            self.model.remove_edge(from_id, port)
        self.dpg.delete_item(link)

    def _on_param_edit(self, sender: int, text: str, user_data: Tuple[str, str]) -> None:
        node_id, name = user_data
        self.model.set_node_param(node_id, name, parse_param(text))

    def _on_choices_edit(self, sender: int, text: str, node_id: str) -> None:
        choices = [c.strip() for c in text.split(";") if c.strip()]
        self.model.set_node_param(node_id, "choices", choices)
        self._sync_positions()
        self._rebuild_canvas()  # port count may have changed

    def _delete_selected(self) -> None:
        self._sync_positions()
        for item in self.dpg.get_selected_nodes("canvas"):
            node_id = self.item_nodes.get(item)
            if node_id:
                self.model.delete_node(node_id)
        self._rebuild_canvas()

    def _new_graph(self) -> None:
        self.model = NodeEditor()
        self.path = None
        self._rebuild_canvas()
        self._status("New graph")

    def _save(self) -> None:
        if self.path is None:
            self.dpg.show_item("save_dialog")
            return
        self._sync_positions()
        self.model.save_to_file(self.path)
        self._status(f"Saved {self.path}")

    def _on_save_file(self, sender: int, app_data: Dict[str, Any]) -> None:
        self.path = app_data["file_path_name"]
        self._save()

    def _on_open_file(self, sender: int, app_data: Dict[str, Any]) -> None:
        path = app_data["file_path_name"]
        try:
            self.model.load_from_file(path)
        except (OSError, ValueError, KeyError) as e:
            self._status(f"Could not open {path}: {e}")
            return
        self.path = path
        self._rebuild_canvas()
        self._status(f"Opened {path}")

    def _validate(self) -> None:
        self._sync_positions()
        errors = self.model.validate_graph()
        self._status("Graph is valid" if not errors else "\n".join(errors))


def run_editor(path: Optional[str] = None) -> None:
    """Run the node editor application"""
    EditorApp(path).run()


if __name__ == "__main__":
    run_editor(sys.argv[1] if len(sys.argv) > 1 else None)
