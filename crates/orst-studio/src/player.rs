//! A built game. File → Build puts a copy of ORST.exe in a folder with the
//! game in Data/; started from there, the exe runs the game in its own
//! window instead of the editor:
//!
//! ```text
//! MyGame/
//!   MyGame.exe     ORST.exe under the game's name
//!   Scripting/     the C# script host (C# games)
//!   Data/
//!     project.orst
//!     Assets/      what the game uses, without the script sources
//!     Scripts.dll  the scripts, built
//! ```

use crate::console::Level;
use crate::history::History;
use crate::native::NativeScripts;
use crate::play::Play;
use crate::project::{Language, ProjectFile, ASSETS, LOCALE, PROJECT_FILE};
use crate::scripts::{take_log, ScriptHost, Scripts};
use orst::asset_db::AssetDb;
use orst::ffi::OrstEngine;
use orst::window::WindowConfig;
use orst::{Color, Engine};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The game's folder next to the exe
pub const DATA: &str = "Data";

/// The size the game draws at; the window scales it
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

/// Data/ next to the running exe, when it holds a game
pub fn data_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let data = exe.parent()?.join(DATA);
    data.join(PROJECT_FILE).is_file().then_some(data)
}

/// The file name of the built scripts library for `language`
pub fn library_name(language: Language) -> String {
    match language {
        Language::CSharp => "Scripts.dll".into(),
        Language::Cpp => format!("Scripts.{}", std::env::consts::DLL_EXTENSION),
    }
}

pub fn read_project(data: &Path) -> Result<ProjectFile, String> {
    let path = data.join(PROJECT_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// A game running: its world, scripts and the fixed ticks
pub struct Game {
    pub engine: Box<OrstEngine>,
    scripts: Option<Box<dyn ScriptHost>>,
    play: Play,
    _assets: AssetDb,
}

impl Game {
    /// The game in `data` on `engine`, at its first scene with the
    /// scripts started
    pub fn load(engine: Engine, data: &Path) -> Result<Game, String> {
        let file = read_project(data)?;
        let mut engine = OrstEngine::new(engine);
        let library = data.join(library_name(file.language));
        // Scripts go first: the scene holds their components
        let mut scripts: Option<Box<dyn ScriptHost>> = None;
        if library.is_file() {
            let mut host: Box<dyn ScriptHost> = match file.language {
                Language::CSharp => {
                    let dir = Scripts::host_dir()
                        .ok_or("the C# script host is missing: Scripting next to the exe")?;
                    Box::new(Scripts::start(&dir, &mut engine)?)
                }
                Language::Cpp => {
                    let dir =
                        std::env::temp_dir().join(format!("orst-game-{}", std::process::id()));
                    Box::new(NativeScripts::new(&mut engine, dir))
                }
            };
            host.load(&library)
                .map_err(|e| format!("the scripts did not load: {e}"))?;
            scripts = Some(host);
        }
        let (mut assets, _) = AssetDb::scan(data.join(ASSETS))?;
        engine.assets_dir = Some(assets.root.clone());
        if let Some(dir) = orst::saves::user_dir(&file.name) {
            engine.set_save_dir(dir);
        }
        for error in engine.locale.load_dir(&assets.file(LOCALE)) {
            crate::scripts::log(1, error.as_ptr(), error.len());
        }
        let main = file.main_scene.ok_or("the game has no first scene")?;
        let path = assets.file(&main);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{main}: {e}"))?;
        let loaded = engine.load_scene(&text, &mut assets)?;
        for warning in loaded.warnings {
            crate::scripts::log(1, warning.as_ptr(), warning.len());
        }
        let play = Play::start(&mut engine, History::default(), false, None);
        if let Some(s) = &mut scripts {
            s.begin_play();
        }
        Ok(Game {
            engine,
            scripts,
            play,
            _assets: assets,
        })
    }

    /// Run the ticks `dt` seconds of real time are worth
    pub fn advance(&mut self, dt: f32) -> u32 {
        let scripts = self.scripts.as_deref_mut();
        self.play
            .advance(&mut self.engine, scripts.map(|s| s as _), dt)
    }

    pub fn draw(&mut self) {
        self.engine.set_camera(self.play.camera);
        self.engine.begin_frame(Color::BLACK);
        self.engine.draw_entities();
        self.engine.end_frame();
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        if let Some(s) = &mut self.scripts {
            s.end_play();
            s.unload();
        }
    }
}

/// Run the game in `data` in a window until it is closed. What the
/// scripts log goes to log.txt beside Data/.
pub fn run(data: &Path) -> Result<(), String> {
    let mut log = Log::open(data);
    let result: Result<(), String> = (|| {
        let file = read_project(data)?;
        let engine = Engine::windowed(WindowConfig {
            title: file.name.clone(),
            width: WIDTH,
            height: HEIGHT,
            ..Default::default()
        })?;
        let mut game = Game::load(engine, data)?;
        log.flush();
        while game.engine.poll() {
            let dt = game.engine.time.dt;
            game.advance(dt);
            game.draw();
            log.flush();
        }
        Ok(())
    })();
    log.flush();
    if let Err(e) = &result {
        log.write(Level::Error, e);
    }
    result
}

/// log.txt of a built game
struct Log(Option<std::fs::File>);

impl Log {
    fn open(data: &Path) -> Log {
        let path = data.parent().unwrap_or(data).join("log.txt");
        Log(std::fs::File::create(path).ok())
    }

    fn write(&mut self, level: Level, text: &str) {
        let tag = match level {
            Level::Info => "",
            Level::Warning => "warning: ",
            Level::Error => "error: ",
        };
        if let Some(f) = &mut self.0 {
            let _ = writeln!(f, "{tag}{text}");
        }
        eprintln!("{tag}{text}");
    }

    fn flush(&mut self) {
        for (level, text) in take_log() {
            self.write(level, &text);
        }
    }
}
