"""Debug sessions: run the scene or project being edited, without GUI code.

A DebugSession compiles what is in the editor, runs it on a fresh Engine
and keeps a transcript of what the player would see. It stops at
player input, at breakpoints and on errors; Wait nodes are skipped over
(frames are advanced as fast as possible).
"""

import json
from typing import Any, Dict, List, Optional, Set, Tuple

from vn_engine.editor.model import NodeEditor, ProjectEditor
from vn_engine.engine import Engine

#: Frames advanced per call before giving up (a runaway Wait chain)
MAX_FRAMES = 100_000


class DebugSession:
    def __init__(
        self,
        graph: NodeEditor,
        project: Optional[ProjectEditor] = None,
        breakpoints: Optional[Set[Tuple[str, str]]] = None,
    ) -> None:
        self.graph = graph
        self.project = project
        self.breakpoints: Set[Tuple[str, str]] = set(breakpoints or ())
        self.engine = Engine()
        self.transcript: List[str] = []
        self.state: Optional[Dict[str, Any]] = None
        self.error: Optional[str] = None
        self.started = False

    # --- lifecycle ------------------------------------------------------

    def start(self) -> None:
        """Compile and run until the first stop. Compile errors end up in
        `error` instead of raising."""
        self.engine.init()
        self.engine.subscribe("*", self._on_event)
        try:
            if self.project is not None:
                self.engine.load_project_from_dict(
                    self.project.export_json(inline=True), base_dir=str(self.project.base_dir)
                )
            else:
                self.engine.load_graph_from_dict(self.graph.export_json())
        except (ValueError, RuntimeError) as e:
            self.error = str(e)
            return
        for scene, node in self.breakpoints:
            self.engine.add_breakpoint(scene, node)
        self.engine.run()
        self.started = True
        self._advance()

    def stop(self) -> None:
        if self.started and self.engine.is_running:
            self.engine.stop()
        self.state = None

    # --- player actions -------------------------------------------------

    def next(self) -> None:
        """Continue after a line of text (or any non-choice wait)"""
        if self.waiting_for == "input":
            self.engine.resume()
            self._advance()

    def choose(self, index: int) -> None:
        if self.waiting_for == "choice":
            choices = self.choices
            self.transcript.append(f"  > {choices[index]}")
            self.engine.resume(index)
            self._advance()

    # --- debugger actions -----------------------------------------------

    def toggle_breakpoint(self, scene: str, node_id: str) -> bool:
        """Returns whether the breakpoint is now set"""
        key = (scene, node_id)
        enabled = key not in self.breakpoints
        if enabled:
            self.breakpoints.add(key)
        else:
            self.breakpoints.discard(key)
        if self.started:
            if enabled:
                self.engine.add_breakpoint(scene, node_id)
            else:
                self.engine.remove_breakpoint(scene, node_id)
        return enabled

    def continue_(self) -> None:
        if self.paused:
            self.engine.continue_execution()
            self._advance()

    def step_node(self) -> None:
        """Run to the next node, stopping there even without a breakpoint"""
        if self.paused or self.waiting_for == "input":
            self.engine.step_node()
            if self.waiting_for == "input":
                self.engine.resume()
            self._advance()

    # --- queries --------------------------------------------------------

    @property
    def finished(self) -> bool:
        return self.started and self.state is None

    @property
    def paused(self) -> bool:
        return self.state is not None and self.state["paused_at"] is not None

    @property
    def waiting_for(self) -> Optional[str]:
        if self.state is None or self.paused:
            return None
        reason: Optional[str] = self.state["yield_reason"]
        if reason is None:
            return None
        return "choice" if reason == "choice" else "input"

    @property
    def choices(self) -> List[str]:
        if self.state is None or self.state["last_block_args"] is None:
            return []
        return [str(c) for c in self.state["last_block_args"].get("choices", [])]

    @property
    def location(self) -> Optional[Tuple[str, str]]:
        """(scene, node) execution is at"""
        if self.state is None or self.state["current_node_id"] is None:
            return None
        return self.state["current_graph_id"], self.state["current_node_id"]

    def inspect(self) -> Dict[str, Any]:
        return self.engine.debug_info() if self.started else {}

    # --- internals ------------------------------------------------------

    def _advance(self) -> None:
        try:
            for _ in range(MAX_FRAMES):
                self.state = self.engine.tick()
                if self.state is None:
                    self.transcript.append("-- end --")
                    return
                if self.state["is_yielding"] or self.state["paused_at"]:
                    return
            self.error = f"Still running after {MAX_FRAMES} frames"
        except RuntimeError as e:
            self.error = str(e)
            self.transcript.append(f"!! {e}")
            self.state = None

    def _on_event(self, kind: str, payload: Dict[str, Any]) -> None:
        if kind == "BlockCall":
            args = payload["args"]
            if payload["block_type"] == "ShowText":
                speaker = args.get("speaker")
                self.transcript.append(f"{speaker}: {args['text']}" if speaker else args["text"])
            elif payload["block_type"] == "BranchChoice":
                self.transcript.append("  [" + " | ".join(map(str, args["choices"])) + "]")
            else:
                self.transcript.append(f"  [{payload['block_type']}] {json.dumps(args)}")
        elif kind == "GraphStart" and self.project is not None:
            self.transcript.append(f"== {payload['graph_id']} ==")
        elif kind == "Breakpoint":
            self.transcript.append(f"-- paused at {payload['graph_id']}/{payload['node_id']} --")
