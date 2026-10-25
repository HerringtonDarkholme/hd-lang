//! Installed executables: immutable release Wasm, a versioned registry,
//! manifest grants and a launcher which selects hd from PATH (#823).

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use hd_deps::{Cache, Remote, Source as _};
use hd_project::Grant;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{Compiled, Pipeline, caps, disk, engine, fail, report::Reporter};

const FORMAT: u32 = 1;

fn io(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str, String> {
    value[name]
        .as_str()
        .ok_or_else(|| format!("invalid installation record: {name}"))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| c.is_control() || "/\\<>:\"|?*".contains(c))
        && !name.ends_with('.')
        && !name.eq_ignore_ascii_case("hd")
        && !matches!(
            name.split('.')
                .next()
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str(),
            "CON"
                | "PRN"
                | "AUX"
                | "NUL"
                | "COM1"
                | "COM2"
                | "COM3"
                | "COM4"
                | "COM5"
                | "COM6"
                | "COM7"
                | "COM8"
                | "COM9"
                | "LPT1"
                | "LPT2"
                | "LPT3"
                | "LPT4"
                | "LPT5"
                | "LPT6"
                | "LPT7"
                | "LPT8"
                | "LPT9"
        )
}

#[derive(Default)]
struct Options {
    root: Option<PathBuf>,
    operand: Option<String>,
    deny: Vec<String>,
    list: bool,
}

fn options(args: &[OsString], remove: bool) -> Result<Options, String> {
    let mut result = Options::default();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            if result.operand.is_some() {
                return Err("the command takes one operand".into());
            }
            result.operand = Some(
                words
                    .next()
                    .ok_or("missing operand after --")?
                    .to_string_lossy()
                    .into_owned(),
            );
            if words.next().is_some() {
                return Err("the command takes one operand".into());
            }
            break;
        }
        match word.to_str() {
            Some("--root") => {
                if result.root.is_some() {
                    return Err("--root is repeated".into());
                }
                result.root = Some(words.next().ok_or("--root needs a directory")?.into());
            }
            Some("--deny") if !remove => {
                let key = words
                    .next()
                    .ok_or("--deny needs a capability")?
                    .to_string_lossy()
                    .into_owned();
                if let Some(error) = hd_project::grant_problem(&key, &Grant::Deny) {
                    return Err(error);
                }
                result.deny.push(key);
            }
            Some("--list") if !remove => result.list = true,
            Some(text) if !text.starts_with('-') && result.operand.is_none() => {
                result.operand = Some(text.into());
            }
            _ => {
                return Err(format!(
                    "unexpected install argument `{}`",
                    word.to_string_lossy()
                ));
            }
        }
    }
    if result.list {
        if result.operand.is_some() || !result.deny.is_empty() {
            return Err("--list takes no source or --deny".into());
        }
    } else if result.operand.is_none() {
        return Err(if remove {
            "hd uninstall needs one executable name"
        } else {
            "hd install needs one source"
        }
        .into());
    }
    Ok(result)
}

fn root(option: Option<PathBuf>) -> Result<PathBuf, String> {
    let root = option
        .or_else(|| std::env::var_os("HD_INSTALL_ROOT").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|h| PathBuf::from(h).join(".hd"))
        })
        .ok_or("cannot find an installation root; use --root DIR")?;
    std::path::absolute(root).map_err(io)
}

fn lock(root: &Path) -> Result<File, String> {
    fs::create_dir_all(root.join("installed")).map_err(io)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("installed/.lock"))
        .map_err(io)?;
    file.lock().map_err(io)?;
    Ok(file)
}

fn registry(root: &Path) -> Result<Value, String> {
    let value: Value = match fs::read(root.join("installed/registry.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("invalid installation registry: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            json!({"format":FORMAT,"commands":{}})
        }
        Err(e) => return Err(io(e)),
    };
    if value["format"] != FORMAT || !value["commands"].is_object() {
        return Err(
            "unsupported installation registry format; reinstall with a compatible hd".into(),
        );
    }
    for (name, record) in value["commands"].as_object().expect("checked registry") {
        if !valid_name(name) || record["name"].as_str() != Some(name.as_str()) {
            return Err("invalid installed command name in registry".into());
        }
        payload(root, record)?;
    }
    Ok(value)
}

fn save(root: &Path, value: &Value) -> Result<(), String> {
    let pending = root.join(format!("installed/.registry.{}.tmp", std::process::id()));
    fs::write(
        &pending,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(io)?;
    let result = fs::rename(&pending, root.join("installed/registry.json")).map_err(io);
    if result.is_err() {
        let _ = fs::remove_file(pending);
    }
    result
}

fn bin(root: &Path, name: &str) -> PathBuf {
    root.join("bin").join(if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.into()
    })
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn launcher(root: &Path, name: &str) -> Vec<u8> {
    if cfg!(windows) {
        let root = root.to_string_lossy().replace('%', "%%");
        let name = name.replace('%', "%%");
        format!("@echo off\r\nsetlocal DisableDelayedExpansion\r\nfor %%I in (hd.exe) do set \"HD_INSTALL_RUNTIME=%%~$PATH:I\"\r\nif not defined HD_INSTALL_RUNTIME exit /b 101\r\n\"%HD_INSTALL_RUNTIME%\" --installed \"{root}\" \"{name}\" -- %*\r\nexit /b %errorlevel%\r\n").into_bytes()
    } else {
        format!(
            "#!/bin/sh\nexec hd --installed {} {} -- \"$@\"\n",
            quote(&root.to_string_lossy()),
            quote(name)
        )
        .into_bytes()
    }
}

fn check_launcher(root: &Path, name: &str, managed: bool) -> Result<(), String> {
    let path = bin(root, name);
    match fs::symlink_metadata(&path) {
        Ok(metadata)
            if managed
                && metadata.is_file()
                && fs::read(&path).map_err(io)? == launcher(root, name) =>
        {
            Ok(())
        }
        Ok(_) => Err(format!(
            "refusing to replace unrelated or modified command {}",
            path.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn write_launcher(root: &Path, name: &str) -> Result<(), String> {
    use std::io::Write as _;
    let path = bin(root, name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(io)?;
    let result = (|| -> Result<(), String> {
        file.write_all(&launcher(root, name)).map_err(io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(fs::Permissions::from_mode(0o755))
                .map_err(io)?;
        }
        Ok(())
    })();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result
}

fn remove_launcher(root: &Path, name: &str) -> Result<(), String> {
    match fs::remove_file(bin(root, name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(e)),
    }
}

struct Source {
    directory: PathBuf,
    owner: String,
    revision: String,
    reinstall: String,
    selection: Option<(hd_deps::Selection, usize)>,
}

fn source(word: &str) -> Result<Source, String> {
    let target = hd_deps::target::parse_target(word)?;
    match target.form {
        hd_deps::target::TargetForm::Path(path) => {
            let directory = hd_run::canonicalize(path).map_err(io)?;
            Ok(Source {
                owner: format!("path:{}", directory.display()),
                directory: directory.clone(),
                revision: "local".into(),
                reinstall: directory.to_string_lossy().into_owned(),
                selection: None,
            })
        }
        hd_deps::target::TargetForm::Host(host) => {
            let mut progress = |key: &str| eprintln!("hd: fetching {key}");
            let cache = Cache::new(crate::cache_root());
            let mut remote = Remote::new(cache.clone(), &mut progress);
            let query = hd_deps::query::parse_query(target.query.as_deref())?;
            let selected = remote.resolve(&host, &query).map_err(|(_, e)| e)?;
            let entry = remote.entry(&host, &selected.version).map_err(|(_, e)| e)?;
            let scratch = cache.scratch()?;
            fs::write(
                scratch.path.join("hd.toml"),
                format!(
                    "[package]\nname=\"install_selector\"\n[dependencies]\napp={}\n",
                    hd_project::edit::string_value(&format!("{}@{}", host.path, selected.version))
                ),
            )
            .map_err(io)?;
            let sum = disk::read_sum(&entry.dir)
                .map_err(|(line, error)| format!("hd.sum:{line}: {error}"))?;
            let selection = hd_deps::select_executable_entry(
                &scratch.path,
                "app",
                &sum,
                hd_deps::Mode::Record,
                &mut remote,
                disk::TOOLCHAIN,
            );
            if let Some(problem) = selection.problems.first() {
                return Err(problem.message.clone());
            }
            let identity = format!(
                "{}@{}",
                host.path,
                hd_project::compatibility_line(&selected.version)
            );
            let first = selection
                .packages
                .iter()
                .position(|p| p.identity == identity)
                .ok_or("installation source was not selected")?;
            let package = &selection.packages[first];
            let fetched = package
                .fetched
                .as_ref()
                .ok_or("installation source is not a fetched package")?;
            Ok(Source {
                directory: package.dir.clone(),
                owner: format!("host:{}", host.path),
                revision: fetched
                    .rsplit_once('@')
                    .ok_or("invalid selected version")?
                    .1
                    .to_owned(),
                reinstall: fetched.clone(),
                selection: Some((selection, first)),
            })
        }
    }
}

fn grants(table: &[(String, Grant)], directory: &Path) -> Value {
    let resolved = caps::grants(table, directory, &[]);
    let mut result = serde_json::Map::new();
    for (key, grant) in table {
        let value = match grant {
            Grant::All => json!(true),
            Grant::Deny => json!(false),
            Grant::Scopes(entries) => {
                let entries = resolved
                    .limits
                    .iter()
                    .find_map(|(k, l)| match l {
                        hd_run::Limit::Entries(paths) if k == key => Some(paths),
                        _ => None,
                    })
                    .unwrap_or(entries);
                json!(entries)
            }
        };
        result.insert(key.clone(), value);
    }
    Value::Object(result)
}

fn table(record: &Value) -> Result<Vec<(String, Grant)>, String> {
    let values = record["capabilities"]
        .as_object()
        .ok_or("invalid installation capabilities")?;
    values
        .iter()
        .map(|(key, value)| {
            let grant = match value {
                Value::Bool(true) => Grant::All,
                Value::Bool(false) => Grant::Deny,
                Value::Array(entries) => Grant::Scopes(
                    entries
                        .iter()
                        .map(|e| {
                            e.as_str()
                                .map(str::to_owned)
                                .ok_or("invalid installation scope".into())
                        })
                        .collect::<Result<_, String>>()?,
                ),
                _ => return Err("invalid installation grant".into()),
            };
            if let Some(e) = hd_project::grant_problem(key, &grant) {
                return Err(e);
            }
            Ok((key.clone(), grant))
        })
        .collect()
}

fn payload(root: &Path, record: &Value) -> Result<PathBuf, String> {
    let generation = field(record, "generation")?;
    let name = field(record, "name")?;
    if !valid_name(name)
        || generation.is_empty()
        || !generation.bytes().all(|b| b.is_ascii_digit() || b == b'-')
    {
        return Err("invalid installation payload path".into());
    }
    Ok(root
        .join("installed")
        .join(generation)
        .join(format!("{name}.wasm")))
}

fn collect_old(root: &Path, before: &Value, after: &Value) {
    let live: BTreeSet<PathBuf> = after["commands"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, record)| payload(root, record).ok())
        .collect();
    for (_, record) in before["commands"].as_object().into_iter().flatten() {
        if let Ok(path) = payload(root, record)
            && !live.contains(&path)
            && let Err(e) = fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "hd: cannot reclaim old installation payload {}: {e}",
                path.display()
            );
        }
    }
    let generations = |registry: &Value| -> BTreeSet<String> {
        registry["commands"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(_, r)| r["generation"].as_str().map(str::to_owned))
            .collect()
    };
    for generation in generations(before).difference(&generations(after)) {
        // Only generated names are ever eligible; never follow a record's path.
        if !generation.is_empty()
            && generation.bytes().all(|b| b.is_ascii_digit() || b == b'-')
            && let Err(e) = fs::remove_dir_all(root.join("installed").join(generation))
        {
            eprintln!("hd: cannot reclaim old installation payload {generation}: {e}");
        }
    }
}

fn do_install(options: Options, rep: &mut Reporter) -> Result<(), ExitCode> {
    let root = root(options.root).map_err(|e| rep.fail(&e))?;
    if options.list {
        if !root.join("installed/registry.json").exists() {
            return Ok(());
        }
        let _lock = lock(&root).map_err(|e| rep.fail(&e))?;
        let registry = registry(&root).map_err(|e| rep.fail(&e))?;
        for (name, record) in registry["commands"].as_object().expect("checked registry") {
            println!(
                "{name}\t{}\t{}",
                field(record, "source")
                    .map_err(|e| rep.fail(&e))?
                    .split_once(':')
                    .map_or("", |(_, location)| location),
                field(record, "revision").map_err(|e| rep.fail(&e))?
            );
        }
        return Ok(());
    }
    let word = options.operand.expect("source was checked");
    let selected = source(&word).map_err(|e| rep.fail(&e))?;
    if !disk::declares_package(&selected.directory).map_err(|e| rep.fail(&e))? {
        return Err(rep.fail("installation source needs an hd.toml declaring a package"));
    }
    let package = disk::package_name(&selected.directory).map_err(|e| rep.fail(&e))?;
    let executables = disk::executables(&selected.directory, &package);
    if executables.is_empty() {
        return Err(rep.fail("the installation source has no executables"));
    }
    let mut built = Vec::new();
    let mut names = BTreeSet::new();
    for executable in executables {
        if !valid_name(&executable.name) || !names.insert(executable.name.to_ascii_lowercase()) {
            return Err(rep.fail(&format!(
                "invalid or duplicate installed executable name `{}`",
                executable.name
            )));
        }
        let mut program = match &selected.selection {
            Some((selection, first)) => disk::load_selected_package(
                &selected.directory,
                &executable.file,
                selection,
                *first,
            ),
            None => disk::load_package(&selected.directory, &executable.file),
        }
        .map_err(|e| rep.fail(&e))?;
        for denied in &options.deny {
            if program
                .capabilities
                .iter()
                .any(|(key, value)| key == denied && *value != Grant::Deny)
            {
                return Err(rep.fail(&format!(
                    "installation denied: package grants capability `{denied}`"
                )));
            }
        }
        program.only_program(&executable.file);
        let wasm = crate::compile_standalone(
            &program,
            (
                hd_driver::BuildProfile::Release,
                Pipeline::of(engine::EngineKind::Node, true),
            ),
            rep,
        )?;
        if let Some(artifacts) = hd_driver::host_artifacts(&wasm).map_err(|e| rep.fail(e))?
            && let Some(provider) = artifacts.contract.interfaces.iter().find(|i| !i.handle)
        {
            return Err(rep.fail(&format!(
                "installation of package providers is not supported yet: {}.{}",
                provider.identity.package, provider.identity.trait_path
            )));
        }
        built.push((
            executable.name,
            wasm,
            grants(&program.capabilities, &selected.directory),
        ));
    }
    let _lock = lock(&root).map_err(|e| rep.fail(&e))?;
    let root = hd_run::canonicalize(&root).map_err(|e| rep.fail(&io(e)))?;
    let before = registry(&root).map_err(|e| rep.fail(&e))?;
    let mut next = before.clone();
    let commands = next["commands"].as_object_mut().expect("checked registry");
    let removed: Vec<String> = commands
        .iter()
        .filter(|(name, r)| {
            r["source"] == selected.owner && !built.iter().any(|(n, _, _)| n == *name)
        })
        .map(|(n, _)| n.clone())
        .collect();
    for name in &removed {
        check_launcher(&root, name, true).map_err(|e| rep.fail(&e))?;
    }
    for (name, _, _) in &built {
        if commands.iter().any(|(n, r)| {
            n.eq_ignore_ascii_case(name) && (n != name || r["source"] != selected.owner)
        }) {
            return Err(rep.fail(&format!(
                "installed executable `{name}` belongs to another source"
            )));
        }
        check_launcher(&root, name, commands.contains_key(name)).map_err(|e| rep.fail(&e))?;
    }
    let generation = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let directory = root.join("installed").join(&generation);
    fs::create_dir(&directory).map_err(|e| rep.fail(&io(e)))?;
    fs::create_dir_all(root.join("bin")).map_err(|e| rep.fail(&io(e)))?;
    let mut created = Vec::new();
    let prepare = (|| -> Result<(), String> {
        let commands = next["commands"].as_object_mut().expect("checked registry");
        for (name, wasm, capabilities) in &built {
            fs::write(directory.join(format!("{name}.wasm")), wasm).map_err(io)?;
            if !bin(&root, name).exists() {
                write_launcher(&root, name)?;
                created.push(name.clone());
            }
            commands.insert(name.clone(),json!({"name":name,"source":selected.owner,"reinstall":selected.reinstall,"revision":selected.revision,"generation":generation,"abi":hd_host_abi::RUNTIME_ABI_REVISION,"sha256":hash(wasm),"capabilities":capabilities}));
        }
        for name in &removed {
            commands.remove(name);
        }
        save(&root, &next)
    })();
    if let Err(error) = prepare {
        for name in created {
            let _ = fs::remove_file(bin(&root, &name));
        }
        let _ = fs::remove_dir_all(directory);
        return Err(rep.fail(&error));
    }
    for name in removed {
        remove_launcher(&root, &name).map_err(|e| rep.fail(&e))?;
    }
    collect_old(&root, &before, &next);
    for (name, _, capabilities) in built {
        println!("installed {name}; capabilities {capabilities}");
    }
    eprintln!("hd: add {} to PATH", root.join("bin").display());
    Ok(())
}

pub(crate) fn install(args: &[OsString]) -> ExitCode {
    let options = match options(args, false) {
        Ok(o) => o,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stdout(false);
    match do_install(options, &mut rep) {
        Ok(()) => rep.finish(0),
        Err(code) => code,
    }
}

pub(crate) fn uninstall(args: &[OsString]) -> ExitCode {
    let result = (|| -> Result<(), String> {
        let options = options(args, true)?;
        let root = root(options.root)?;
        let name = options.operand.expect("name was checked");
        if !valid_name(&name) {
            return Err(format!("invalid installed executable name `{name}`"));
        }
        let _lock = lock(&root)?;
        let root = hd_run::canonicalize(root).map_err(io)?;
        let before = registry(&root)?;
        if before["commands"].get(&name).is_none() {
            return Err(format!("`{name}` is not installed"));
        }
        check_launcher(&root, &name, true)?;
        let mut next = before.clone();
        next["commands"]
            .as_object_mut()
            .expect("checked registry")
            .remove(&name);
        let had_launcher = bin(&root, &name).exists();
        remove_launcher(&root, &name)?;
        if let Err(error) = save(&root, &next) {
            if had_launcher {
                let _ = write_launcher(&root, &name);
            }
            return Err(error);
        }
        collect_old(&root, &before, &next);
        println!("uninstalled {name}");
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}

pub(crate) fn run(args: &[OsString]) -> ExitCode {
    let result = (|| -> Result<_, String> {
        let [root, name, separator, words @ ..] = args else {
            return Err("invalid installed launcher arguments".into());
        };
        if separator != "--" {
            return Err("invalid installed launcher separator".into());
        }
        let name = name
            .to_str()
            .filter(|n| valid_name(n))
            .ok_or("invalid installed command name")?;
        let root = Path::new(root);
        let _lock = lock(root)?;
        let registry = registry(root)?;
        let record = &registry["commands"][name];
        if record.is_null() {
            return Err(format!("`{name}` is not installed"));
        }
        let reinstall = format!(
            "reinstall with hd install {}",
            quote(field(record, "reinstall")?)
        );
        if record["abi"] != hd_host_abi::RUNTIME_ABI_REVISION {
            return Err(format!(
                "incompatible installed Wasm/runtime ABI; {reinstall}"
            ));
        }
        let wasm = fs::read(payload(root, record)?).map_err(io)?;
        if hash(&wasm) != field(record, "sha256")? {
            return Err(format!("installed artifact hash mismatch; {reinstall}"));
        }
        if hd_driver::host_artifacts(&wasm)
            .map_err(str::to_owned)?
            .is_some()
        {
            return Err("installation of package providers is not supported yet".into());
        }
        Ok((wasm, table(record)?, name.to_owned(), words))
    })();
    let (wasm, table, name, words) = match result {
        Ok(r) => r,
        Err(e) => return fail(&e),
    };
    let engine = match engine::from_env() {
        Ok(e) => e,
        Err(e) => return fail(&e),
    };
    let mut rep = Reporter::stderr(false);
    crate::execute(
        &Compiled { wasm, frames: None },
        &name,
        &table,
        None,
        &[],
        None,
        words,
        (
            engine,
            hd_driver::OptLevel::Optimized,
            engine::ProgramLimits::default(),
        ),
        &mut rep,
    )
}
