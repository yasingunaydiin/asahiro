use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;
use serde_json::json;

use crate::config::{BottleConfig, Config, GraphicsBackend, RunnerKind};
use crate::game::{self, EXECUTABLE, LAUNCHER, LAUNCHER_ARGS};
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

    // crossover's wine script drops WINEDLLOVERRIDES from the environment and only takes --dll
    fn dll_overrides(&self, c: &mut Command, overrides: &str) {
        match self {
            Runner::CrossOver(_) => {
                c.arg("--dll").arg(overrides);
            }
            Runner::Bundled(_) => {
                c.env("WINEDLLOVERRIDES", overrides);
            }
        }
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
        l.len() > 3
            && l.as_bytes()[1] == b':'
            && l.as_bytes()[2] == b'\\'
            && l.split_whitespace().next().is_some_and(|exe| exe.ends_with(EXECUTABLE))
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
    c.spawn()?;
    let loaded = steam::wait_for_helper_d3d(task, Duration::from_secs(180));
    if !loaded {
        task.log("steam's web helper did not load d3d in time, switching to dxmt anyway");
    }
    select(task, cfg, &rn, runner::Graphics::Dxmt)
}

fn launch_args(cfg: &Config) -> Vec<String> {
    let mut args: Vec<String> = LAUNCHER_ARGS.iter().map(|a| a.to_string()).collect();
    args.extend(cfg.launch_args.split_whitespace().map(String::from));
    args
}

// dxmt paces presents itself. the hooks put the game bundle's icon in the dock and can move the game into
// a macos fullscreen space
fn apply_hooks(task: &Task, cfg: &Config, c: &mut Command) {
    let b = &cfg.bottle;
    if cfg.frame_cap > 0 && !b.env.contains_key("DXMT_CONFIG") {
        c.env("DXMT_CONFIG", format!("d3d11.preferredMaxFrameRate={}", cfg.frame_cap));
    }
    let icon = runner::game_app_icon();
    if icon.is_none() && !cfg.native_fullscreen {
        return;
    }
    let p = match runner::hooks_path() {
        Ok(p) => p,
        Err(e) => return task.log(format!("hooks skipped: {}", e)),
    };
    let mut libs = p.to_string_lossy().into_owned();
    if let Some(user) = b.env.get("DYLD_INSERT_LIBRARIES").filter(|v| !v.trim().is_empty()) {
        libs = format!("{}:{}", libs, user.trim());
    }
    c.env("DYLD_INSERT_LIBRARIES", libs);
    if let Some(icon) = icon {
        c.env("ASAHIRO_DOCK_ICON", icon);
    }
    if cfg.native_fullscreen {
        c.env("ASAHIRO_NATIVE_FULLSCREEN", "1");
    }
}

fn windows_path(p: &Path) -> String {
    format!("Z:{}", p.to_string_lossy().replace('/', "\\"))
}

// starts steam when needed, then the game's launcher exe the way steam does with steam's app id set, so
// the game talks to the running client. the game itself always runs on dxmt
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
    start_steam(task, cfg, &[])?;
    select(task, cfg, &rn, runner::Graphics::Dxmt)?;

    let dir = game::game_dir(cfg);
    let log_path = logs_dir().join(format!(
        "game-{}.log",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    ));
    let log = std::fs::File::create(&log_path)?;
    let mut c = rn.wine(cfg, Some(&dir));
    apply_hooks(task, cfg, &mut c);
    if !cfg.bottle.dll_overrides.trim().is_empty() {
        rn.dll_overrides(&mut c, cfg.bottle.dll_overrides.trim());
    }
    c.env("SteamAppId", steam::APP_ID)
        .env("SteamGameId", steam::APP_ID)
        .env_remove("SteamOS")
        .env_remove("SteamDeck")
        .arg(windows_path(&dir.join(LAUNCHER)))
        .args(launch_args(cfg))
        .stdout(log.try_clone()?)
        .stderr(log);
    task.log(format!("log: {}", log_path.display()));
    task.status("Starting Wuthering Waves");
    task.progress(None, "");

    let mut child = c.spawn()?;
    let mut announced = false;
    let started = std::time::Instant::now();
    // the launcher exe hands off to the client and exits, so watch the process table for the client
    loop {
        if task.cancelled() {
            stop_game(cfg);
        }
        if !announced && game_running() {
            announced = true;
            task.event(json!({ "type": "game_started" }));
            task.status("Playing");
        }
        let exited = child.try_wait()?.is_some();
        if exited && !game_running() {
            if !announced && started.elapsed() < Duration::from_secs(120) {
                // the launcher may still be handing off, give the client a moment to show up
                std::thread::sleep(Duration::from_secs(2));
                if game_running() {
                    continue;
                }
                return err(format!("the game exited early. see {}", log_path.display()));
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    task.status("Game closed");
    Ok(())
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
