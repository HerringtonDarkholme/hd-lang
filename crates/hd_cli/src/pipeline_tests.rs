//! Each command that builds a program to run tunes it for the engine that
//! runs it (`crates/hd_opt/DESIGN.md` (optimization pipelines) §5): `--engine NAME`, else `HD_ENGINE`, else
//! Node, at the level `--release` selects. These tests run the commands
//! themselves and read the pipeline the driver reports for each build. A
//! program with a type error stops each command after its build, before
//! anything runs. The engine is always named, so `HD_ENGINE` in the test
//! environment changes nothing.

use std::ffi::OsString;
use std::path::Path;

use hd_driver::{EngineTarget, OptLevel};
use hd_testkit::Scratch;

use crate::{Pipeline, take_built};

const SCRATCH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/tmp");

/// A program that builds to a type error.
const BROKEN: &str = "pub fn main() -> void:\n    nope()\n";

fn args(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

fn pipeline(target: EngineTarget, level: OptLevel) -> Pipeline {
    Pipeline { target, level }
}

/// A package `shop` with `files`, below the scratch directory `name`.
fn package(name: &str, files: &[(&str, &str)]) -> Scratch {
    let dir = Scratch::new(SCRATCH, name);
    let mut all = vec![("hd.toml", "[package]\nname = \"shop\"\n")];
    all.extend_from_slice(files);
    for (path, text) in all {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("directory");
        std::fs::write(path, text).expect("file");
    }
    dir
}

/// The engines a test names, each with the target of its builds: Node,
/// and wasmtime in a build with the feature.
fn engines() -> Vec<(&'static str, EngineTarget)> {
    let mut engines = vec![("node", EngineTarget::V8)];
    if cfg!(feature = "wasmtime") {
        engines.push(("wasmtime", EngineTarget::Wasmtime));
    }
    engines
}

/// Runs `command` once per engine and level, and checks that it built
/// `builds` programs, each with that engine's target and that level.
fn check(builds: usize, command: &dyn Fn(&[&str])) {
    for (engine, target) in engines() {
        for (release, level) in [(false, OptLevel::Dev), (true, OptLevel::Optimized)] {
            let mut flags = vec!["--engine", engine];
            if release {
                flags.push("--release");
            }
            take_built();
            command(&flags);
            assert_eq!(
                take_built(),
                vec![pipeline(target, level); builds],
                "{flags:?}"
            );
        }
    }
}

fn path(dir: &Path, file: &str) -> String {
    dir.join(file).to_string_lossy().into_owned()
}

/// `hd FILE`.
#[test]
fn a_file_builds_for_its_engine() {
    let dir = Scratch::new(SCRATCH, "pipeline-file");
    let file = path(&dir, "broken.hd");
    std::fs::write(&file, BROKEN).expect("file");
    check(1, &|flags| {
        let mut words = args(flags);
        words.push(file.clone().into());
        crate::run_file_command(&words);
    });
}

/// `hd run`.
#[test]
fn a_run_builds_for_its_engine() {
    let dir = package("pipeline-run", &[("src/main.hd", BROKEN)]);
    check(1, &|flags| {
        crate::run_in(&dir, &args(flags));
    });
}

/// `hd build`, which takes `--engine` as `hd run` does.
#[test]
fn a_build_is_for_the_engine_hd_run_would_choose() {
    let dir = package("pipeline-build", &[("src/main.hd", BROKEN)]);
    let file = path(&dir, "src/main.hd");
    check(1, &|flags| {
        let mut words = args(flags);
        words.push(file.clone().into());
        crate::build_command(&words);
    });
    assert!(
        !dir.join("build").exists(),
        "a build with an error writes no module"
    );
}

/// `hd build --engine wasmtime` in a build without the feature: refused
/// with the message `hd run --engine wasmtime` gives.
#[cfg(not(feature = "wasmtime"))]
#[test]
fn hd_build_refuses_wasmtime_without_the_feature() {
    let words = args(&["--engine", "wasmtime"]);
    let Err(build) = crate::words("build", &words) else {
        panic!("hd build accepted the missing engine");
    };
    let Err(run) = crate::words("run", &words) else {
        panic!("hd run accepted the missing engine");
    };
    assert_eq!(build, run);
    assert!(build.contains("no wasmtime engine"), "{build}");
}

/// `hd build --engine wasmtime` in a build with the feature.
#[cfg(feature = "wasmtime")]
#[test]
fn hd_build_accepts_wasmtime_with_the_feature() {
    let w = crate::words("build", &args(&["--engine", "wasmtime"])).expect("accepted");
    assert_eq!(w.engine, crate::engine::EngineKind::Wasmtime);
}

/// `hd test`: its test programs, and the executables and tasks an
/// integration test starts (`cli.test.builds-executables`), which run on
/// the same engine. The task does not build, which ends the command.
#[test]
fn a_test_run_and_its_executables_build_for_its_engine() {
    let dir = package(
        "pipeline-test",
        &[
            // Let the integration program reach the deliberately broken task.
            (
                "hd.toml",
                "[package]\nname = \"shop\"\n[dev-capabilities]\nProcess = true\n",
            ),
            ("tasks/seed.hd", BROKEN),
            (
                "tests/seed.hd",
                "use std.process.Command\nuse std.testing.{assert_equal, hd_run}\n\n\
                 it(\"runs the seed task\"):\n    \
                 out := hd_run!(*Command::new(\"seed\"))\n    \
                 assert_equal(out.status, 0, reason=\"a clean exit\")\n",
            ),
        ],
    );
    let file = path(&dir, "tests/seed.hd");
    check(2, &|flags| {
        let mut words = args(flags);
        words.push(file.clone().into());
        crate::test_cmd::command(&words);
    });
}
