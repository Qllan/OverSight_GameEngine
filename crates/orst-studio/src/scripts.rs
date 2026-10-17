//! C# scripts inside the editor. .NET runs in the editor's own process:
//! the ORST.Scripting assembly (bindings/csharp/Scripting) gets the
//! editor's engine as a C handle, and the scripts call the C API that
//! ORST.exe exports, so they change the very world the editor shows.
//! A new build of the scripts replaces the old one in place.

// The editor starts using it in the next step
#![allow(dead_code)]

use crate::console::Level;
use orst::ffi::OrstEngine;
use std::ffi::{c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

type LogFn = extern "system" fn(u32, *const u8, usize);

/// What scripts printed, until the editor puts it in the Console
static LOG: Mutex<Vec<(Level, String)>> = Mutex::new(Vec::new());

extern "system" fn log(level: u32, text: *const u8, len: usize) {
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
        let (attach, scripts) = dotnet::start(host)?;
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

    /// Load a build of the scripts in place of the one before; the
    /// number of Behaviours in it
    pub fn load(&mut self, dll: &Path) -> Result<u32, String> {
        let path = CString::new(dll.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let n = (self.load)(path.as_ptr().cast());
        self.count = u32::try_from(n).map_err(|_| self.error())?;
        Ok(self.count)
    }

    /// Drop the scripts; false when .NET still holds them
    pub fn unload(&mut self) -> bool {
        self.count = 0;
        (self.unload)() == 0
    }

    pub fn begin_play(&mut self) {
        (self.start)();
    }

    /// One tick of every script; how many threw
    pub fn tick(&mut self, dt: f32) -> u32 {
        (self.tick)(dt).max(0) as u32
    }

    pub fn end_play(&mut self) {
        (self.stop)();
    }
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
    use orst::components::{read, write, Transform, TRANSFORM};
    use orst::schema::Value;
    use orst::Engine;
    use std::process::Command;

    /// Builds the script host with the .NET SDK; None (the test skips)
    /// without one, unless ORST_REQUIRE_DOTNET is set (CI)
    pub fn host() -> Option<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings/csharp/Scripting");
        let built = Command::new("dotnet")
            .args(["build", "-c", "Release", "-nologo", "-v", "q"])
            .arg(&dir)
            .output();
        match built {
            Ok(out) if out.status.success() => Some(dir.join("bin/Release/net8.0")),
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

    /// Builds `source` as a script project against the host in `host`
    pub fn build(host: &Path, dir: &Path, source: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let reference = |name: &str| {
            format!(
                r#"<Reference Include="{name}"><HintPath>{}</HintPath><Private>false</Private></Reference>"#,
                host.join(format!("{name}.dll")).display()
            )
        };
        let project = format!(
            r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup><TargetFramework>net8.0</TargetFramework><Nullable>enable</Nullable>
  <ImplicitUsings>enable</ImplicitUsings><AssemblyName>Game</AssemblyName></PropertyGroup>
  <ItemGroup>{}{}</ItemGroup>
</Project>"#,
            reference("ORST.Net"),
            reference("ORST.Scripting")
        );
        std::fs::write(dir.join("Game.csproj"), project).unwrap();
        std::fs::write(dir.join("Game.cs"), source).unwrap();
        let out = Command::new("dotnet")
            .args(["build", "-c", "Release", "-nologo", "-v", "q", "-o"])
            .arg(dir.join("out"))
            .arg(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        dir.join("out/Game.dll")
    }

    const FIRST: &str = r#"
using ORST;
public enum Mode { Walk, Run }
public class Mover : Behaviour
{
    public float Speed = 3;
    public Mode Mode = Mode.Run;
    public int Starts;
    public override void Start() { Starts++; Log("started"); }
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

    #[test]
    fn scripts_run_on_the_editors_engine_and_reload_in_place() {
        let Some(host) = host() else { return };
        let dir = std::env::temp_dir().join(format!("orst-scripts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut engine = OrstEngine::new(Engine::headless(16, 16).unwrap());
        let mut scripts = Scripts::start(&host, &mut engine).unwrap();
        let dll = build(&host, &dir.join("first"), FIRST);
        assert_eq!(scripts.load(&dll), Ok(1));

        let world = &mut engine.world;
        let id = world.component_id("Mover").expect("the script's component");
        let names: Vec<_> = world.fields(id).iter().map(|f| f.name.clone()).collect();
        assert_eq!(names, ["Speed", "Mode", "Starts"]);
        assert_eq!(world.fields(id)[1].options, ["Walk", "Run"]);
        let e = world.spawn();
        write(world, e, TRANSFORM, &Transform::default());
        let defaults = world.default_value(id);
        world.set(e, id, &defaults);
        assert_eq!(world.field(e, id, "Speed"), Some(Value::F32(3.0)));
        assert_eq!(world.field(e, id, "Mode"), Some(Value::I32(1)));

        scripts.begin_play();
        assert_eq!(scripts.tick(1.0 / 60.0), 0);
        assert_eq!(scripts.tick(1.0 / 60.0), 0);
        scripts.end_play();
        let t: Transform = read(&engine.world, e, TRANSFORM).unwrap();
        assert_eq!(t.pos.x, 6.0, "Update moved the entity twice");
        assert_eq!(engine.world.field(e, id, "Starts"), Some(Value::I32(1)));
        let log = take_log();
        assert!(log.iter().any(|(_, l)| l == "started"), "{log:?}");

        // A new build: Speed keeps the value the entity has, Jump is new
        engine.world.set_field(e, id, "Speed", Value::F32(5.0));
        let dll = build(&host, &dir.join("second"), SECOND);
        assert_eq!(scripts.load(&dll), Ok(1));
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

        let broken = dir.join("missing.dll");
        assert!(scripts.load(&broken).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
