"""DearPyGui-based Visual Node Editor for VN Engine"""

import json
from pathlib import Path
from typing import Dict, List, Any, Optional
import dearpygui.dearpygui as dpg


class NodeEditor:
    """Visual node editor using DearPyGui"""
    
    def __init__(self):
        self.nodes: Dict[str, Dict[str, Any]] = {}
        self.edges: List[Dict[str, Any]] = []
        self.selected_node: Optional[str] = None
        self.graph_id: str = "untitled"
        
    def create_node(self, node_type: str, x: int = 100, y: int = 100) -> str:
        """Create a new node"""
        import uuid
        node_id = str(uuid.uuid4())[:8]
        
        self.nodes[node_id] = {
            "id": node_id,
            "type": node_type,
            "position": [x, y],
            "parameters": self._default_params_for_type(node_type),
        }
        
        return node_id
    
    def _default_params_for_type(self, node_type: str) -> Dict[str, Any]:
        """Get default parameters for a node type"""
        defaults = {
            "entry": {},
            "ShowText": {"text": "", "speaker": ""},
            "BranchChoice": {"choices": ["Option 1", "Option 2"]},
            "SetVariable": {"name": "", "value": ""},
            "EndScene": {},
        }
        return defaults.get(node_type, {})
    
    def delete_node(self, node_id: str) -> None:
        """Delete a node and its edges"""
        if node_id in self.nodes:
            del self.nodes[node_id]
            self.edges = [e for e in self.edges 
                         if e["from"] != node_id and e["to"] != node_id]
    
    def add_edge(self, from_id: str, to_id: str, port: int = 0) -> None:
        """Add an edge between nodes"""
        if from_id in self.nodes and to_id in self.nodes:
            self.edges.append({
                "from": from_id,
                "to": to_id,
                "port": port,
            })
    
    def remove_edge(self, from_id: str, to_id: str) -> None:
        """Remove an edge"""
        self.edges = [e for e in self.edges 
                     if not (e["from"] == from_id and e["to"] == to_id)]
    
    def set_node_param(self, node_id: str, param_name: str, value: Any) -> None:
        """Set a node parameter"""
        if node_id in self.nodes:
            self.nodes[node_id]["parameters"][param_name] = value
    
    def export_json(self) -> Dict[str, Any]:
        """Export graph to JSON format"""
        return {
            "graph_id": self.graph_id,
            "version": "1.0",
            "nodes": list(self.nodes.values()),
            "edges": self.edges,
        }
    
    def import_json(self, data: Dict[str, Any]) -> None:
        """Import graph from JSON"""
        self.graph_id = data.get("graph_id", "imported")
        self.nodes = {n["id"]: n for n in data.get("nodes", [])}
        self.edges = data.get("edges", [])
    
    def save_to_file(self, path: str) -> None:
        """Save graph to file"""
        with open(path, 'w') as f:
            json.dump(self.export_json(), f, indent=2)
    
    def load_from_file(self, path: str) -> None:
        """Load graph from file"""
        with open(path, 'r') as f:
            data = json.load(f)
        self.import_json(data)
    
    def validate_graph(self) -> List[str]:
        """Basic validation of graph structure"""
        errors = []
        
        # Check for entry node
        entry_nodes = [n for n in self.nodes.values() if n["type"] == "entry"]
        if not entry_nodes:
            errors.append("No entry node found")
        
        # Check for orphan nodes
        connected = set()
        for edge in self.edges:
            connected.add(edge["from"])
            connected.add(edge["to"])
        
        for node_id, node in self.nodes.items():
            if node["type"] != "entry" and node_id not in connected:
                errors.append(f"Node '{node_id}' is not connected")
        
        return errors


def run_editor():
    """Run the node editor application"""
    dpg.create_context()
    dpg.create_viewport(title="VN Engine - Visual Node Editor", width=1200, height=800)
    
    editor = NodeEditor()
    
    with dpg.window(label="Toolbar", pos=(0, 0), size=(200, 50), no_move=True):
        if dpg.button(label="New Graph"):
            editor = NodeEditor()
        dpg.same_line()
        if dpg.button(label="Save"):
            pass  # Implement save dialog
        dpg.same_line()
        if dpg.button(label="Load"):
            pass  # Implement load dialog
        dpg.same_line()
        if dpg.button(label="Validate"):
            errors = editor.validate_graph()
            if errors:
                print("Validation errors:", errors)
            else:
                print("Graph is valid")
        dpg.same_line()
        if dpg.button(label="Export JSON"):
            data = editor.export_json()
            print(json.dumps(data, indent=2))
    
    with dpg.window(label="Node Palette", pos=(0, 50), size=(200, 300), no_move=True):
        dpg.text("Drag to add nodes:")
        node_types = ["entry", "ShowText", "BranchChoice", "SetVariable", "EndScene"]
        for nt in node_types:
            if dpg.button(label=nt, width=180):
                editor.create_node(nt, 250, 100)
    
    with dpg.window(label="Canvas", pos=(200, 0)):
        with dpg.node_editor():
            for node_id, node_data in editor.nodes.items():
                with dpg.node(label=f"{node_data['type']}", pos=node_data["position"]):
                    dpg.text(f"ID: {node_id}")
                    for param, value in node_data.get("parameters", {}).items():
                        dpg.text(f"{param}: {value}")
    
    dpg.setup_dearpygui()
    dpg.show_viewport()
    dpg.start_dearpygui()
    dpg.destroy_context()


if __name__ == "__main__":
    run_editor()
