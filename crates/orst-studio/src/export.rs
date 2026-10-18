//! File → Build: the game as a folder to give away. The exe is a copy of
//! the running ORST.exe named after the game, which runs the game when it
//! finds Data/ beside it (see `player`).

use crate::console::Level;
use crate::player::{library_name, read_project, DATA};
use crate::project::{Language, ASSETS, PROJECT_FILE};
use crate::scripts::{build_messages, ScriptProject, Scripts};
use std::path::{Path, PathBuf};

/// Where a build goes unless the user says otherwise
pub const DEFAULT_DIR: &str = "Build";

/// The game's file name from its name: letters, digits, spaces, - and _
pub fn exe_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .collect();
    let clean = clean.trim();
    let stem = if clean.is_empty() { "Game" } else { clean };
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// Build the game of the project in `root` into `out`: the scripts, the
/// assets without the script sources, and the exe. Returns the exe, or
/// what went wrong (compiler errors included).
pub fn export(root: &Path, out: &Path) -> Result<PathBuf, Vec<(Level, String)>> {
    let fail = |e: String| vec![(Level::Error, e)];
    let file = read_project(root).map_err(fail)?;
    if file.main_scene.is_none() {
        return Err(fail(
            "Save a scene first: the game starts with the first scene saved".into(),
        ));
    }
    let absolute = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let (out, root_abs) = (absolute(out), absolute(root));
    if out == root_abs || out.starts_with(root_abs.join(ASSETS)) {
        return Err(fail(format!(
            "{} can't hold the build: choose a folder outside Assets",
            out.display()
        )));
    }
    let earlier = out.join(DATA).join(PROJECT_FILE).is_file();
    let busy = std::fs::read_dir(&out).is_ok_and(|mut d| d.next().is_some());
    if busy && !earlier {
        return Err(fail(format!(
            "{} is not empty: choose an empty folder or an earlier build",
            out.display()
        )));
    }
    let io = |what: &Path| {
        let what = what.display().to_string();
        move |e: std::io::Error| vec![(Level::Error, format!("{what}: {e}"))]
    };
    let data = out.join(DATA);
    let scripting = out.join("Scripting");
    for dir in [&data, &scripting] {
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(io(dir))?;
        }
    }
    std::fs::create_dir_all(&data).map_err(io(&data))?;
    let exe = out.join(exe_name(&file.name));

    // The scripts, built for the game's exe
    let host = match file.language {
        Language::CSharp => Scripts::host_dir(),
        Language::Cpp => crate::native::include_dir(),
    };
    let code = host.map(|host| ScriptProject {
        root: root.to_path_buf(),
        language: file.language,
        host,
    });
    if count_sources(root, file.language) > 0 {
        let Some(code) = code else {
            return Err(fail(match file.language {
                Language::CSharp => "ORST.Scripting was not found next to ORST.exe".into(),
                Language::Cpp => "orst_script.hpp was not found next to ORST.exe".into(),
            }));
        };
        match file.language {
            Language::CSharp => {
                let built = code.build();
                if !built.ok {
                    return Err(built.messages);
                }
                let to = data.join(library_name(Language::CSharp));
                std::fs::copy(code.library(), &to).map_err(io(&to))?;
                copy_dir(&code.host, &scripting, &|_| true).map_err(io(&scripting))?;
            }
            Language::Cpp => {
                let log = root.join("Library/Scripts/game.log");
                let name = exe.file_name().unwrap().to_string_lossy().into_owned();
                let (ok, text) =
                    crate::native::build_for_game(&code, &name, &data, &log).map_err(fail)?;
                if !ok {
                    let mut messages = build_messages(&text, root);
                    if messages.is_empty() {
                        messages.push((Level::Error, text.trim().to_string()));
                    }
                    return Err(messages);
                }
            }
        }
    }

    // What the game uses, without the script sources
    let assets = root.join(ASSETS);
    let keep = |p: &Path| {
        let name = p.to_string_lossy();
        let source = name.strip_suffix(".meta").unwrap_or(&name);
        !is_source(Path::new(source), file.language)
    };
    copy_dir(&assets, &data.join(ASSETS), &keep).map_err(io(&assets))?;
    let project = data.join(PROJECT_FILE);
    std::fs::copy(root.join(PROJECT_FILE), &project).map_err(io(&project))?;
    let me = std::env::current_exe().map_err(|e| fail(e.to_string()))?;
    std::fs::copy(&me, &exe).map_err(io(&exe))?;
    Ok(exe)
}

fn is_source(path: &Path, language: Language) -> bool {
    let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("");
    match language {
        Language::CSharp => ext == "cs",
        Language::Cpp => matches!(ext, "cpp" | "cc" | "cxx" | "hpp" | "h"),
    }
}

fn count_sources(root: &Path, language: Language) -> usize {
    fn walk(dir: &Path, language: Language) -> usize {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return 0;
        };
        entries
            .flatten()
            .map(|e| match e.path() {
                p if p.is_dir() => walk(&p, language),
                p => is_source(&p, language) as usize,
            })
            .sum()
    }
    walk(&root.join(ASSETS), language)
}

/// Copy the files of `from` that `keep` lets through into `to`
fn copy_dir(from: &Path, to: &Path, keep: &dyn Fn(&Path) -> bool) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            copy_dir(&path, &to.join(entry.file_name()), keep)?;
        } else if keep(&path) {
            std::fs::copy(&path, to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::TICK;
    use crate::player::{Game, HEIGHT, WIDTH};
    use crate::scripts::take_log;
    use crate::scripts::tests::{copy_dir as copy_sample, host, DOTNET};
    use orst::components::{read, Name, Transform, NAME, TRANSFORM};
    use orst::input::Key;
    use orst::Engine;

    #[test]
    fn games_are_named_after_their_project() {
        let exe = std::env::consts::EXE_SUFFIX;
        assert_eq!(
            exe_name(" Coins: the game! "),
            format!("Coins the game{exe}")
        );
        assert_eq!(exe_name("../.."), format!("Game{exe}"));
    }

    /// Coins, built, plays from its Data folder as it does in the editor
    #[test]
    fn the_coins_sample_builds_into_a_game() {
        let _turn = DOTNET.lock().unwrap_or_else(|e| e.into_inner());
        if host().is_none() {
            return;
        }
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/studio/Coins");
        let root = std::env::temp_dir().join(format!("orst-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        copy_sample(&sample, &root);
        // As the editor opens it: the .meta files come back
        crate::project::Project::open(&root).unwrap();
        let out = root.join(DEFAULT_DIR);

        let busy = root.join("busy");
        std::fs::create_dir_all(&busy).unwrap();
        std::fs::write(busy.join("notes.txt"), "mine").unwrap();
        assert!(
            export(&root, &busy).is_err(),
            "a folder that holds other files"
        );
        assert!(export(&root, &root.join("Assets/out")).is_err());

        let exe = export(&root, &out).unwrap();
        // Again over the first build
        assert_eq!(export(&root, &out).unwrap(), exe);
        assert!(exe.is_file() && exe.parent() == Some(out.as_path()));
        let data = out.join(DATA);
        assert!(data.join("Scripts.dll").is_file());
        assert!(out.join("Scripting/ORST.Scripting.dll").is_file());
        assert!(data.join("Assets/Main.scene").is_file());
        assert!(data.join("Assets/art/coin.png.meta").is_file());
        assert!(
            !data.join("Assets/Scripts/Coin.cs").exists()
                && !data.join("Assets/Scripts/Coin.cs.meta").exists(),
            "no script sources in the game"
        );

        take_log();
        let mut game = Game::load(Engine::headless(WIDTH, HEIGHT).unwrap(), &data).unwrap();
        let player = game.engine.world.find_named("Player").unwrap();
        game.engine.input.set_key(Key::Up, true);
        for _ in 0..120 {
            game.advance(TICK);
        }
        game.draw();
        let log: Vec<String> = take_log().into_iter().map(|(_, t)| t).collect();
        assert!(log.iter().any(|l| l == "Coins: 1/8"), "{log:?}");
        let t: Transform = read(&game.engine.world, player, TRANSFORM).unwrap();
        assert!((-91.0..-85.0).contains(&t.pos.y), "{:?}", t.pos);
        drop(game);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A new C++ project builds into a game whose scripts run
    #[test]
    fn cpp_projects_build_into_games() {
        let _turn = DOTNET.lock().unwrap_or_else(|e| e.into_inner());
        let cmake = std::process::Command::new("cmake")
            .arg("--version")
            .output();
        if !cmake.is_ok_and(|o| o.status.success()) {
            assert!(std::env::var_os("ORST_REQUIRE_CMAKE").is_none(), "no CMake");
            return;
        }
        let root = std::env::temp_dir().join(format!("orst-export-cpp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut project = crate::project::Project::create(&root, "Walker", Language::Cpp).unwrap();
        // A scene with one entity that walks
        let mut engine = orst::ffi::OrstEngine::new(Engine::headless(64, 64).unwrap());
        let world = &mut engine.world;
        let e = world.spawn();
        orst::components::write(world, e, NAME, &Name::new("Hero"));
        orst::components::write(world, e, TRANSFORM, &Transform::default());
        let (text, _) = engine.save_scene(&[e], &project.assets);
        std::fs::write(root.join("Assets/Main.scene"), text).unwrap();
        project.file.main_scene = Some("Main.scene".into());
        crate::project::write_project_file(&root, &project.file).unwrap();
        project.rescan().unwrap();

        let out = root.join(DEFAULT_DIR);
        let exe = export(&root, &out).unwrap();
        assert_eq!(exe, out.join(exe_name("Walker")));
        let data = out.join(DATA);
        assert!(data.join(library_name(Language::Cpp)).is_file());
        assert!(!data.join("Assets/Scripts/PlayerMove.cpp").exists());
        // On Windows the library takes the engine from Walker.exe, which is
        // not this test
        if cfg!(windows) {
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let mut game = Game::load(Engine::headless(WIDTH, HEIGHT).unwrap(), &data).unwrap();
        let hero = game.engine.world.find_named("Hero").unwrap();
        let id = game.engine.world.component_id("PlayerMove").unwrap();
        let defaults = game.engine.world.default_value(id);
        game.engine.world.set(hero, id, &defaults);
        game.engine.input.set_key(Key::Right, true);
        for _ in 0..60 {
            game.advance(TICK);
        }
        let t: Transform = read(&game.engine.world, hero, TRANSFORM).unwrap();
        assert!(
            (t.pos.x - 120.0).abs() < 1.0,
            "a second at 120: {:?}",
            t.pos
        );
        drop(game);
        let _ = std::fs::remove_dir_all(&root);
    }
}
