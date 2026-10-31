use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;
use serde_json::json;

use crate::config::{BottleConfig, Config, GraphicsBackend, RunnerKind};
use crate::game::{self, EXECUTABLE, LAUNCHER};
use crate::runner;
use crate::steam;
use crate::util::{err, home, logs_dir, Result, Task};

const ENV_BEGIN: &str = ";; asahiro managed begin";
const ENV_END: &str = ";; asahiro managed end";
const MANAGED_KEYS: [&str; 10] = [
    "CX_GRAPHICS_BACKEND",
    "WINEMSYNC",
    "WINEESYNC",
    "MTL_HUD_ENABLED",
    "D3DM_ENABLE_METALFX",
    "D3DM_SUPPORT_DXR",
    "D3DM_DEVICE_ID",
    "D3DM_DEVICE_DESCRIPTION",
    "DXMT_ENABLE_NVEXT",
    "WINEDLLOVERRIDES",
];

#[derive(Serialize)]
pub struct WineInfo {
    pub crossover_found: bool,
    pub crossover_version: Option<String>,
    pub runner: RunnerKind,
    pub runner_name: String,
    pub runner_installed: bool,
    pub rosetta: bool,
    pub bottle_exists: bool,
    pub bottle_path: String,
    pub game_running: bool,
}

// the wine build a command runs on, both use the same crossover-style bottle
pub enum Runner {
    CrossOver(PathBuf),
    Bundled(PathBuf),
}

impl Runner {
    // always the bundled runner: the game's anti-cheat needs its patched kernel side, crossover's wine
    // does not have it. the bundled one downloads when a task is available
    pub fn resolve(_cfg: &Config, task: Option<&Task>) -> Result<Runner> {
        match task {
            Some(t) => Ok(Runner::Bundled(runner::ensure(t)?)),
            None if runner::installed() => Ok(Runner::Bundled(runner::runner_dir())),
            None => err("the wine runner is not downloaded yet, start the game once first"),
        }
    }

    // a wine invocation for the bottle, arguments are passed through as windows paths
    fn wine(&self, cfg: &Config, workdir: Option<&Path>) -> Command {
        let b = &cfg.bottle;
        let mut c = match self {
            Runner::CrossOver(root) => {
                let mut c = Command::new(root.join("bin/wine"));
                c.arg("--bottle").arg(&b.name);
                if let Some(w) = workdir {
                    c.arg("--workdir").arg(w);
                }
                c.arg("--no-convert");
                c
            }
            Runner::Bundled(dir) => {
                let mut c = Command::new(dir.join("bin/wine"));
                c.env("WINEPREFIX", bottle_path(cfg));
                if let Some(w) = workdir {
                    c.current_dir(w);
                }
                c
            }
        };
        // the default lives in the purgeable darwin cache folder, which macos empties when disk space runs low
        if !b.env.contains_key("DXMT_SHADER_CACHE_PATH") {
            c.env("DXMT_SHADER_CACHE_PATH", dxmt_cache_dir());
        }
        for (k, v) in bottle_env(b) {
            c.env(k, v);
        }
        if matches!(self, Runner::Bundled(_)) {
            if !b.msync {
                c.env("WINEESYNC", "1");
            }
            c.env("GST_PLUGIN_FEATURE_RANK", "atdec:MAX,avdec_h264:MAX");
            // crossover's ntdll preloads this for d3dmetal, its wine script sets it the same way
            let d3dshared = runner::d3dshared_path();
            if d3dshared.exists() {
                c.env("CX_APPLEGPTK_LIBD3DSHARED_PATH", d3dshared);
            }
        }
        c.env("WINEDEBUG", "-all");
        c
    }

    // the game has to be the process that boots the wineserver, an already running one makes the
    // anti-cheat abort, so every launch waits for the setup commands' server to go away first
    fn wait_idle(&self, cfg: &Config) {
        let _ = Command::new(self.wineserver()).env("WINEPREFIX", bottle_path(cfg)).arg("-w").status();
    }

    fn wineserver(&self) -> PathBuf {
        match self {
            Runner::CrossOver(root) => root.join("bin/wineserver"),
            Runner::Bundled(dir) => dir.join("bin/wineserver"),
        }
    }
}

pub fn crossover_root(cfg: &Config) -> Option<PathBuf> {
    let candidates = [
        PathBuf::from(&cfg.crossover_path),
        PathBuf::from("/Applications/CrossOver.app"),
        home().join("Applications/CrossOver.app"),
    ];
    candidates
        .into_iter()
        .map(|app| app.join("Contents/SharedSupport/CrossOver"))
        .find(|root| root.join("bin/wine").exists())
}

fn dxmt_cache_dir() -> PathBuf {
    let p = crate::util::data_dir().join("shadercache/dxmt");
    if !p.exists() {
        let _ = std::fs::create_dir_all(&p);
        import_dxmt_cache(&p);
    }
    p
}

// carries over the cache dxmt built in its default location, so moving it costs no recompiles
fn import_dxmt_cache(to: &Path) {
    let Ok(out) = Command::new("/usr/bin/getconf").arg("DARWIN_USER_CACHE_DIR").output() else { return };
    let old = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()).join("dxmt").join(EXECUTABLE);
    let Ok(entries) = std::fs::read_dir(&old) else { return };
    for e in entries.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        // the write-ahead log holds entries not yet merged into the database
        if name.starts_with("shaders_") && (name.ends_with(".db") || name.ends_with(".db-wal")) {
            let _ = std::fs::copy(e.path(), to.join(&name));
        }
    }
}


// the bottle holds steam and the game, so it lives where the game folder setting points
pub fn bottle_path(cfg: &Config) -> PathBuf {
    cfg.game_path()
}

fn bottle_exists(cfg: &Config) -> bool {
    bottle_path(cfg).join("drive_c").exists()
}

fn crossover_version(cfg: &Config) -> Option<String> {
    let root = crossover_root(cfg)?;
    let plist = root.parent()?.parent()?.join("Info.plist");
    let out = Command::new("/usr/bin/defaults")
        .arg("read")
        .arg(plist)
        .arg("CFBundleShortVersionString")
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

pub fn info(cfg: &Config) -> WineInfo {
    let rosetta = Command::new("/usr/bin/arch")
        .args(["-x86_64", "/usr/bin/true"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let bottle = bottle_path(cfg);
    WineInfo {
        crossover_found: crossover_root(cfg).is_some(),
        crossover_version: crossover_version(cfg),
        runner: RunnerKind::Bundled,
        runner_name: runner::RUNNER_NAME.to_string(),
        runner_installed: runner::installed(),
        rosetta,
        bottle_exists: bottle_exists(cfg),
        bottle_path: bottle.to_string_lossy().into_owned(),
        game_running: game_running(),
    }
}

// wine shows the windows command line for its processes, so only count those and skip
// the crossover wrapper and steam shim which carry the exe path as an argument
pub fn game_running() -> bool {
    let Ok(out) = Command::new("/bin/ps").args(["-axo", "args="]).output() else { return false };
    String::from_utf8_lossy(&out.stdout).lines().any(|l| {
        let l = l.trim_start();
        // the path has spaces in it (program files), so the exe name is matched up to its own end
        l.len() > 3
            && l.as_bytes()[1] == b':'
            && l.as_bytes()[2] == b'\\'
            && l.find(&format!("\\{}", EXECUTABLE)).is_some_and(|i| {
                let rest = &l[i + 1 + EXECUTABLE.len()..];
                rest.is_empty() || rest.starts_with(' ')
            })
    })
}

// environment shared by the bottle config and the launch command
fn bottle_env(b: &BottleConfig) -> Vec<(String, String)> {
    let mut env = vec![("CX_GRAPHICS_BACKEND".to_string(), b.graphics_backend.as_str().to_string())];
    if b.msync && b.graphics_backend != GraphicsBackend::Dxvk {
        env.push(("WINEMSYNC".into(), "1".into()));
    }
    if b.metal_hud {
        env.push(("MTL_HUD_ENABLED".into(), "1".into()));
    }
    let d3dmetal = b.graphics_backend == GraphicsBackend::D3dmetal;
    // d3dmetal runs dlss through metalfx, so dlss needs it on too
    if (b.metalfx || b.dlss) && d3dmetal {
        env.push(("D3DM_ENABLE_METALFX".into(), "1".into()));
    }
    if b.raytracing && d3dmetal {
        env.push(("D3DM_SUPPORT_DXR".into(), "1".into()));
    }
    // d3dmetal names the gpu "AMD Compatibility Mode", which zzz does not recognize, so it hides its
    // super resolution and ray tracing settings. an rtx name gets them shown
    if (b.dlss || b.raytracing) && d3dmetal {
        env.push(("D3DM_DEVICE_ID".into(), "0x2684".into()));
        env.push(("D3DM_DEVICE_DESCRIPTION".into(), "NVIDIA GeForce RTX 4090".into()));
    }
    if b.dlss {
        env.push(("DXMT_ENABLE_NVEXT".into(), "1".into()));
    }
    if !b.dll_overrides.trim().is_empty() {
        env.push(("WINEDLLOVERRIDES".into(), b.dll_overrides.trim().to_string()));
    }
    for (k, v) in &b.env {
        if !k.trim().is_empty() {
            env.push((k.trim().to_string(), v.clone()));
        }
    }
    env
}

// rewrites our block inside [EnvironmentVariables] so the crossover gui reflects the same settings
fn write_bottle_env(cfg: &Config) -> Result<()> {
    let b = &cfg.bottle;
    let conf = bottle_path(cfg).join("cxbottle.conf");
    let Ok(text) = std::fs::read_to_string(&conf) else { return Ok(()) };
    let env = bottle_env(b);
    let user_keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
    let mut out = Vec::new();
    let mut in_section = false;
    let mut in_block = false;
    let mut wrote = false;
    let block: Vec<String> = std::iter::once(ENV_BEGIN.to_string())
        .chain(env.iter().map(|(k, v)| format!("\"{}\" = \"{}\"", k, v.replace('"', "\\\""))))
        .chain(std::iter::once(ENV_END.to_string()))
        .collect();
    for line in text.lines() {
        let t = line.trim();
        if t == ENV_BEGIN {
            in_block = true;
            continue;
        }
        if t == ENV_END {
            in_block = false;
            continue;
        }
        if in_block {
            continue;
        }
        if t.starts_with('[') {
            in_section = t == "[EnvironmentVariables]";
            out.push(line.to_string());
            if in_section && !wrote {
                out.extend(block.iter().cloned());
                wrote = true;
            }
            continue;
        }
        if in_section {
            let key = t.strip_prefix('"').and_then(|r| r.split('"').next()).unwrap_or("");
            if MANAGED_KEYS.contains(&key) || user_keys.contains(&key) {
                continue;
            }
        }
        out.push(line.to_string());
    }
    if !wrote {
        out.push("[EnvironmentVariables]".into());
        out.extend(block);
    }
    std::fs::write(conf, out.join("\n") + "\n")?;
    Ok(())
}

fn run_logged(task: &Task, mut c: Command, what: &str) -> Result<()> {
    let out = c.output()?;
    if !out.status.success() {
        task.log(String::from_utf8_lossy(&out.stderr).to_string());
        return err(format!("{} failed ({})", what, out.status));
    }
    Ok(())
}


// a plain prefix at the game folder, initialised by the bundled runner
pub fn create_bottle(task: &Task, cfg: &Config) -> Result<()> {
    if bottle_exists(cfg) {
        task.status("Bottle already exists, applying settings");
    } else {
        let rn = Runner::resolve(cfg, Some(task))?;
        task.status("Creating the bottle");
        task.progress(None, "This can take a minute");
        std::fs::create_dir_all(bottle_path(cfg))?;
        let mut c = rn.wine(cfg, None);
        c.arg("wineboot").arg("-u");
        run_logged(task, c, "bottle creation")?;
    }
    apply_bottle(task, cfg)
}

pub fn delete_bottle(cfg: &Config) -> Result<()> {
    if bottle_exists(cfg) {
        std::fs::remove_dir_all(bottle_path(cfg))?;
    }
    Ok(())
}

// reads user.reg directly, the wineserver is not running between launches so the file is what wine will load
fn registry_current(bottle: &Path, b: &BottleConfig) -> bool {
    let Ok(bytes) = std::fs::read(bottle.join("user.reg")) else { return false };
    let text = String::from_utf8_lossy(&bytes);
    let mut section = false;
    let mut values = std::collections::HashMap::new();
    for l in text.lines() {
        if l.starts_with('[') {
            section = l.starts_with("[Software\\\\Wine\\\\Mac Driver]");
        } else if section {
            if let Some((k, v)) = l.split_once('=') {
                values.insert(k.trim_matches('"').to_string(), v.trim_matches('"').to_string());
            }
        }
    }
    let yn = |v: bool| if v { "y" } else { "n" };
    [
        ("RetinaMode", b.retina),
        ("LeftCommandIsCtrl", b.left_cmd_is_ctrl),
        ("LeftOptionIsAlt", b.left_option_is_alt),
        ("RightOptionIsAlt", b.right_option_is_alt),
    ]
    .iter()
    .all(|(k, v)| values.get(*k).map(String::as_str) == Some(yn(*v)))
}

// writes registry tweaks through a batch file so the bottle only boots once
pub fn apply_bottle(task: &Task, cfg: &Config) -> Result<()> {
    let b = &cfg.bottle;
    let bottle = bottle_path(cfg);
    if !bottle_exists(cfg) {
        return err("the bottle does not exist yet");
    }
    let rn = Runner::resolve(cfg, Some(task))?;
    task.status("Configuring bottle");
    write_bottle_env(cfg)?;
    let marker = bottle.join(".asahiro-winver");
    let winver_current = std::fs::read_to_string(&marker).ok().as_deref() == Some(b.windows_version.as_str());
    if winver_current && registry_current(&bottle, b) {
        return Ok(());
    }
    let yn = |v: bool| if v { "y" } else { "n" };
    let key = "HKEY_CURRENT_USER\\Software\\Wine\\Mac Driver";
    let bat = [
        "@echo off".to_string(),
        format!("reg add \"{}\" /v RetinaMode /t REG_SZ /d {} /f >nul", key, yn(b.retina)),
        format!("reg add \"{}\" /v LeftCommandIsCtrl /t REG_SZ /d {} /f >nul", key, yn(b.left_cmd_is_ctrl)),
        // games expect alt, macos would otherwise turn option into a dead key
        format!("reg add \"{}\" /v LeftOptionIsAlt /t REG_SZ /d {} /f >nul", key, yn(b.left_option_is_alt)),
        format!("reg add \"{}\" /v RightOptionIsAlt /t REG_SZ /d {} /f >nul", key, yn(b.right_option_is_alt)),
        "exit /b 0".to_string(),
    ];
    let bat_path = bottle.join("drive_c/asahiro-setup.bat");
    std::fs::write(&bat_path, bat.join("\r\n") + "\r\n")?;
    let mut c = rn.wine(cfg, None);
    c.arg("cmd").arg("/c").arg("C:\\asahiro-setup.bat");
    run_logged(task, c, "bottle configuration")?;
    let _ = std::fs::remove_file(bat_path);

    if !winver_current {
        let mut c = rn.wine(cfg, None);
        c.arg("winecfg").arg("-v").arg(&b.windows_version);
        run_logged(task, c, "setting the windows version")?;
        std::fs::write(marker, &b.windows_version)?;
    }
    Ok(())
}

// ace-core wants lsass's SamSs service running. the patched runner ships the lsass stub and its wineboot
// starts the service on every boot, the bottle only needs the service registered once
fn ensure_samss(task: &Task, cfg: &Config, rn: &Runner) -> Result<()> {
    let bottle = bottle_path(cfg);
    let Runner::Bundled(dir) = rn else { return Ok(()) };
    let lsass = dir.join("lib/wine/x86_64-windows/lsass.exe");
    if !lsass.exists() {
        return err("the wine runner has no lsass.exe, it is not the patched runner");
    }
    runner::copy_if_changed(&lsass, &bottle.join("drive_c/windows/system32/lsass.exe"))?;
    let registered = std::fs::read_to_string(bottle.join("system.reg"))
        .map(|r| r.contains("\\\\Services\\\\SamSs]"))
        .unwrap_or(false);
    if !registered {
        task.status("Registering the SamSs service");
        let mut c = rn.wine(cfg, None);
        c.args(["sc", "create", "SamSs", "binPath=", "C:\\windows\\system32\\lsass.exe", "type=", "share", "start=", "demand"]);
        run_logged(task, c, "registering SamSs")?;
        rn.wait_idle(cfg);
    }
    Ok(())
}

// the bottle, the SamSs service and steam with its web helper wrapped
pub fn ensure_steam(task: &Task, cfg: &Config) -> Result<()> {
    let rn = Runner::resolve(cfg, Some(task))?;
    if !bottle_exists(cfg) {
        create_bottle(task, cfg)?;
    } else {
        apply_bottle(task, cfg)?;
    }
    ensure_samss(task, cfg, &rn)?;
    let bottle = bottle_path(cfg);
    if !steam::installed(&bottle) {
        steam::install(task, rn.wine(cfg, None))?;
        rn.wait_idle(cfg);
    }
    steam::wrap_webhelper(&bottle)?;
    Ok(())
}

fn select(task: &Task, cfg: &Config, rn: &Runner, g: runner::Graphics) -> Result<()> {
    if let Runner::Bundled(_) = rn {
        runner::select_graphics(task, g, None, &bottle_path(cfg))?;
    }
    Ok(())
}

// steam's windows only render with wine's own d3d, the game needs dxmt. steam starts on the builtin
// libraries and once its web helper has loaded them the runner switches to dxmt, the helper keeps what
// it loaded. a running steam gets the arguments handed over instead
pub fn start_steam(task: &Task, cfg: &Config, args: &[String]) -> Result<()> {
    let rn = Runner::resolve(cfg, Some(task))?;
    if steam::running() {
        if !args.is_empty() {
            let mut c = rn.wine(cfg, None);
            c.arg(steam::EXE).args(args).stdout(Stdio::null()).stderr(Stdio::null());
            c.spawn()?;
        }
        return Ok(());
    }
    task.status("Starting Steam");
    task.progress(None, "");
    select(task, cfg, &rn, runner::Graphics::Builtin)?;
    let log = std::fs::File::create(logs_dir().join("steam.log"))?;
    let mut c = rn.wine(cfg, None);
    // steam's startup check puts valve's web helper back over the wrapper unless it leaves files alone
    c.arg(steam::EXE)
        .args(["-noverifyfiles", "-norepairfiles", "-cef-disable-gpu", "-cef-disable-sandbox"])
        .args(args)
        // since game 3.4 these load a broken video plugin, proton's fix for the game unsets them too
        .env_remove("SteamOS")
        .env_remove("SteamDeck")
        .stdout(log.try_clone()?)
        .stderr(log);
    if cfg.frame_cap > 0 && !cfg.bottle.env.contains_key("DXMT_CONFIG") {
        c.env("DXMT_CONFIG", format!("d3d11.preferredMaxFrameRate={}", cfg.frame_cap));
    }
    // the game inherits steam's environment, so this is where wwmi's d3d11.dll gets allowed in. without
    // mods it changes nothing, the d3d11.dll in system32 is wine's builtin placeholder
    c.env("WINEDLLOVERRIDES", dll_overrides(cfg));
    c.spawn()?;
    let loaded = steam::wait_for_helper_d3d(task, Duration::from_secs(180));
    if !loaded {
        task.log("steam's web helper did not load d3d in time, switching to dxmt anyway");
    }
    select(task, cfg, &rn, runner::Graphics::Dxmt)
}

// steam adds the game's own arguments, these are only the user's extra ones. no -dx11 like xxmi passes: the
// game is on d3d11 unless ray tracing is on, and the flag makes it quit and start again through steam
fn launch_args(cfg: &Config) -> Vec<String> {
    cfg.launch_args.split_whitespace().map(String::from).collect()
}

// starts steam when needed, then asks it to start the game. steam passes the game its own environment,
// so the frame cap is set when steam starts. the game itself always runs on dxmt
pub fn launch(task: &Task, cfg: &Config) -> Result<()> {
    if game::installed_version(cfg).is_none() {
        return err("the game is not installed, install it through Steam first");
    }
    if game_running() {
        return err("the game is already running");
    }
    ensure_steam(task, cfg)?;
    let rn = Runner::resolve(cfg, Some(task))?;
    if let Err(e) = runner::ensure_game_app() {
        task.log(format!("game mode bundle skipped: {}", e));
    }
    if cfg.mods_enabled {
        task.status("Preparing WWMI");
        if let Err(e) = crate::mods::update_if_needed(task, cfg) {
            if !crate::mods::installed() {
                return Err(e);
            }
            task.log(format!("wwmi update skipped: {}", e));
        }
        // no UserEngine.ini lod setting like xxmi writes: under wine ace stops the game within a minute of
        // it being there (13-131081-130), even with mods off
        crate::mods::prepare(cfg)?;
    }
    start_steam(task, cfg, &[])?;
    select(task, cfg, &rn, runner::Graphics::Dxmt)?;

    // the injector waits for the game and loads 3dmigoto into it, the game itself comes from steam
    let mut injector = if cfg.mods_enabled { Some(start_injector(task, &rn, cfg)?) } else { None };
    task.status("Starting Wuthering Waves");
    task.progress(None, "");
    let mut c = rn.wine(cfg, None);
    c.arg(steam::EXE).args(["-noverifyfiles", "-norepairfiles", "-applaunch", steam::APP_ID]).args(launch_args(cfg));
    c.stdout(Stdio::null()).stderr(Stdio::null());
    c.spawn()?.wait()?;

    // steam may first apply an update or show a dialog, so it gets a while to bring the game up
    let started = std::time::Instant::now();
    while !game_running() {
        if task.cancelled() {
            return Ok(());
        }
        if started.elapsed() > Duration::from_secs(180) {
            return err("Steam did not start the game, check Steam's window for an update or an error");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    task.event(json!({ "type": "game_started" }));
    task.status("Playing");
    while game_running() {
        if task.cancelled() {
            stop_game(cfg);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    // the injector exits on its own after injecting, this only reaps it or stops a stuck one
    if let Some(mut helper) = injector.take() {
        let _ = helper.kill();
        let _ = helper.wait();
    }
    task.status("Game closed");
    Ok(())
}

// the user's overrides plus native first d3d11, so 3dmigoto's d3d11.dll loaded by path is not swapped
// for the builtin one
fn dll_overrides(cfg: &Config) -> String {
    let mut parts: Vec<String> =
        cfg.bottle.dll_overrides.split(';').map(str::trim).filter(|p| !p.is_empty()).map(String::from).collect();
    // xxmi's d3dcompiler_47.dll too, wine's builtin one can't compile wwmi's shaders
    for dll in ["d3d11", "d3dcompiler_47"] {
        if !parts.iter().any(|p| p.split('=').next().is_some_and(|k| k.split(',').any(|d| d.trim() == dll))) {
            parts.push(format!("{}=n,b", dll));
        }
    }
    parts.join(";")
}

fn windows_path(p: &Path) -> String {
    format!("Z:{}", p.to_string_lossy().replace('/', "\\"))
}

// starts the injector before steam starts the game, its status lines go to the task log
fn start_injector(task: &Task, rn: &Runner, cfg: &Config) -> Result<std::process::Child> {
    use std::io::BufRead;
    let dir = crate::mods::zzmi_dir();
    let mut c = rn.wine(cfg, None);
    c.env("WINEDLLOVERRIDES", dll_overrides(cfg))
        .arg(windows_path(&dir.join(crate::mods::HELPER_NAME)))
        .arg(windows_path(&dir.join("3dmloader.dll")))
        .arg(windows_path(&dir.join("d3d11.dll")))
        .arg(EXECUTABLE)
        // xxmi injects wwmi directly into the running game, which ace under wine reports as irregular.
        // the global hook loads it as the game starts instead, like zzmi
        .arg("240")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    task.log("wwmi injector: starting");
    let mut child = c.spawn()?;
    let out = child.stdout.take().ok_or("injector has no stdout")?;
    let task = task.clone();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(out).lines().map_while(|l| l.ok()) {
            let line = line.trim().to_string();
            if !line.is_empty() {
                task.log(format!("wwmi injector: {}", line));
            }
            match line.as_str() {
                "injected" => task.event(json!({ "type": "mods_injected", "ok": true })),
                "inject_timeout" => task.event(json!({ "type": "mods_injected", "ok": false })),
                l if l.starts_with("error") => task.event(json!({ "type": "mods_injected", "ok": false, "error": l })),
                _ => {}
            }
        }
    });
    Ok(child)
}

// stops the game only, steam keeps running for the next launch
pub fn stop_game(cfg: &Config) {
    let _ = cfg;
    for name in [EXECUTABLE, LAUNCHER] {
        let _ = Command::new("/usr/bin/pkill").args(["-f", name]).status();
    }
}

pub fn open_tool(cfg: &Config, tool: &str) -> Result<()> {
    let allowed = ["winecfg", "regedit", "taskmgr", "explorer", "control"];
    if !allowed.contains(&tool) {
        return err(format!("unknown tool {}", tool));
    }
    if !bottle_exists(cfg) {
        return err("create the bottle first");
    }
    let rn = Runner::resolve(cfg, None)?;
    let mut c = rn.wine(cfg, None);
    c.arg(tool).stdout(Stdio::null()).stderr(Stdio::null());
    c.spawn()?;
    Ok(())
}
