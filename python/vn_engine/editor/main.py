"""DearPyGui-based Visual Node Editor for VN Engine

Run with: python -m vn_engine.editor.main [graph.json | project.json]

Opens a single graph, or a project (a JSON file with "project_id") whose
scenes are listed on the left. The palette and parameter widgets come
from the engine's block registry, and validation runs after every edit,
marking the nodes that have problems.
"""

import json
import sys
from typing import Any, Callable, Dict, List, Optional, Tuple

from vn_engine.editor.model import (
    FLOW,
    NodeEditor,
    ParamSpec,
    ProjectEditor,
    format_error,
    format_param,
    parse_param,
)

__all__ = ["EditorApp", "NodeEditor", "ProjectEditor", "parse_param", "run_editor"]

ERROR_COLOR = (255, 110, 110)
PARAM_WIDTH = 190


def is_project_file(path: str) -> bool:
    with open(path, "r", encoding="utf-8") as f:
        return "project_id" in json.load(f)


class EditorApp:
    """DearPyGui front end for NodeEditor / ProjectEditor"""

    def __init__(self, path: Optional[str] = None) -> None:
        import dearpygui.dearpygui as dpg

        self.dpg = dpg
        self.project: Optional[ProjectEditor] = None
        self.model = NodeEditor()
        self.path: Optional[str] = None
        self.errors: List[Dict[str, Any]] = []
        # DearPyGui item ids → model
        self.node_items: Dict[str, int] = {}
        self.item_nodes: Dict[int, str] = {}
        self.output_attrs: Dict[int, Tuple[str, int]] = {}
        self.input_attrs: Dict[int, str] = {}
        self.link_edges: Dict[int, Tuple[str, int]] = {}
        self.node_error_items: Dict[str, int] = {}
        self.error_theme: Optional[int] = None
        self._pending_path = path

    # --- loading --------------------------------------------------------

    def open_path(self, path: str) -> None:
        """Open a graph or a project file"""
        if is_project_file(path):
            project = ProjectEditor()
            project.load(path)
            self.project = project
            self.path = path
            self.model = project.scenes.get(project.start) or next(iter(project.scenes.values()))
        else:
            self.project = None
            self.model = NodeEditor()
            self.model.load_from_file(path)
            self.path = path

    # --- building -------------------------------------------------------

    def run(self) -> None:
        dpg = self.dpg
        dpg.create_context()
        dpg.create_viewport(title="VN Engine - Visual Node Editor", width=1400, height=860)
        self.build()
        dpg.setup_dearpygui()
        dpg.show_viewport()
        dpg.start_dearpygui()
        dpg.destroy_context()

    def build(self) -> None:
        """Create all windows (requires a DearPyGui context)"""
        dpg = self.dpg
        with dpg.theme() as theme:
            with dpg.theme_component(dpg.mvNode):
                for col in (dpg.mvNodeCol_TitleBar, dpg.mvNodeCol_TitleBarHovered,
                            dpg.mvNodeCol_TitleBarSelected):
                    dpg.add_theme_color(col, (150, 45, 45), category=dpg.mvThemeCat_Nodes)
        self.error_theme = theme

        with dpg.file_dialog(show=False, callback=self._on_open_file, tag="open_dialog",
                             width=640, height=420):
            dpg.add_file_extension(".json")
        with dpg.file_dialog(show=False, callback=self._on_save_file, tag="save_dialog",
                             width=640, height=420):
            dpg.add_file_extension(".json")
        with dpg.file_dialog(show=False, callback=self._on_new_project_file,
                             tag="new_project_dialog", width=640, height=420):
            dpg.add_file_extension(".json")

        with dpg.handler_registry():
            dpg.add_key_press_handler(dpg.mvKey_Delete, callback=lambda: self._delete_selected())
            dpg.add_key_press_handler(dpg.mvKey_S, callback=self._on_key_s)

        with dpg.window(tag="main"):
            with dpg.group(horizontal=True):
                dpg.add_button(label="New Graph", callback=lambda: self._new_graph())
                dpg.add_button(label="New Project", callback=lambda: dpg.show_item("new_project_dialog"))
                dpg.add_button(label="Open", callback=lambda: dpg.show_item("open_dialog"))
                dpg.add_button(label="Save (Ctrl+S)", callback=lambda: self._save())
                dpg.add_button(label="Save As", tag="save_as_button",
                               callback=lambda: dpg.show_item("save_dialog"))
                dpg.add_button(label="Delete Selected (Del)", callback=lambda: self._delete_selected())
                dpg.add_text("graph_id")
                dpg.add_input_text(tag="graph_id", width=160, on_enter=True,
                                   callback=lambda s, a: self._rename_graph(a))
            with dpg.group(horizontal=True):
                with dpg.child_window(width=210, tag="sidebar"):
                    with dpg.group(tag="project_panel", show=False):
                        dpg.add_text("", tag="project_title")
                        dpg.add_group(tag="scene_list")
                        with dpg.group(horizontal=True):
                            dpg.add_input_text(tag="new_scene_id", width=110, hint="scene id")
                            dpg.add_button(label="Add", callback=lambda: self._add_scene())
                        dpg.add_button(label="Set as start scene", width=-1,
                                       callback=lambda: self._set_start())
                        dpg.add_separator()
                    dpg.add_text("Add node:")
                    dpg.add_group(tag="palette")
                with dpg.group():
                    dpg.add_node_editor(tag="canvas", callback=self._on_link,
                                        delink_callback=self._on_delink, height=-170)
                    with dpg.child_window(height=-30, tag="error_panel"):
                        dpg.add_group(tag="error_list")
                    dpg.add_text("", tag="status")

        dpg.set_primary_window("main", True)
        if self._pending_path:
            try:
                self.open_path(self._pending_path)
            except (OSError, ValueError, KeyError) as e:
                self._status(f"Could not open {self._pending_path}: {e}")
        self.refresh_all()

    def refresh_all(self) -> None:
        self._build_palette()
        self._build_project_panel()
        self._rebuild_canvas()
        self.validate()

    def _build_palette(self) -> None:
        dpg = self.dpg
        dpg.delete_item("palette", children_only=True)
        category = None
        for node_type in self.model.node_types():
            spec = self.model.spec(node_type)
            assert spec is not None
            if spec.category != category:
                category = spec.category
                dpg.add_text(category if category == FLOW else f"Blocks: {category}",
                             parent="palette", color=(170, 170, 255))
            dpg.add_button(label=spec.display_name, width=-1, parent="palette",
                           callback=lambda s, a, u: self._add_node(u), user_data=node_type)

    def _build_project_panel(self) -> None:
        dpg = self.dpg
        dpg.configure_item("project_panel", show=self.project is not None)
        dpg.configure_item("save_as_button", show=self.project is None)
        dpg.configure_item("graph_id", enabled=self.project is None)
        dpg.delete_item("scene_list", children_only=True)
        if self.project is None:
            return
        dpg.set_value("project_title", f"Project: {self.project.project_id}")
        broken = {e["graph_id"] for e in self.errors}
        for scene_id in self.project.scene_ids():
            label = scene_id
            if scene_id == self.project.start:
                label += "  [start]"
            if scene_id in broken:
                label += "  (!)"
            dpg.add_selectable(label=label, parent="scene_list",
                               default_value=scene_id == self.model.graph_id,
                               callback=lambda s, a, u: self._switch_scene(u), user_data=scene_id)

    def _rebuild_canvas(self) -> None:
        dpg = self.dpg
        dpg.delete_item("canvas", children_only=True)
        self.node_items.clear()
        self.item_nodes.clear()
        self.output_attrs.clear()
        self.input_attrs.clear()
        self.link_edges.clear()
        self.node_error_items.clear()
        for node_id in self.model.nodes:
            self._draw_node(node_id)
        for edge in self.model.edges:
            self._draw_link(edge["from"], edge.get("port", 0), edge["to"])
        dpg.set_value("graph_id", self.model.graph_id)

    def _draw_node(self, node_id: str) -> None:
        dpg = self.dpg
        node = self.model.nodes[node_id]
        spec = self.model.spec(node["type"])
        title = spec.display_name if spec else f"{node['type']} (unknown)"
        item = dpg.add_node(label=title, parent="canvas", pos=node.get("position", [100, 100]))
        self.node_items[node_id] = item
        self.item_nodes[item] = node_id

        kind = dpg.mvNode_Attr_Input if self.model.has_input(node) else dpg.mvNode_Attr_Static
        attr = dpg.add_node_attribute(parent=item, attribute_type=kind)
        dpg.add_text(node_id, parent=attr, color=(160, 160, 160))
        if kind == dpg.mvNode_Attr_Input:
            self.input_attrs[attr] = node_id

        params = node["parameters"]
        specs = list(spec.params) if spec else []
        known = {p.name for p in specs}
        # Parameters the block doesn't declare are shown so they can be fixed
        specs += [ParamSpec(name, "any") for name in params if name not in known]
        for param in specs:
            attr = dpg.add_node_attribute(parent=item, attribute_type=dpg.mvNode_Attr_Static)
            if param.name in params:
                self._param_widget(attr, node_id, param, params[param.name])
            else:
                dpg.add_button(label=f"+ {param.name}", parent=attr, small=True,
                               callback=lambda s, a, u: self._add_param(*u),
                               user_data=(node_id, param))

        for port, label in enumerate(self.model.output_labels(node)):
            attr = dpg.add_node_attribute(parent=item, attribute_type=dpg.mvNode_Attr_Output)
            dpg.add_text(label, parent=attr)
            self.output_attrs[attr] = (node_id, port)

        attr = dpg.add_node_attribute(parent=item, attribute_type=dpg.mvNode_Attr_Static)
        self.node_error_items[node_id] = dpg.add_text(
            "", parent=attr, color=ERROR_COLOR, wrap=PARAM_WIDTH + 60, show=False
        )

    def _param_widget(self, parent: int, node_id: str, param: ParamSpec, value: Any) -> None:
        dpg = self.dpg
        base = param.param_type.split(":")[0]
        on_change: Callable[..., None] = lambda s, a, u: self._on_param_edit(u, a)
        user = (node_id, param.name)
        common: Dict[str, Any] = {"label": param.name, "parent": parent, "user_data": user}

        if param.name == "choices":
            dpg.add_input_text(label="choices (;)", parent=parent, width=PARAM_WIDTH,
                               default_value="; ".join(map(str, value)), on_enter=True,
                               callback=self._on_choices_edit, user_data=node_id)
        elif base == "bool":
            dpg.add_checkbox(default_value=bool(value), callback=on_change, **common)
        elif base == "int":
            dpg.add_input_int(default_value=int(value) if isinstance(value, (int, float)) else 0,
                              width=PARAM_WIDTH, callback=on_change, **common)
        elif base in ("number", "float"):
            dpg.add_input_float(default_value=float(value) if isinstance(value, (int, float)) else 0.0,
                                width=PARAM_WIDTH, format="%.3f", callback=on_change, **common)
        elif base == "scene" and self.project is not None:
            dpg.add_combo(self.project.scene_ids(), default_value=str(value),
                          width=PARAM_WIDTH, callback=on_change, **common)
        elif base == "asset" and self.project is not None and self.project.assets is not None:
            kind = param.param_type.split(":")[1] if ":" in param.param_type else ""
            dpg.add_combo(self.project.asset_ids(kind), default_value=str(value),
                          width=PARAM_WIDTH, callback=on_change, **common)
        elif base == "text":
            dpg.add_input_text(default_value=format_param(value), multiline=True,
                               width=PARAM_WIDTH, height=46, callback=on_change, **common)
        elif base == "any":
            dpg.add_input_text(default_value=format_param(value), width=PARAM_WIDTH,
                               callback=lambda s, a, u: self._on_param_edit(u, parse_param(a)),
                               **common)
        else:  # string, expression, asset without a scan
            dpg.add_input_text(default_value=format_param(value), width=PARAM_WIDTH,
                               callback=on_change, **common)

        # Optional parameters without a default can be removed again
        if not param.required and param.default is None:
            dpg.add_button(label="x", parent=parent, small=True,
                           callback=lambda s, a, u: self._remove_param(*u), user_data=user)

    def _draw_link(self, from_id: str, port: int, to_id: str) -> None:
        out_attr = next((a for a, v in self.output_attrs.items() if v == (from_id, port)), None)
        in_attr = next((a for a, v in self.input_attrs.items() if v == to_id), None)
        if out_attr is None or in_attr is None:
            return
        link = self.dpg.add_node_link(out_attr, in_attr, parent="canvas")
        self.link_edges[link] = (from_id, port)

    # --- validation -----------------------------------------------------

    def validate(self) -> List[Dict[str, Any]]:
        """Validate and show errors on nodes, in the list and the scene list"""
        dpg = self.dpg
        if self.project is not None:
            self.errors = self.project.validate()
            shown = [e for e in self.errors
                     if e["graph_id"] in (self.model.graph_id, None)]
            other = len(self.errors) - len(shown)
        else:
            self.errors = self.model.validate_detailed()
            shown, other = self.errors, 0

        by_node: Dict[str, List[str]] = {}
        for error in shown:
            if error["node_id"]:
                by_node.setdefault(error["node_id"], []).append(
                    f"{error['error_type']}: {error['message']}"
                )
        for node_id, item in self.node_items.items():
            messages = by_node.get(node_id, [])
            text_item = self.node_error_items[node_id]
            dpg.set_value(text_item, "\n".join(messages))
            dpg.configure_item(text_item, show=bool(messages))
            dpg.bind_item_theme(item, self.error_theme if messages else 0)

        dpg.delete_item("error_list", children_only=True)
        if not self.errors:
            dpg.add_text("No problems found", parent="error_list", color=(120, 220, 120))
        for error in shown:
            dpg.add_text(format_error(error), parent="error_list", color=ERROR_COLOR, wrap=1100)
            if error.get("hint"):
                dpg.add_text(f"    {error['hint']}", parent="error_list", wrap=1100)
        if other:
            dpg.add_text(f"{other} problem(s) in other scenes (marked with (!))",
                         parent="error_list", color=(230, 190, 90))
        if self.project is not None:
            self._build_project_panel()
        return self.errors

    # --- callbacks ------------------------------------------------------

    def _status(self, text: str) -> None:
        self.dpg.set_value("status", text)

    def _sync_positions(self) -> None:
        for node_id, item in self.node_items.items():
            x, y = self.dpg.get_item_pos(item)
            self.model.set_node_position(node_id, x, y)

    def _changed(self, rebuild: bool = False) -> None:
        if rebuild:
            self._sync_positions()
            self._rebuild_canvas()
        self.validate()

    def _add_node(self, node_type: str) -> None:
        self._sync_positions()
        offset = 20 * (len(self.model.nodes) % 10)
        node_id = self.model.create_node(node_type, 60 + offset, 60 + offset)
        self._draw_node(node_id)
        self._changed()

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
            self._changed(rebuild=True)  # replaces any previous link from this port

    def _on_delink(self, sender: int, link: int) -> None:
        if link in self.link_edges:
            from_id, port = self.link_edges.pop(link)
            self.model.remove_edge(from_id, port)
        self.dpg.delete_item(link)
        self._changed()

    def _on_param_edit(self, user_data: Tuple[str, str], value: Any) -> None:
        node_id, name = user_data
        self.model.set_node_param(node_id, name, value)
        self._changed()

    def _add_param(self, node_id: str, param: ParamSpec) -> None:
        empty = ParamSpec(param.name, param.param_type, required=True).empty_value()
        self.model.set_node_param(node_id, param.name, empty)
        self._changed(rebuild=True)

    def _remove_param(self, node_id: str, name: str) -> None:
        self.model.set_node_param(node_id, name, None)
        self._changed(rebuild=True)

    def _on_choices_edit(self, sender: int, text: str, node_id: str) -> None:
        choices = [c.strip() for c in text.split(";") if c.strip()]
        self.model.set_node_param(node_id, "choices", choices)
        self._changed(rebuild=True)  # port count may have changed

    def _delete_selected(self) -> None:
        # Del while typing in a field must not delete nodes
        if self.dpg.is_item_active(self.dpg.get_focused_item() or 0):
            return
        selected = self.dpg.get_selected_nodes("canvas")
        if not selected:
            return
        for item in selected:
            node_id = self.item_nodes.get(item)
            if node_id:
                self.model.delete_node(node_id)
        self._changed(rebuild=True)

    def _on_key_s(self) -> None:
        dpg = self.dpg
        if dpg.is_key_down(dpg.mvKey_LControl) or dpg.is_key_down(dpg.mvKey_RControl):
            self._save()

    def _rename_graph(self, graph_id: str) -> None:
        if self.project is None and graph_id:
            self.model.graph_id = graph_id
            self._changed()

    def _switch_scene(self, scene_id: str) -> None:
        if self.project is None or scene_id not in self.project.scenes:
            return
        self._sync_positions()
        self.model = self.project.scenes[scene_id]
        self._rebuild_canvas()
        self.validate()
        self._status(f"Scene {scene_id}")

    def _add_scene(self) -> None:
        if self.project is None:
            return
        scene_id = self.dpg.get_value("new_scene_id").strip()
        if not scene_id:
            self._status("Enter a scene id first")
            return
        try:
            self.project.add_scene(scene_id)
        except ValueError as e:
            self._status(str(e))
            return
        self.dpg.set_value("new_scene_id", "")
        self._switch_scene(scene_id)

    def _set_start(self) -> None:
        if self.project is not None:
            self.project.set_start(self.model.graph_id)
            self.validate()

    def _new_graph(self) -> None:
        self.project = None
        self.model = NodeEditor()
        self.model.create_node("entry", 40, 120)
        self.path = None
        self.refresh_all()
        self._status("New graph")

    def _save(self) -> None:
        self._sync_positions()
        if self.project is not None:
            self.project.save()
            self._status(f"Saved project {self.path}")
            return
        if self.path is None:
            self.dpg.show_item("save_dialog")
            return
        self.model.save_to_file(self.path)
        self._status(f"Saved {self.path}")

    def _on_save_file(self, sender: int, app_data: Dict[str, Any]) -> None:
        self.path = app_data["file_path_name"]
        self._save()

    def _on_new_project_file(self, sender: int, app_data: Dict[str, Any]) -> None:
        path = app_data["file_path_name"]
        project = ProjectEditor()
        project.new(path, project_id=app_data.get("file_name", "game").rsplit(".", 1)[0])
        project.save()
        self.project = project
        self.path = path
        self.model = project.scenes[project.start]
        self.refresh_all()
        self._status(f"Created project {path}")

    def _on_open_file(self, sender: int, app_data: Dict[str, Any]) -> None:
        path = app_data["file_path_name"]
        try:
            self.open_path(path)
        except (OSError, ValueError, KeyError) as e:
            self._status(f"Could not open {path}: {e}")
            return
        self.refresh_all()
        self._status(f"Opened {path}")


def run_editor(path: Optional[str] = None) -> None:
    """Run the node editor application"""
    EditorApp(path).run()


if __name__ == "__main__":
    run_editor(sys.argv[1] if len(sys.argv) > 1 else None)
