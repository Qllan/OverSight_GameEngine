# Visual Novel Engine - 2D Game Engine with Block-based Visual Logic

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Python API Layer                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │   Editor    │  │   Engine    │  │   Plugin System     │  │
│  │ (DearPyGui) │  │   Wrapper   │  │   (Runtime Disc.)   │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
├─────────────────────────────────────────────────────────────┤
│                      PyO3 Bridge                             │
├─────────────────────────────────────────────────────────────┤
│                    Rust Core Engine                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │  EventBus   │  │PluginRegistry│ │   AssetManager      │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │GraphCompiler│  │   VM Core   │  │  FixedTickLoop(60)  │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## Project Structure

```
.
├── Cargo.toml              # Rust crate (cdylib + rlib)
├── pyproject.toml          # Python project + maturin config
├── requirements.txt        # Dev dependencies
├── src/
│   ├── lib.rs              # PyO3 module entry point (vn_engine._core)
│   ├── core/
│   │   ├── event_bus.rs    # Event enum + EventBus (Python callbacks)
│   │   ├── plugin_registry.rs # Block/plugin registry
│   │   ├── asset_manager.rs# Asset registration stubs
│   │   └── tick_loop.rs    # Fixed-timestep loop (60 FPS default)
│   ├── compiler/
│   │   ├── graph_parser.rs # JSON → Graph AST
│   │   ├── validator.rs    # Structural validation
│   │   ├── codegen.rs      # AST → Opcodes (labels + resolved jumps)
│   │   └── graph_compiler.rs # Python-facing compiler
│   ├── vm/
│   │   ├── machine.rs      # Bytecode interpreter (pure Rust)
│   │   ├── executor.rs     # VMExecutor: Python wrapper + event dispatch
│   │   ├── state.rs        # PyVMState snapshot
│   │   ├── opcodes.rs      # Opcode re-export
│   │   └── stack_frame.rs  # Call frames (reserved)
│   └── plugins/
│       └── novel_plugin.rs # Visual novel blocks
├── python/vn_engine/
│   ├── __init__.py         # Package init
│   ├── engine.py           # High-level Engine API
│   ├── compiler.py         # GraphCompiler wrapper
│   ├── vm.py               # VMExecutor wrapper
│   └── editor/main.py      # DearPyGui node editor
├── tests/                  # pytest suite + fixtures
└── examples/
    ├── simple_novel.json   # Linear scene with one choice
    └── menu_loop.json      # Menu the player can return to
```

## Key Interfaces

### Rust Core Interfaces

#### `Event`
Delivered to `EventBus` subscribers as `(payload_json)`; wildcard (`"*"`)
subscribers receive `(event_type, payload_json)`.
```rust
pub enum Event {
    GraphStart { graph_id: String },
    GraphEnd { graph_id: String },
    NodeEnter { node_id: String, node_type: String },
    BlockCall { node_id: Option<String>, block_type: String, args: JsonValue },
    VariableChanged { name: String, value: JsonValue },
    YieldRequest { reason: String },
    Error { message: String, node_id: Option<String> },
}
```

#### `Opcode`
```rust
pub enum Opcode {
    PUSH(JsonValue),
    VAR_SET { name: String },
    VAR_GET { name: String },
    EQ,
    IF_JUMP { target: usize },
    JUMP { target: usize },
    YIELD { wait_type: String },
    WAIT { frames: u32 },
    CALL_BLOCK { block_type: String, args: Vec<String> },
    END,
}
```

### Python API Interfaces

#### `Engine`
```python
class Engine:
    def init(self) -> None
    def register_plugin(self, plugin_name: str) -> None
    def subscribe(self, event_type: str, callback) -> None   # payload dict; "*" for all
    def load_graph(self, json_path: str) -> str               # returns graph_id
    def load_graph_from_dict(self, graph: dict) -> str
    def run(self) -> None
    def step(self) -> dict | None            # one opcode; None once ended
    def run_until_yield(self) -> dict | None # until the player is needed; None once ended
    def resume(self, choice: int | None = None) -> dict | None
    def stop(self) -> None
```

A scene is played like this:
```python
engine.run()
state = engine.run_until_yield()
while state is not None:
    if state["yield_reason"] == "choice":
        choices = state["last_block_args"]["choices"]
        state = engine.resume(ask_player(choices))
    else:  # "input": ShowText is waiting for a click
        show(state["last_block_args"])       # {"text": ..., "speaker": ...}
        state = engine.resume()
    state = engine.run_until_yield()
```

#### `GraphCompiler`
```python
class GraphCompiler:
    def parse(self, json_data: dict) -> dict
    def validate(self, json_data: dict) -> list[str]
    def validate_detailed(self, json_data: dict) -> list[dict]  # error_type, message, node_id, hint
    def compile(self, json_data: dict) -> dict                  # opcodes + disassembly
```

## Compilation

Each node becomes a labelled block of opcodes. Nodes are emitted depth-first
from the start node (the `entry` node, or the only node without inputs), so a
node's first successor normally follows it directly; any other transition is
a `JUMP`, resolved to the target node's first opcode in a second pass. Nodes
not reachable from the start are skipped.

Graphs may contain loops (e.g. a menu the player returns to), as long as every
loop passes through a node that waits for the player (`ShowText` or
`BranchChoice`); otherwise validation fails with `BUSY_LOOP`.

| Node | Bytecode |
|------|----------|
| `entry` | *(nothing)*, then flow to successor |
| `ShowText` | `PUSH text`, `PUSH speaker`, `CALL_BLOCK ShowText [text, speaker]`, `YIELD input` |
| `BranchChoice` | `PUSH choices`, `CALL_BLOCK`, `YIELD choice`, `VAR_SET __branch_result`, then per output *i*: `VAR_GET`, `PUSH i`, `EQ`, `IF_JUMP →successor i`; `END` if nothing matched |
| `SetVariable` | `PUSH value`, `VAR_SET name` |
| `EndScene` / `End` | `END` |
| other blocks | `PUSH` each parameter (sorted by name), `CALL_BLOCK type [names]` |

A node with no outputs ends the scene. `BranchChoice` outputs are ordered by the
edge `port`; edges without a port keep their order in the file.

## Opcodes Specification

| Opcode | Description | Stack Effect |
|--------|-------------|--------------|
| `PUSH(value)` | Push value onto stack | → +1 |
| `VAR_SET{name}` | Pop and store in variable | -1 → |
| `VAR_GET{name}` | Push variable value (error if undefined) | → +1 |
| `EQ` | Pop b, pop a, push `a == b` (numbers compared by value) | -2 → +1 |
| `IF_JUMP{target}` | Pop, jump if truthy (non-zero, non-empty, `true`) | -1 → |
| `JUMP{target}` | Unconditional jump | → |
| `YIELD{wait_type}` | Pause until `resume()`; a `choice` resume pushes the choice | → (+1 for `choice`) |
| `WAIT{frames}` | Stay on this opcode for `frames` steps | → |
| `CALL_BLOCK{type, args}` | Pop `args` (listed in push order) and emit `BlockCall` | -n → |
| `END` | Terminate execution | → |

Runtime errors (stack underflow, undefined variable) stop the VM, emit an
`Error` event and raise `RuntimeError` in Python.

## Example Valid Graph JSON

```json
{
  "graph_id": "intro_scene",
  "version": "1.0",
  "nodes": [
    {
      "id": "start",
      "type": "entry",
      "position": [20, 120],
      "parameters": {}
    },
    {
      "id": "show_title",
      "type": "ShowText",
      "position": [150, 120],
      "parameters": {
        "text": "Welcome to the Visual Novel!",
        "speaker": "Narrator"
      }
    },
    {
      "id": "player_choice",
      "type": "BranchChoice",
      "position": [540, 120],
      "parameters": {
        "choices": ["Start Adventure", "Exit"]
      }
    },
    {
      "id": "adventure_path",
      "type": "ShowText",
      "position": [920, 40],
      "parameters": {
        "text": "Your adventure begins...",
        "speaker": "Narrator"
      }
    },
    {
      "id": "exit_path",
      "type": "EndScene",
      "position": [920, 260],
      "parameters": {}
    }
  ],
  "edges": [
    {"from": "start", "to": "show_title"},
    {"from": "show_title", "to": "player_choice"},
    {"from": "player_choice", "to": "adventure_path", "port": 0},
    {"from": "player_choice", "to": "exit_path", "port": 1}
  ]
}
```

## Build & Run Instructions

### Prerequisites
- Python 3.11+
- Rust 1.70+ (via rustup)

### Development Setup

```bash
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt

# Build the Rust extension and install the package in editable mode
pip install -e .            # or: maturin develop

# Run the editor (optionally with a graph to open)
python -m vn_engine.editor.main examples/simple_novel.json

# Run tests
cargo test                  # compiler + VM unit tests
pytest                      # Python API, VM and editor model tests
```

### Release Build

```bash
maturin build --release
```

## Phase 1 Checklist

- [x] Project structure created
- [x] Rust core: EventBus, PluginRegistry, AssetManager stubs
- [x] Rust core: FixedTickLoop (60 FPS)
- [x] PyO3 bindings for core components
- [x] Python Engine API wrapper
- [x] DearPyGui editor with node canvas
- [x] GraphCompiler: JSON → AST → validation → opcodes
- [x] VM: deterministic execution with YIELD support
- [x] pytest suite for compiler and VM
- [x] Example JSON graph

## Risks & Next Steps (Phase 2)

### Risks
1. **SDL2 dependencies**: pygame-ce may need system libraries (libsdl2-dev).
2. **Determinism**: Floating point in blocks could break determinism.

### Phase 2 Planned Features
- Actual rendering with pygame-ce (sprites, backgrounds, text boxes)
- Save/Load system integration (VM state is pc + stack + variables)
- Audio playback (BGM, SFX, voice)
- More block types (animations, transitions, mini-games)
- Export to standalone executable (PyInstaller + Rust binary)
