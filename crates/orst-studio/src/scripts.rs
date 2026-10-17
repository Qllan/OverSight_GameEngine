//! Scripts inside the editor, in C# here and C++ in `native`. Both run
//! in the editor's own process and call the C API that ORST.exe exports
//! on the editor's engine, so they change the very world the editor
//! shows. For C#, .NET runs in-process: the ORST.Scripting assembly
//! (bindings/csharp/Scripting) gets the engine as a C handle. A new build
//! of the scripts replaces the old one in place.

use crate::console::Level;
use crate::project::Language;
use orst::ffi::OrstEngine;
use std::ffi::{c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

type LogFn = extern "system" fn(u32, *const u8, usize);

/// What scripts printed, until the editor puts it in the Console
static LOG: Mutex<Vec<(Level, String)>> = Mutex::new(Vec::new());

pub extern "system" fn log(level: u32, text: *const u8, len: usize) {
    let text = match text.is_null() {
        true => String::new(),
        // SAFETY: the host passes len valid bytes for the call's duration
        false => String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(text, len) }).into(),
    };
    let level = match level {
        0 => Level::Info,
        1 => Level::Warning,
        _ => Level::Error,
    };
    LOG.lock().unwrap().push((level, text));
}

/// Lines scripts logged since the last call
pub fn take_log() -> Vec<(Level, String)> {
    std::mem::take(&mut *LOG.lock().unwrap())
}

/// What the editor needs from scripts, in any language
pub trait ScriptHost {
    /// Load a build in place of the one before; the number of Behaviours
    fn load(&mut self, built: &Path) -> Result<u32, String>;
    /// Drop the scripts; false when they are still held in memory
    fn unload(&mut self) -> bool;
    fn begin_play(&mut self);
    /// One tick of every script; how many calls threw
    fn tick(&mut self, dt: f32) -> u32;
    fn end_play(&mut self);
}

/// C# scripts, through .NET
#[derive(Clone, Copy)]
pub struct Scripts {
    load: extern "system" fn(*const u8) -> i32,
    unload: extern "system" fn() -> i32,
    start: extern "system" fn() -> i32,
    tick: extern "system" fn(f32) -> i32,
    stop: extern "system" fn(),
    last_error: extern "system" fn() -> *const u8,
    /// Behaviours in the scripts loaded now
    pub count: u32,
}

impl Scripts {
    /// Where ORST.Scripting.dll is: $ORST_SCRIPTING, a Scripting folder
    /// next to ORST.exe, or the build in this repository
    pub fn host_dir() -> Option<PathBuf> {
        let file = "ORST.Scripting.dll";
        let mut places = Vec::new();
        if let Some(dir) = std::env::var_os("ORST_SCRIPTING") {
            places.push(PathBuf::from(dir));
        }
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|p| Some(p.parent()?.to_path_buf()))
        {
            places.push(dir.join("Scripting"));
        }
        places.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../bindings/csharp/Scripting/bin/Release/net8.0"),
        );
        places.into_iter().find(|d| d.join(file).is_file())
    }

    /// Start .NET (once per process) and hand it `engine`, which must
    /// stay where it is while scripts run
    pub fn start(host: &Path, engine: &mut OrstEngine) -> Result<Scripts, String> {
        // .NET starts once per process and cannot start again, so a
        // project opened later reuses it (with the first host found)
        static STARTED: OnceLock<Result<(Attach, Scripts), String>> = OnceLock::new();
        let (attach, scripts) = STARTED.get_or_init(|| dotnet::start(host)).clone()?;
        let log: LogFn = log;
        if attach(engine.handle().cast(), log as *const c_void) != 0 {
            return Err(scripts.error());
        }
        Ok(scripts)
    }

    fn error(&self) -> String {
        let p = (self.last_error)();
        if p.is_null() {
            return "unknown error".into();
        }
        // SAFETY: the host keeps its last error as a NUL-terminated string
        unsafe { CStr::from_ptr(p.cast()) }.to_string_lossy().into()
    }
}

impl ScriptHost for Scripts {
    fn load(&mut self, dll: &Path) -> Result<u32, String> {
        let path = CString::new(dll.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let n = (self.load)(path.as_ptr().cast());
        self.count = u32::try_from(n).map_err(|_| self.error())?;
        Ok(self.count)
    }

    fn unload(&mut self) -> bool {
        self.count = 0;
        (self.unload)() == 0
    }

    fn begin_play(&mut self) {
        (self.start)();
    }

    fn tick(&mut self, dt: f32) -> u32 {
        (self.tick)(dt).max(0) as u32
    }

    fn end_play(&mut self) {
        (self.stop)();
    }
}

/// Where a project's scripts are built, under its Library
const BUILD_DIR: &str = "Library/Scripts";

/// The code of a project: every script under Assets builds into one
/// library (Scripts.dll for C#, a shared library for C++) from a project
/// file the editor writes in Library/Scripts (open it in an IDE for
/// completion)
#[derive(Debug, Clone)]
pub struct ScriptProject {
    pub root: PathBuf,
    pub language: Language,
    /// C#: where ORST.Scripting.dll is; C++: where orst_script.hpp is
    pub host: PathBuf,
}

/// What the scripts under Assets are like: their number and the newest
/// change, enough to see an edit, a new file or a deleted one
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sources {
    pub count: usize,
    pub newest: u128,
}

#[derive(Debug, Clone)]
pub struct BuildResult {
    pub ok: bool,
    /// Errors and warnings, with paths from the project folder
    pub messages: Vec<(Level, String)>,
    /// The sources as the build saw them
    pub sources: Sources,
}

impl ScriptProject {
    pub fn csproj(&self) -> PathBuf {
        self.root.join(BUILD_DIR).join("Scripts.csproj")
    }

    /// The library a good build makes
    pub fn library(&self) -> PathBuf {
        let out = self.root.join(BUILD_DIR).join("out");
        match self.language {
            Language::CSharp => out.join("Scripts.dll"),
            Language::Cpp => out.join(format!("Scripts.{}", std::env::consts::DLL_EXTENSION)),
        }
    }

    /// Whether `path` is one of this language's sources
    fn is_source(&self, path: &Path) -> bool {
        let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("");
        match self.language {
            Language::CSharp => ext == "cs",
            Language::Cpp => matches!(ext, "cpp" | "cc" | "cxx" | "hpp" | "h"),
        }
    }

    fn stamp_file(&self) -> PathBuf {
        self.root.join(BUILD_DIR).join("built")
    }

    /// The sources the last good build was made from
    pub fn built(&self) -> Option<Sources> {
        let text = std::fs::read_to_string(self.stamp_file()).ok()?;
        let (count, newest) = text.trim().split_once(' ')?;
        Some(Sources {
            count: count.parse().ok()?,
            newest: newest.parse().ok()?,
        })
    }

    pub fn sources(&self) -> Sources {
        fn walk(p: &ScriptProject, dir: &Path, s: &mut Sources) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(p, &path, s);
                } else if p.is_source(&path) {
                    s.count += 1;
                    let changed = entry.metadata().and_then(|m| m.modified()).ok();
                    let at = changed
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_nanos());
                    s.newest = s.newest.max(at);
                }
            }
        }
        let mut s = Sources::default();
        walk(self, &self.root.join(crate::project::ASSETS), &mut s);
        s
    }

    fn csproj_text(&self) -> String {
        let reference = |name: &str| {
            format!(
                "    <Reference Include=\"{name}\">\n      <HintPath>{}</HintPath>\n      <Private>false</Private>\n    </Reference>\n",
                self.host.join(format!("{name}.dll")).display()
            )
        };
        format!(
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <!-- Written by ORST Studio: every .cs file under Assets. Edits here are lost. -->
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    <AssemblyName>Scripts</AssemblyName>
    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>
  </PropertyGroup>
  <ItemGroup>
    <Compile Include="../../{assets}/**/*.cs" />
{net}{scripting}  </ItemGroup>
</Project>
"#,
            assets = crate::project::ASSETS,
            net = reference("ORST.Net"),
            scripting = reference("ORST.Scripting"),
        )
    }

    /// Write the project files when they are missing or out of date
    pub fn write_project(&self) -> Result<(), String> {
        match self.language {
            Language::CSharp => write_if_changed(&self.csproj(), &self.csproj_text()),
            Language::Cpp => crate::native::write_project(self),
        }
    }

    /// Build with dotnet or CMake; blocks for as long as the build takes
    pub fn build(&self) -> BuildResult {
        let sources = self.sources();
        let failed = |text: String| BuildResult {
            ok: false,
            messages: vec![(Level::Error, text)],
            sources,
        };
        if let Err(e) = self.write_project() {
            return failed(e);
        }
        let out = self.library().parent().unwrap().to_path_buf();
        let log = self.root.join(BUILD_DIR).join("build.log");
        let built = match self.language {
            Language::CSharp => dotnet_build(
                &[
                    "-clp:NoSummary".as_ref(),
                    "-o".as_ref(),
                    out.as_os_str(),
                    self.csproj().as_os_str(),
                ],
                &log,
            )
            .map_err(|e| {
                format!("dotnet did not run ({e}); install the .NET 8 SDK to build C# scripts")
            }),
            Language::Cpp => crate::native::build(self, &log),
        };
        let (ok, text) = match built {
            Ok(built) => built,
            Err(e) => return failed(e),
        };
        let mut messages = build_messages(&text, &self.root);
        if ok {
            let stamp = format!("{} {}\n", sources.count, sources.newest);
            let _ = std::fs::write(self.stamp_file(), stamp);
        } else if !messages.iter().any(|(l, _)| *l == Level::Error) {
            messages.push((Level::Error, text.trim().to_string()));
        }
        BuildResult {
            ok,
            messages,
            sources,
        }
    }
}

/// Write `text` to `path` unless it already holds it, so the build tools
/// do not see a change
pub fn write_if_changed(path: &Path, text: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(());
    }
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(path.parent().unwrap()).map_err(io)?;
    std::fs::write(path, text).map_err(io)
}

/// `dotnet build` with `args`; whether it succeeded, and its output
pub fn dotnet_build(args: &[&std::ffi::OsStr], log: &Path) -> std::io::Result<(bool, String)> {
    let mut dotnet = Command::new("dotnet");
    dotnet
        .args(["build", "-nologo", "-v", "q", "-nodeReuse:false"])
        .args(args);
    run_logged(&mut dotnet, log)
}

/// Run a build tool; whether it succeeded, and its output. The output
/// goes through the file `log`, not a pipe: build servers the tools leave
/// running inherit the handle, and on Windows a pipe would then never
/// close.
pub fn run_logged(command: &mut Command, log: &Path) -> std::io::Result<(bool, String)> {
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::File::create(log)?;
    let status = command
        .stdout(file.try_clone()?)
        .stderr(file)
        .stdin(std::process::Stdio::null())
        .status()?;
    Ok((
        status.success(),
        std::fs::read_to_string(log).unwrap_or_default(),
    ))
}

/// The errors and warnings in dotnet's output, once each, with paths
/// from the project folder and without the project file at the end
pub fn build_messages(text: &str, root: &Path) -> Vec<(Level, String)> {
    let mut root = root.display().to_string();
    if !root.ends_with(['/', '\\']) {
        root.push(std::path::MAIN_SEPARATOR);
    }
    let mut out: Vec<(Level, String)> = Vec::new();
    for line in text.lines() {
        // MSVC and dotnet write "x.cs(3,5): error CS1002: ...", GCC and
        // Clang "x.cpp:3:5: error: ..."
        let has = |what: &str| {
            [" ", ":"]
                .iter()
                .any(|end| line.contains(&format!(": {what}{end}")))
        };
        let level = if has("error") || has("fatal error") {
            Level::Error
        } else if has("warning") {
            Level::Warning
        } else {
            continue;
        };
        let mut line = line.trim();
        if let Some(i) = line.rfind(" [") {
            if line.ends_with(".csproj]") {
                line = &line[..i];
            }
        }
        let line = line.strip_prefix(root.as_str()).unwrap_or(line).to_string();
        if !out.iter().any(|(_, l)| *l == line) {
            out.push((level, line));
        }
    }
    out
}

type Attach = extern "system" fn(*mut c_void, *const c_void) -> i32;

#[cfg(not(all(windows, target_env = "gnu")))]
mod dotnet {
    use super::{Attach, Scripts};
    use netcorehost::{nethost, pdcstr, pdcstring::PdCString};
    use std::path::Path;

    pub fn start(host: &Path) -> Result<(Attach, Scripts), String> {
        let path = |name: &str| {
            PdCString::from_os_str(host.join(name).as_os_str()).map_err(|e| e.to_string())
        };
        let fxr = nethost::load_hostfxr()
            .map_err(|e| format!("No .NET 8 runtime found ({e}); install it to use C# scripts"))?;
        let context = fxr
            .initialize_for_runtime_config(path("ORST.Scripting.runtimeconfig.json")?)
            .map_err(|e| format!(".NET did not start: {e}"))?;
        let loader = context
            .get_delegate_loader_for_assembly(path("ORST.Scripting.dll")?)
            .map_err(|e| format!(".NET did not start: {e}"))?;
        let ty = pdcstr!("ORST.Scripting.Host, ORST.Scripting");
        macro_rules! get {
            ($name:literal, $sig:ty) => {
                *loader
                    .get_function_with_unmanaged_callers_only::<$sig>(ty, pdcstr!($name))
                    .map_err(|e| format!("ORST.Scripting has no {}: {e}", $name))?
            };
        }
        let attach = get!(
            "Attach",
            fn(*mut std::ffi::c_void, *const std::ffi::c_void) -> i32
        );
        let scripts = Scripts {
            load: get!("Load", fn(*const u8) -> i32),
            unload: get!("Unload", fn() -> i32),
            start: get!("Start", fn() -> i32),
            tick: get!("Tick", fn(f32) -> i32),
            stop: get!("Stop", fn()),
            last_error: get!("LastError", fn() -> *const u8),
            count: 0,
        };
        Ok((attach, scripts))
    }
}

#[cfg(all(windows, target_env = "gnu"))]
mod dotnet {
    use super::{Attach, Scripts};
    use std::path::Path;

    pub fn start(_: &Path) -> Result<(Attach, Scripts), String> {
        Err("C# scripts need the MSVC build of ORST".into())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use orst::components::{read, write, Collider, Transform, COLLIDER, TRANSFORM};
    use orst::schema::Value;
    use orst::{Engine, Vec2};

    /// Builds the script host with the .NET SDK; None (the test skips)
    /// without one, unless ORST_REQUIRE_DOTNET is set (CI)
    pub fn host() -> Option<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings/csharp/Scripting");
        let log = std::env::temp_dir().join(format!("orst-host-{}.log", std::process::id()));
        let built = dotnet_build(&["-c".as_ref(), "Release".as_ref(), dir.as_os_str()], &log);
        match built {
            Ok((true, _)) => Some(dir.join("bin/Release/net8.0")),
            other => {
                assert!(
                    std::env::var_os("ORST_REQUIRE_DOTNET").is_none(),
                    "the script host did not build: {other:?}"
                );
                eprintln!("skipped: no .NET SDK");
                None
            }
        }
    }

    /// Writes `source` as Assets/Mover.cs (or .cpp) of the project and
    /// builds it
    pub fn build(project: &ScriptProject, source: &str) -> BuildResult {
        let assets = project.root.join(crate::project::ASSETS);
        std::fs::create_dir_all(&assets).unwrap();
        let file = format!("Mover.{}", project.language.extension());
        std::fs::write(assets.join(file), source).unwrap();
        project.build()
    }

    const FIRST: &str = r#"
using ORST;
public enum Mode { Walk, Run }
public class Mover : Behaviour
{
    public float Speed = 3;
    public Mode Mode = Mode.Run;
    public int Starts;
    public int Hits;
    public override void Start() { Starts++; Log("started"); }
    public override void OnContactBegin(Entity other, Contact c) { if (c.Trigger) Hits++; }
    public override void Update(float dt) => Transform.Pos.X += Speed;
}
"#;

    const SECOND: &str = r#"
using ORST;
public class Mover : Behaviour
{
    public float Speed = 1;
    public float Jump = 7;
    public override void Update(float dt) => throw new System.Exception("boom");
}
"#;

    /// .NET is one per process and the log is shared: tests that run
    /// scripts take turns
    pub static DOTNET: Mutex<()> = Mutex::new(());

    #[test]
    fn scripts_run_on_the_editors_engine_and_reload_in_place() {
        let _turn = DOTNET.lock().unwrap_or_else(|e| e.into_inner());
        let Some(host) = host() else { return };
        let dir = std::env::temp_dir().join(format!("orst-scripts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut engine = OrstEngine::new(Engine::headless(16, 16).unwrap());
        let mut scripts = Scripts::start(&host, &mut engine).unwrap();
        let project = ScriptProject {
            root: dir.clone(),
            language: Language::CSharp,
            host: host.clone(),
        };
        assert_eq!(project.sources().count, 0);
        let built = build(&project, FIRST);
        assert!(built.ok, "{:?}", built.messages);
        assert_eq!(project.built(), Some(project.sources()));
        assert_eq!(scripts.load(&project.library()), Ok(1));

        let world = &mut engine.world;
        let id = world.component_id("Mover").expect("the script's component");
        let names: Vec<_> = world.fields(id).iter().map(|f| f.name.clone()).collect();
        assert_eq!(names, ["Speed", "Mode", "Starts", "Hits"]);
        assert_eq!(world.fields(id)[1].options, ["Walk", "Run"]);
        let e = world.spawn();
        write(world, e, TRANSFORM, &Transform::default());
        let shape = Collider {
            size: Vec2::new(10.0, 10.0),
            ..Default::default()
        };
        write(world, e, COLLIDER, &shape);
        // A trigger the mover walks into
        let zone = world.spawn();
        let at = Transform {
            pos: Vec2::new(12.0, 0.0),
            ..Default::default()
        };
        write(world, zone, TRANSFORM, &at);
        write(
            world,
            zone,
            COLLIDER,
            &Collider {
                trigger: 1,
                ..shape
            },
        );
        let defaults = world.default_value(id);
        world.set(e, id, &defaults);
        assert_eq!(world.field(e, id, "Speed"), Some(Value::F32(3.0)));
        assert_eq!(world.field(e, id, "Mode"), Some(Value::I32(1)));

        scripts.begin_play();
        assert_eq!(scripts.tick(1.0 / 60.0), 0);
        engine.step(1.0 / 60.0);
        assert_eq!(scripts.tick(1.0 / 60.0), 0);
        scripts.end_play();
        let t: Transform = read(&engine.world, e, TRANSFORM).unwrap();
        assert_eq!(t.pos.x, 6.0, "Update moved the entity twice");
        assert_eq!(engine.world.field(e, id, "Starts"), Some(Value::I32(1)));
        assert_eq!(
            engine.world.field(e, id, "Hits"),
            Some(Value::I32(1)),
            "at x 3 it touched the trigger"
        );
        let log = take_log();
        assert!(log.iter().any(|(_, l)| l == "started"), "{log:?}");

        // A new build: Speed keeps the value the entity has, Jump is new
        engine.world.set_field(e, id, "Speed", Value::F32(5.0));
        let built = build(&project, SECOND);
        assert!(built.ok, "{:?}", built.messages);
        assert_eq!(scripts.load(&project.library()), Ok(1));
        assert_eq!(engine.world.component_id("Mover"), Some(id));
        assert_eq!(engine.world.field(e, id, "Speed"), Some(Value::F32(5.0)));
        assert_eq!(engine.world.field(e, id, "Jump"), Some(Value::F32(7.0)));
        let log = take_log();
        assert!(
            log.iter().all(|(l, _)| *l != Level::Warning),
            "unloaded: {log:?}"
        );

        scripts.begin_play();
        assert_eq!(scripts.tick(1.0 / 60.0), 1, "the exception is counted");
        scripts.end_play();
        let log = take_log();
        assert!(
            log.iter()
                .any(|(l, t)| *l == Level::Error && t.contains("boom")),
            "{log:?}"
        );
        assert!(scripts.unload(), "the assembly can be unloaded");

        let broken = build(&project, "public class Mover : ORST.Behaviour { int x = }");
        assert!(!broken.ok);
        let (level, first) = &broken.messages[0];
        assert!(
            *level == Level::Error && first.starts_with("Assets"),
            "{first}"
        );
        assert!(
            first.contains("Mover.cs(1,") && first.contains("error CS"),
            "{first}"
        );
        assert!(scripts.load(&dir.join("missing.dll")).is_err());

        // What a new project starts with builds
        let root = dir.join("new");
        let language = crate::project::Language::CSharp;
        crate::project::Project::create(&root, "New", language).unwrap();
        let fresh = ScriptProject {
            root,
            language,
            host,
        };
        let built = fresh.build();
        assert!(
            built.ok && built.messages.is_empty(),
            "{:?}",
            built.messages
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    pub fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if name == "Library" || name.to_string_lossy().ends_with(".meta") {
                continue;
            }
            if path.is_dir() {
                copy_dir(&path, &to.join(name));
            } else {
                std::fs::copy(&path, to.join(name)).unwrap();
            }
        }
    }

    /// The first game made in the editor: walk up into a coin, then into
    /// the wall above it
    #[test]
    fn the_coins_sample_plays() {
        let _turn = DOTNET.lock().unwrap_or_else(|e| e.into_inner());
        let Some(host) = host() else { return };
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/studio/Coins");
        let root = std::env::temp_dir().join(format!("orst-coins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        copy_dir(&sample, &root);
        let (mut project, _) = crate::project::Project::open(&root).unwrap();

        let mut engine = OrstEngine::new(Engine::headless(320, 200).unwrap());
        let mut scripts = Scripts::start(&host, &mut engine).unwrap();
        let code = ScriptProject {
            root: root.clone(),
            language: Language::CSharp,
            host,
        };
        let built = code.build();
        assert!(
            built.ok && built.messages.is_empty(),
            "{:?}",
            built.messages
        );
        assert_eq!(scripts.load(&code.library()), Ok(5));
        let main = project.file.main_scene.clone().unwrap();
        let text = std::fs::read_to_string(project.assets.file(&main)).unwrap();
        let loaded = engine.load_scene(&text, &mut project.assets).unwrap();
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);

        let world = &engine.world;
        let player = world.find_named("Player").unwrap();
        let coin = world.find_named("Coin 1").unwrap();
        let rules = world.find_named("Rules").unwrap();
        let score = world.component_id("Score").unwrap();
        let history = crate::history::History::default();
        let mut play = crate::play::Play::start(&mut engine, history, false, None);
        scripts.begin_play();
        engine.input.set_key(orst::input::Key::Up, true);
        for _ in 0..120 {
            play.advance(&mut engine, Some(&mut scripts), crate::play::TICK);
        }
        let log: Vec<String> = take_log().into_iter().map(|(_, t)| t).collect();
        assert!(log.iter().any(|l| l == "Coins: 1/8"), "{log:?}");
        assert!(!engine.world.alive(coin), "the coin was picked up");
        let field = engine.world.field(rules, score, "Coins");
        assert_eq!(field, Some(Value::I32(1)));
        let t: Transform = read(&engine.world, player, TRANSFORM).unwrap();
        assert!((t.pos.x + 130.0).abs() < 0.01, "straight up: {:?}", t.pos);
        assert!(
            (-91.0..-85.0).contains(&t.pos.y),
            "stopped by the wall: {:?}",
            t.pos
        );

        scripts.end_play();
        let (restored, ..) = play.stop(&mut engine);
        restored.unwrap();
        assert!(engine.world.alive(coin), "Stop brings it back");
        let _ = std::fs::remove_dir_all(&root);
    }
}
