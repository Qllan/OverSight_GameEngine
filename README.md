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
├── Cargo.toml / pyproject.toml / requirements.txt
├── src/
│   ├── lib.rs                  # PyO3 module entry point (vn_engine._core)
│   ├── pyjson.rs               # Python <-> JSON conversions
│   ├── core/
│   │   ├── event_bus.rs        # Event enum + EventBus (Python callbacks)
│   │   ├── plugin_registry.rs  # Block definitions (typed parameters)
│   │   ├── asset_manager.rs    # Asset registry, folder scan
│   │   └── tick_loop.rs        # Fixed-timestep loop (60 FPS default)
│   ├── compiler/
│   │   ├── expr.rs             # Expression language and {text} templates
│   │   ├── graph_parser.rs     # JSON → Graph AST
│   │   ├── validator.rs        # Graph checks (blocks, params, loops, assets)
│   │   ├── codegen.rs          # AST → opcodes
│   │   ├── project.rs          # Multi-scene projects
│   │   └── graph_compiler.rs   # Python-facing compiler
│   ├── vm/
│   │   ├── machine.rs          # Interpreter: scenes, saves, rollback
│   │   ├── values.rs           # Arithmetic/comparison on JSON values
│   │   ├── executor.rs         # VMExecutor: Python wrapper + events
│   │   └── state.rs            # PyVMState snapshot
│   └── plugins/
│       └── novel_plugin.rs     # ShowText, PlayVoice
├── python/vn_engine/
│   ├── engine.py               # High-level Engine API
│   ├── compiler.py / vm.py     # Thin wrappers
│   ├── _core.pyi               # Type stubs for the Rust extension
│   └── editor/main.py          # DearPyGui node editor
├── tests/                      # pytest suite + fixtures
└── examples/
    ├── simple_novel.json       # Linear scene with one choice
    ├── menu_loop.json          # Menu the player can return to
    ├── affection.json          # Variables, If, {placeholders}
    └── project/                # Three scenes with CallScene / JumpScene
```

## Writing Scenes

A scene is a graph: nodes plus edges `{"from", "to", "port"}`. Execution
starts at the `entry` node and follows the edges.

### Built-in nodes

| Node | Parameters | Outputs |
|------|------------|---------|
| `entry` | – | 1 |
| `ShowText`, `PlayVoice`, … | see *Blocks* | 0–1 |
| `BranchChoice` | `choices`: list of labels | one per choice (`port` = index) |
| `If` | `condition`: expression | 0 = true, 1 = false |
| `SetVariable` | `name`, and `value` (constant) **or** `expression` | 1 |
| `Wait` | `frames` (60 = one second at 60 FPS) | 1 |
| `CallScene` | `scene`: graph_id — runs it, then continues here | 1 |
| `JumpScene` | `scene`: graph_id — continues there | 0 |
| `End` / `EndScene` | – ends the scene (returns to the caller, if any) | 0 |

Loops are allowed ("back to the menu") as long as every loop passes through a
node that waits for the player; otherwise validation reports `BUSY_LOOP`.

### Expressions and text

```text
affection >= 5 and not met_rival
score + 10 * bonus
persistent.endings_seen
```

Numbers, `"strings"`, `true`/`false`/`null`, variables (dotted names allowed),
`+ - * / %`, comparisons, `and`/`or`/`not` (also `&& || !`), parentheses.
Integers stay integers while results are exact (`7 / 2` is `3.5`); `+` joins
text when either side is a string. `and`/`or` short-circuit.

Text parameters (`ShowText.text`, `speaker`, choice labels) may contain
`{expression}` placeholders: `"Hi, {player}! You have {gold} gold."` —
`{{` and `}}` are literal braces.

### Blocks

Every node that is not built in is a *block*: an action the host performs.
Blocks are declared in the `PluginRegistry`; `novel_plugin` (installed by
default) provides:

| Block | Parameters | Waits for |
|-------|------------|-----------|
| `ShowText` | `text` (text), `speaker` (text, default `""`) | `input` |
| `PlayVoice` | `voice_id` (asset:audio), `volume` (number, default 1.0) | – |

Games add their own blocks from Python:

```python
from vn_engine import Engine, _core

engine.register_block(
    _core.BlockDefinition(
        "Shake", "Shake Screen", ["exec"], ["next"],
        [_core.ParameterDefinition("strength", "expression", 1)],
        wait="animation",           # optional: pause until resume()
    ),
    handler=lambda args: screen.shake(args["strength"]),
)
```

Parameter types: `string`, `text` (placeholders expanded), `expression`
(evaluated at runtime), `number`/`float`, `int`, `bool`, `array`, `object`,
`any`, `asset` / `asset:<image|audio|font|data>`. A parameter without a
default is required. The validator reports unknown blocks (with a
"did you mean" hint), missing, unknown or mistyped parameters, and too many
outputs.

## Projects

A project bundles scenes, the start scene and initial variables:

```json
{
  "project_id": "market_day",
  "start": "town",
  "variables": {"gold": 10, "persistent.endings": 0},
  "scenes": ["scenes/town.json", "scenes/shop.json", "scenes/ending.json"]
}
```

Scene entries are paths relative to the project file, or inline graphs.
Links to missing scenes are reported as `UNKNOWN_SCENE`, naming the scene and
node. A single graph can still be loaded on its own with `load_graph()`.

## Playing

```python
engine = Engine()
engine.init()
engine.load_project("examples/project/project.json")
engine.run()

while (state := engine.tick()) is not None:     # once per frame
    if state["yield_reason"] == "choice":
        state = engine.resume(ask(state["last_block_args"]["choices"]))
    elif state["yield_reason"] == "input":       # a ShowText line
        show(state["last_block_args"])           # {"text": ..., "speaker": ...}
        if clicked or (skipping and state["seen_before"]):
            engine.resume()
```

`tick()` advances one frame: it counts down `Wait` nodes and runs until the
game needs the player. `run_until_yield()` does the same without advancing
frames (tests, tools). Blocks without `wait` (e.g. `PlayVoice`) reach the host
through events.

### Events

`engine.subscribe(event_type, callback)` — the callback gets the payload dict;
`"*"` subscribes to everything with `(event_type, payload)`.

| Event | Payload |
|-------|---------|
| `GraphStart` / `GraphEnd` | `graph_id` (a scene is entered / left) |
| `NodeEnter` | `graph_id`, `node_id`, `node_type` |
| `BlockCall` | `graph_id`, `node_id`, `block_type`, `args` |
| `VariableChanged` | `name`, `value` |
| `YieldRequest` | `reason` |
| `Error` | `message`, `graph_id`, `node_id` |

### Saves, rollback and persistent data

- `save_game(path)` / `load_game(path)` (or `save_state()` / `load_state()`
  for dicts) store scene, position, variables, call stack and backlog.
  Positions are also stored per node, so a save made with an older build
  still loads after the project is edited; it is refused only if the node it
  stopped at was removed, and the error names that node. Variables added to
  the project since get their initial values.
- `rollback()` returns to the previous line or choice (last 100 by default,
  `set_rollback_limit()`); `backlog()` lists what was shown, with the picked
  choices.
- Variables named `persistent.*` and the "seen" markers behind
  `state["seen_before"]` are shared by all playthroughs and not affected by
  loading or rollback. Store them with `save_persistent(path)` /
  `load_persistent(path)`.

### Assets

`engine.scan_assets("assets")` registers every image, audio, font and data file
under the folder as `"<relative path without extension>"` (e.g.
`"voice/mia_001"`) and from then on checks `asset` parameters when graphs are
loaded (`UNKNOWN_ASSET`, `WRONG_ASSET_TYPE`). `engine.asset_path(id)` returns
the file path.

## Editor

```bash
python -m vn_engine.editor.main examples/project/project.json   # or a single graph
```

- The palette and each node's parameter widgets come from the block registry:
  numbers get number fields, `bool` a checkbox, `scene` a list of the
  project's scenes, `asset:*` a list of scanned assets (when the project has
  an `"assets"` folder). Optional parameters are added with `+ name`.
- Validation runs after every edit. Problem nodes get a red title and the
  message inside the node; the panel below lists all problems with hints,
  and scenes with problems are marked `(!)` in the project list.
- Projects: switch scenes in the sidebar, add scenes, set the start scene;
  Save writes `project.json` and every scene file. `Ctrl+S` saves, `Del`
  deletes the selected nodes.
- Run panel: **Play (F5)** runs the project (or graph) being edited in the
  editor: the transcript shows lines and choices, Next / choice buttons
  answer, the current node is highlighted in green and the editor follows
  execution into other scenes. **F9** toggles a breakpoint on the selected
  nodes (`[BP]` in the title); when paused, **Continue (F5)** runs on and
  **Step node (F10)** stops at the next node. The inspector shows the
  position, call stack, variables and persistent variables.

The same controls exist in code: `engine.add_breakpoint(scene, node)`,
`state["paused_at"]`, `engine.step_node()`, `engine.continue_execution()`,
`engine.debug_info()`; `vn_engine.editor.debugger.DebugSession` wraps them
for tools.

## Compilation

Each node becomes a labelled block of opcodes, emitted depth-first from the
start node so a node's first successor usually follows directly; other
transitions are `JUMP`s resolved in a second pass. Block arguments are pushed
in the order the block declares them, then `CALL_BLOCK`, then `YIELD` if the
block waits. `BranchChoice` compiles to a jump table on the picked index,
`If` to a conditional jump. Unreachable nodes are skipped.

## Opcodes Specification

| Opcode | Description | Stack Effect |
|--------|-------------|--------------|
| `PUSH(value)` / `POP` / `DUP` | Stack manipulation | +1 / -1 / +1 |
| `VAR_SET{name}` / `VAR_GET{name}` | Store / load a variable | -1 / +1 |
| `ADD SUB MUL DIV MOD` | Arithmetic (`ADD` also joins text and arrays) | -2 → +1 |
| `EQ NE LT LE GT GE` | Comparison (numbers or strings) | -2 → +1 |
| `NEG` / `NOT` | Unary minus / logical not | -1 → +1 |
| `CONCAT{n}` | Join n values as text | -n → +1 |
| `MAKE_ARRAY{n}` | Build a list of n values | -n → +1 |
| `IF_JUMP{t}` / `JUMP_IF_FALSE{t}` | Pop, jump if truthy / falsy | -1 |
| `JUMP{t}` | Unconditional jump | |
| `YIELD{reason}` | Wait for `resume()`; a `choice` resume pushes the pick | (+1) |
| `WAIT{frames}` | Wait for this many `tick()`s | |
| `CALL_BLOCK{type, args}` | Pop args, emit `BlockCall` | -n |
| `CALL_SCENE{scene}` / `JUMP_SCENE{scene}` | Run / continue in another scene | |
| `END` | End the scene: return to the caller, or stop | |

Runtime errors (undefined variable, division by zero, bad operand types) stop
the VM, emit an `Error` event naming the scene and node, and raise
`RuntimeError` in Python.

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
mypy                        # strict type check (uses vn_engine/_core.pyi)
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

## Risks & Next Steps

### Risks
1. **SDL2 dependencies**: pygame-ce may need system libraries (libsdl2-dev).
2. **Determinism**: Floating point in blocks could break determinism.

### Next: quality of life
- Rendering with pygame-ce (backgrounds, sprites, text box), audio playback
- Save slots UI, auto-save, skip/auto modes built on `seen_before`
- Export to a standalone executable (PyInstaller + Rust binary)
