//! C++ scripts: CMake builds every C++ file under Assets, with
//! orst_script.hpp, into one shared library, and the editor loads it into
//! its own process. The library calls the C API that ORST.exe exports:
//! on Linux and macOS the symbols resolve against the editor itself, on
//! Windows through an import library made from the editor's export list.
//! A new build is loaded from a copy, so the build can overwrite the
//! file while the old copy is still in use.

use crate::scripts::{log, run_logged, write_if_changed, ScriptHost, ScriptProject};
use orst::ffi::OrstEngine;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::process::Command;

include!(concat!(env!("OUT_DIR"), "/exports.rs"));

/// How the editor builds scripts; with debug info, so a debugger can step
/// into them
const CONFIG: &str = "RelWithDebInfo";

/// Where orst.hpp and orst_script.hpp are: $ORST_INCLUDE, an include
/// folder next to ORST.exe, or the headers in this repository
pub fn include_dir() -> Option<PathBuf> {
    let mut places = Vec::new();
    if let Some(dir) = std::env::var_os("ORST_INCLUDE") {
        places.push(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| Some(p.parent()?.to_path_buf()))
    {
        places.push(dir.join("include"));
    }
    places.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../orst/include"));
    places
        .into_iter()
        .find(|d| d.join("orst_script.hpp").is_file())
}

fn build_dir(project: &ScriptProject) -> PathBuf {
    project
        .library()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// CMake paths use forward slashes everywhere
fn cmake_path(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

fn cmake_text(project: &ScriptProject) -> String {
    format!(
        r#"# Written by ORST Studio: every C++ file under Assets. Edits here are lost.
cmake_minimum_required(VERSION 3.16)
project(Scripts CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
# For completion in editors that read it
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

file(GLOB_RECURSE SOURCES CONFIGURE_DEPENDS
    "${{CMAKE_CURRENT_SOURCE_DIR}}/../../{assets}/*.cpp"
    "${{CMAKE_CURRENT_SOURCE_DIR}}/../../{assets}/*.cc"
    "${{CMAKE_CURRENT_SOURCE_DIR}}/../../{assets}/*.cxx")
# A built game links the same scripts against its own exe and puts them
# elsewhere
set(ORST_OUT "${{CMAKE_CURRENT_SOURCE_DIR}}/out" CACHE PATH "Where the library goes")
set(ORST_HOST_DEF "${{CMAKE_CURRENT_SOURCE_DIR}}/host.def" CACHE FILEPATH "The exports of the exe")
# The C++ runtime inside the library, so a built game needs nothing more
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
add_library(Scripts SHARED main.cpp ${{SOURCES}})
target_include_directories(Scripts PRIVATE "{include}")
# Only the orst_script_ functions leave the library, and on Linux each
# build gets its own copy of every inline function, so a reload is clean
set_target_properties(Scripts PROPERTIES
    PREFIX ""
    CXX_VISIBILITY_PRESET hidden
    VISIBILITY_INLINES_HIDDEN ON
    LIBRARY_OUTPUT_DIRECTORY "${{ORST_OUT}}$<0:>"
    RUNTIME_OUTPUT_DIRECTORY "${{ORST_OUT}}$<0:>")
if(CMAKE_CXX_COMPILER_ID STREQUAL "GNU")
    target_compile_options(Scripts PRIVATE -fno-gnu-unique)
endif()

# The engine's functions are in ORST.exe
set(HOST_DEF "${{ORST_HOST_DEF}}")
if(MSVC)
    target_compile_options(Scripts PRIVATE /utf-8 /EHsc)
    set(HOST_LIB "${{CMAKE_CURRENT_BINARY_DIR}}/host.lib")
    add_custom_command(OUTPUT "${{HOST_LIB}}"
        COMMAND "${{CMAKE_AR}}" /nologo "/def:${{HOST_DEF}}" /machine:x64 "/out:${{HOST_LIB}}"
        DEPENDS "${{HOST_DEF}}" VERBATIM)
elseif(WIN32)
    set(HOST_LIB "${{CMAKE_CURRENT_BINARY_DIR}}/libhost.a")
    add_custom_command(OUTPUT "${{HOST_LIB}}"
        COMMAND "${{CMAKE_DLLTOOL}}" -d "${{HOST_DEF}}" -l "${{HOST_LIB}}"
        DEPENDS "${{HOST_DEF}}" VERBATIM)
elseif(APPLE)
    target_link_options(Scripts PRIVATE -undefined dynamic_lookup)
endif()
if(WIN32)
    add_custom_target(host DEPENDS "${{HOST_LIB}}")
    add_dependencies(Scripts host)
    target_link_libraries(Scripts PRIVATE "${{HOST_LIB}}")
endif()
"#,
        assets = crate::project::ASSETS,
        include = cmake_path(&project.host),
    )
}

/// The running editor's file name
fn exe_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| Some(p.file_name()?.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "ORST.exe".into())
}

/// The exports of `exe` (a copy of ORST.exe), for the import library
fn def_text(exe: &str) -> String {
    let mut text = format!("NAME \"{exe}\"\nEXPORTS\n");
    for name in EXPORTS {
        text += &format!("    {name}\n");
    }
    text
}

const MAIN: &str = "// Written by ORST Studio: the entry points of the scripts library.\n\
                    #define ORST_SCRIPT_MAIN\n\
                    #include \"orst_script.hpp\"\n";

pub fn write_project(project: &ScriptProject) -> Result<(), String> {
    let dir = build_dir(project);
    write_if_changed(&dir.join("CMakeLists.txt"), &cmake_text(project))?;
    write_if_changed(&dir.join("main.cpp"), MAIN)?;
    write_if_changed(&dir.join("host.def"), &def_text(&exe_name()))
}

/// Configure once, then build; whether it worked, and the output
pub fn build(project: &ScriptProject, log: &Path) -> Result<(bool, String), String> {
    let dir = build_dir(project);
    run_cmake(&dir, &dir.join("cmake"), &[], log)
}

/// The scripts again for a built game, whose exe is `exe`, into `out`
/// (on Windows the library names the exe it takes the engine from)
pub fn build_for_game(
    project: &ScriptProject,
    exe: &str,
    out: &Path,
    log: &Path,
) -> Result<(bool, String), String> {
    write_project(project)?;
    let dir = build_dir(project);
    let def = dir.join("game.def");
    write_if_changed(&def, &def_text(exe))?;
    let options = [
        format!("-DORST_OUT={}", cmake_path(out)),
        format!("-DORST_HOST_DEF={}", cmake_path(&def)),
    ];
    run_cmake(&dir, &dir.join("cmake-game"), &options, log)
}

fn run_cmake(
    dir: &Path,
    cmake: &Path,
    options: &[String],
    log: &Path,
) -> Result<(bool, String), String> {
    let missing =
        |e: std::io::Error| format!("CMake did not run ({e}); install CMake and a C++ compiler");
    // Options change with the game's folder, so they are passed again
    if !cmake.join("CMakeCache.txt").is_file() || !options.is_empty() {
        let (ok, text) = run_logged(
            Command::new("cmake")
                .arg("-S")
                .arg(dir)
                .arg("-B")
                .arg(cmake)
                .arg(format!("-DCMAKE_BUILD_TYPE={CONFIG}"))
                .args(options),
            log,
        )
        .map_err(missing)?;
        if !ok {
            // Configure again next time, with whatever was fixed
            let _ = std::fs::remove_file(cmake.join("CMakeCache.txt"));
            return Ok((false, text));
        }
    }
    run_logged(
        Command::new("cmake")
            .arg("--build")
            .arg(cmake)
            .args(["--config", CONFIG]),
        log,
    )
    .map_err(missing)
}

type Attach = unsafe extern "C" fn(*mut OrstEngine, LogFn) -> i32;
type LogFn = extern "C" fn(u32, *const u8, usize);

extern "C" fn log_c(level: u32, text: *const u8, len: usize) {
    log(level, text, len)
}

/// The library loaded now and its entry points
struct Loaded {
    define: unsafe extern "C" fn() -> i32,
    start: unsafe extern "C" fn(),
    tick: unsafe extern "C" fn(f32) -> i32,
    stop: unsafe extern "C" fn(),
    detach: unsafe extern "C" fn(),
    error: unsafe extern "C" fn() -> *const u8,
    path: PathBuf,
    // Last, so the functions above go first
    _library: libloading::Library,
}

/// C++ scripts on the editor's engine
pub struct NativeScripts {
    engine: *mut OrstEngine,
    /// Where the copies of builds are loaded from
    dir: PathBuf,
    loaded: Option<Loaded>,
    copies: u32,
}

impl NativeScripts {
    /// Scripts for `engine`, which must stay where it is while they run;
    /// copies of the builds go in `dir`
    pub fn new(engine: &mut OrstEngine, dir: PathBuf) -> NativeScripts {
        // What earlier sessions left there
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        NativeScripts {
            engine: engine.handle(),
            dir,
            loaded: None,
            copies: 0,
        }
    }
}

impl ScriptHost for NativeScripts {
    fn load(&mut self, built: &Path) -> Result<u32, String> {
        self.unload();
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        self.copies += 1;
        let name = format!(
            "Scripts{}-{}.{}",
            std::process::id(),
            self.copies,
            std::env::consts::DLL_EXTENSION
        );
        let path = self.dir.join(name);
        std::fs::copy(built, &path).map_err(|e| format!("{}: {e}", built.display()))?;
        // SAFETY: the library is a build of the project's scripts; its
        // initialisers only register Behaviours
        let library = unsafe { libloading::Library::new(&path) }
            .map_err(|e| format!("{}: {e}", built.display()))?;
        macro_rules! get {
            ($name:literal) => {
                // SAFETY: orst_script.hpp defines each with this type
                *unsafe { library.get($name) }
                    .map_err(|e| format!("not a scripts library ({e})"))?
            };
        }
        let attach: Attach = get!(b"orst_script_attach");
        let loaded = Loaded {
            define: get!(b"orst_script_define"),
            start: get!(b"orst_script_start"),
            tick: get!(b"orst_script_tick"),
            stop: get!(b"orst_script_stop"),
            detach: get!(b"orst_script_detach"),
            error: get!(b"orst_script_error"),
            path,
            _library: library,
        };
        // SAFETY: the engine outlives the scripts (see new)
        unsafe { attach(self.engine, log_c) };
        let n = unsafe { (loaded.define)() };
        let error = || {
            let p = unsafe { (loaded.error)() };
            match p.is_null() {
                true => "unknown error".to_string(),
                // SAFETY: a NUL-terminated string the library keeps
                false => unsafe { CStr::from_ptr(p.cast()) }.to_string_lossy().into(),
            }
        };
        let result = u32::try_from(n).map_err(|_| error());
        self.loaded = Some(loaded);
        if result.is_err() {
            self.unload();
        }
        result
    }

    fn unload(&mut self) -> bool {
        if let Some(loaded) = self.loaded.take() {
            unsafe { (loaded.detach)() };
            let path = loaded.path.clone();
            drop(loaded);
            let _ = std::fs::remove_file(path);
        }
        true
    }

    fn begin_play(&mut self) {
        if let Some(l) = &self.loaded {
            unsafe { (l.start)() }
        }
    }

    fn tick(&mut self, dt: f32) -> u32 {
        match &self.loaded {
            Some(l) => unsafe { (l.tick)(dt) }.max(0) as u32,
            None => 0,
        }
    }

    fn end_play(&mut self) {
        if let Some(l) = &self.loaded {
            unsafe { (l.stop)() }
        }
    }
}

impl Drop for NativeScripts {
    fn drop(&mut self) {
        self.unload();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::Level;
    use crate::project::Language;
    use crate::scripts::take_log;
    use crate::scripts::tests::{build, DOTNET};
    use orst::components::{read, write, Collider, Transform, COLLIDER, TRANSFORM};
    use orst::schema::Value;
    use orst::{Engine, Vec2};

    /// Whether CMake is here; the test skips without it, unless
    /// ORST_REQUIRE_CMAKE is set (CI)
    fn have_cmake() -> bool {
        let ok = Command::new("cmake")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            assert!(std::env::var_os("ORST_REQUIRE_CMAKE").is_none(), "no CMake");
            eprintln!("skipped: no CMake");
        }
        ok
    }

    const FIRST: &str = r#"
#include "orst_script.hpp"
using namespace orst;
enum class Mode : int32_t { Walk, Run };
struct Mover : Behaviour {
    float speed = 3;
    Mode mode = Mode::Run;
    int32_t starts = 0;
    int32_t hits = 0;
    static void fields(Fields<Mover> &f) {
        f("Speed", &Mover::speed);
        f("Mode", &Mover::mode).options("Walk|Run");
        f("Starts", &Mover::starts);
        f("Hits", &Mover::hits);
    }
    void start() override { starts++; log("started"); }
    void on_contact_begin(Entity, const Contact &c) override { if (c.trigger) hits++; }
    void update(float) override { transform().pos.x += speed; }
};
ORST_BEHAVIOUR(Mover);
"#;

    const SECOND: &str = r#"
#include "orst_script.hpp"
#include <stdexcept>
using namespace orst;
struct Mover : Behaviour {
    float speed = 1;
    float jump = 7;
    static void fields(Fields<Mover> &f) { f("Speed", &Mover::speed); f("Jump", &Mover::jump); }
    void update(float) override { throw std::runtime_error("boom"); }
};
ORST_BEHAVIOUR(Mover);
"#;

    #[test]
    fn cpp_scripts_run_on_the_editors_engine_and_reload_in_place() {
        let _turn = DOTNET.lock().unwrap_or_else(|e| e.into_inner());
        if !have_cmake() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("orst-native-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut engine = OrstEngine::new(Engine::headless(16, 16).unwrap());
        let mut scripts = NativeScripts::new(&mut engine, dir.join("Library/Scripts/loaded"));
        let project = ScriptProject {
            root: dir.clone(),
            language: Language::Cpp,
            host: include_dir().unwrap(),
        };
        let built = build(&project, FIRST);
        assert!(built.ok, "{:?}", built.messages);
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
        let zone = world.spawn();
        let at = Transform {
            pos: Vec2::new(12.0, 0.0),
            ..Default::default()
        };
        write(world, zone, TRANSFORM, &at);
        let trigger = Collider {
            trigger: 1,
            ..shape
        };
        write(world, zone, COLLIDER, &trigger);
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
        assert_eq!(t.pos.x, 6.0, "update() moved the entity twice");
        assert_eq!(engine.world.field(e, id, "Starts"), Some(Value::I32(1)));
        assert_eq!(engine.world.field(e, id, "Hits"), Some(Value::I32(1)));
        let log = take_log();
        assert!(log.iter().any(|(_, l)| l == "started"), "{log:?}");

        // A new build over the loaded one: Speed keeps its value, Jump is new
        engine.world.set_field(e, id, "Speed", Value::F32(5.0));
        let built = build(&project, SECOND);
        assert!(built.ok, "{:?}", built.messages);
        assert_eq!(scripts.load(&project.library()), Ok(1));
        assert_eq!(engine.world.component_id("Mover"), Some(id));
        assert_eq!(engine.world.field(e, id, "Speed"), Some(Value::F32(5.0)));
        assert_eq!(engine.world.field(e, id, "Jump"), Some(Value::F32(7.0)));
        scripts.begin_play();
        assert_eq!(scripts.tick(1.0 / 60.0), 1, "the exception is counted");
        scripts.end_play();
        let log = take_log();
        assert!(
            log.iter()
                .any(|(l, t)| *l == Level::Error && t.contains("boom")),
            "{log:?}"
        );

        let broken = build(&project, "struct Mover { int x = };");
        assert!(!broken.ok);
        let (level, first) = &broken.messages[0];
        assert!(*level == Level::Error, "{first}");
        assert!(first.contains("Mover.cpp"), "{first}");
        assert!(scripts.load(&dir.join("missing.so")).is_err());
        scripts.unload();
        let loaded = std::fs::read_dir(dir.join("Library/Scripts/loaded")).unwrap();
        assert_eq!(loaded.count(), 0, "copies go once unloaded");

        // What a new project starts with builds
        let root = dir.join("new");
        crate::project::Project::create(&root, "New", Language::Cpp).unwrap();
        let fresh = ScriptProject {
            root,
            language: Language::Cpp,
            host: include_dir().unwrap(),
        };
        let built = fresh.build();
        assert!(
            built.ok && built.messages.is_empty(),
            "{:?}",
            built.messages
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
