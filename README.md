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
/workspace
├── Cargo.toml              # Rust workspace configuration
├── pyproject.toml          # Python project + maturin config
├── requirements.txt        # Python dependencies
├── README.md               # Documentation
├── src/
│   ├── lib.rs              # PyO3 module entry point
│   ├── core/
│   │   ├── mod.rs          # Core module exports
│   │   ├── event_bus.rs    # EventBus implementation
│   │   ├── plugin.rs       # Plugin trait & registry
│   │   ├── asset_manager.rs# Asset loading stubs
│   │   └── tick_loop.rs    # Fixed 60 FPS loop
│   ├── compiler/
│   │   ├── mod.rs          # Compiler exports
│   │   ├── graph_parser.rs # JSON → Graph AST
│   │   ├── validator.rs    # DAG validation
│   │   ├── topology.rs     # Topological sort
│   │   └── codegen.rs      # AST → Opcodes
│   ├── vm/
│   │   ├── mod.rs          # VM exports
│   │   ├── opcodes.rs      # Opcode definitions
│   │   ├── executor.rs     # VM execution loop
│   │   ├── stack_frame.rs  # Stack & variables
│   │   └── state.rs        # VMState for debugging
│   └── plugins/
│       ├── mod.rs          # Plugin system exports
│       └── novel_plugin.rs # Example: visual novel blocks
├── python/
│   ├── __init__.py         # Python package init
│   ├── engine.py           # High-level Engine API
│   ├── compiler.py         # GraphCompiler Python wrapper
│   ├── vm.py               # VM executor wrapper
│   └── editor/
│       ├── __init__.py     # Editor package init
│       ├── main.py         # DearPyGui editor entry
│       ├── canvas.py       # Node canvas implementation
│       ├── nodes.py        # Block node definitions
│       └── serializer.py   # JSON export/import
├── tests/
│   ├── test_compiler.py    # Compiler tests (valid/broken graphs)
│   ├── test_vm.py          # VM tests (determinism, yield, errors)
│   └── fixtures/
│       ├── valid_graph.json
│       └── broken_graph.json
├── assets/                 # Game assets (stubs for Phase 1)
└── examples/
    └── simple_novel.json   # Example visual novel graph
```

## Key Interfaces

### Rust Core Interfaces

#### `Event`
```rust
pub enum Event {
    GraphStart { graph_id: String },
    GraphEnd { graph_id: String },
    NodeEnter { node_id: String, node_type: String },
    NodeExit { node_id: String },
    VariableChanged { name: String, value: JsonValue },
    YieldRequest { reason: String },
    Error { message: String, node_id: Option<String> },
}
```

#### `Plugin` Trait
```rust
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn register_blocks(&self, registry: &mut BlockRegistry);
    fn on_event(&self, event: &Event, ctx: &mut PluginContext);
    fn on_save(&self, state: &mut serde_json::Value) {}
    fn on_load(&self, state: &serde_json::Value) {}
}
```

#### `Opcode`
```rust
#[derive(Debug, Clone)]
pub enum Opcode {
    PUSH(JsonValue),
    VAR_SET { name: String },
    VAR_GET { name: String },
    IF_JUMP { target: usize },
    JUMP { target: usize },
    YIELD { wait_type: String },
    WAIT { frames: u32 },
    CALL_BLOCK { block_type: String, args: Vec<String> },
    END,
}
```

#### `VMState`
```rust
pub struct VMState {
    pub pc: usize,                    // Program counter
    pub stack: Vec<JsonValue>,        // Operand stack
    pub variables: HashMap<String, JsonValue>,
    pub call_stack: Vec<CallFrame>,
    pub current_node_id: Option<String>,
    pub is_yielding: bool,
    pub yield_reason: Option<String>,
}
```

### Python API Interfaces

#### `Engine`
```python
class Engine:
    def init(self) -> None
    def register_plugin(self, plugin_path: str) -> None
    def load_graph(self, json_path: str) -> GraphID
    def run_graph(self, graph_id: GraphID) -> None
    def step(self) -> VMState | None  # Single VM step, returns state or None if yielded
    def stop(self) -> None
```

#### `GraphCompiler`
```python
class GraphCompiler:
    def parse(self, json_data: dict) -> GraphAST
    def validate(self, ast: GraphAST) -> list[ValidationError]
    def compile(self, ast: GraphAST) -> list[Opcode]
```

#### `BlockNode` (Editor)
```python
@dataclass
class BlockNode:
    id: str
    node_type: str
    position: tuple[int, int]
    inputs: list[Port]
    outputs: list[Port]
    parameters: dict[str, Any]
```

## Opcodes Specification

| Opcode | Description | Stack Effect |
|--------|-------------|--------------|
| `PUSH(value)` | Push value onto stack | → +1 |
| `VAR_SET{name}` | Pop and store in variable | -1 → |
| `VAR_GET{name}` | Push variable value | → +1 |
| `IF_JUMP{target}` | Pop, jump if truthy | -1 → |
| `JUMP{target}` | Unconditional jump | → |
| `YIELD{wait_type}` | Pause execution, return control | → |
| `WAIT{frames}` | Wait N frames (busy wait) | → |
| `CALL_BLOCK{type, args}` | Execute registered block | -n → |
| `END` | Terminate execution | → |

## Example Valid Graph JSON

```json
{
  "graph_id": "intro_scene",
  "version": "1.0",
  "nodes": [
    {
      "id": "start",
      "type": "entry",
      "position": [100, 100],
      "parameters": {}
    },
    {
      "id": "show_title",
      "type": "ShowText",
      "position": [300, 100],
      "parameters": {
        "text": "Welcome to the Visual Novel!",
        "speaker": "Narrator"
      }
    },
    {
      "id": "player_choice",
      "type": "BranchChoice",
      "position": [500, 100],
      "parameters": {
        "choices": ["Start Adventure", "Exit"]
      }
    },
    {
      "id": "adventure_path",
      "type": "ShowText",
      "position": [700, 50],
      "parameters": {
        "text": "Your adventure begins...",
        "speaker": "Narrator"
      }
    },
    {
      "id": "exit_path",
      "type": "EndScene",
      "position": [700, 150],
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
- maturin: `pip install maturin`

### Development Setup

```bash
# Install maturin
pip install maturin

# Build and install Rust extension in development mode
cd /workspace
maturin develop

# Run the editor
python -m python.editor.main

# Run tests
pytest tests/
```

### Release Build

```bash
maturin build --release
```

## Phase 1 Checklist

- [x] Project structure created
- [ ] Rust core: EventBus, PluginRegistry, AssetManager stubs
- [ ] Rust core: FixedTickLoop (60 FPS)
- [ ] PyO3 bindings for core components
- [ ] Python Engine API wrapper
- [ ] DearPyGui editor with node canvas
- [ ] GraphCompiler: JSON → AST → validation → opcodes
- [ ] VM: deterministic execution with YIELD support
- [ ] pytest suite for compiler and VM
- [ ] Example JSON graph

## Risks & Next Steps (Phase 2)

### Risks
1. **Space constraints**: Rust toolchain requires ~2GB. Monitor disk space.
2. **SDL2 dependencies**: pygame-ce may need system libraries (libsdl2-dev).
3. **PyO3 GIL management**: Ensure proper GIL handling in callbacks.
4. **Determinism**: Floating point in blocks could break determinism.

### Phase 2 Planned Features
- Actual rendering with pygame-ce (sprites, backgrounds, text boxes)
- Save/Load system integration
- Audio playback (BGM, SFX, voice)
- More block types (animations, transitions, mini-games)
- Export to standalone executable (PyInstaller + Rust binary)
