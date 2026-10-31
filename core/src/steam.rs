use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::util::{data_dir, err, Result, Task};

// the windows steam client inside the game bottle. steam installs, updates and starts the game, this
// installs steam itself, keeps its web helper wrapped and reads what steam reports about the game

pub const APP_ID: &str = "3513350";
pub const EXE: &str = "C:\\Program Files (x86)\\Steam\\steam.exe";
const SETUP_URL: &str = "https://cdn.cloudflare.steamstatic.com/client/installer/SteamSetup.exe";
const HELPER: &str = "steamwebhelper.exe";
const HELPER_REAL: &str = "steamwebhelper_real.exe";

// starts valve's web helper with --disable-gpu --single-process, see app/inject/steamwebhelper-wrapper.c
static WRAPPER: &[u8] = include_bytes!("../../app/Resources/bin/steamwebhelper.exe");

pub fn dir(bottle: &Path) -> PathBuf {
    bottle.join("drive_c/Program Files (x86)/Steam")
}

pub fn installed(bottle: &Path) -> bool {
    dir(bottle).join("steam.exe").exists()
}

fn manifest_path(bottle: &Path) -> PathBuf {
    dir(bottle).join(format!("steamapps/appmanifest_{}.acf", APP_ID))
}

// what steam's app manifest says about the game, steam writes it as soon as an install starts
pub struct AppState {
    pub install_dir: PathBuf,
    pub fully_installed: bool,
    pub update_required: bool,
    pub build_id: Option<String>,
    pub size_on_disk: u64,
    pub bytes_to_download: u64,
    pub bytes_downloaded: u64,
}

// StateFlags bits from steam's EAppState
const STATE_UPDATE_REQUIRED: u64 = 2;
const STATE_FULLY_INSTALLED: u64 = 4;

pub fn app_state(bottle: &Path) -> Option<AppState> {
    let text = std::fs::read_to_string(manifest_path(bottle)).ok()?;
    // the values we need are flat "key" "value" pairs at the top level of the vdf
    let value = |key: &str| {
        text.lines().find_map(|l| {
            let mut parts = l.trim().split('"').filter(|p| !p.trim().is_empty());
            (parts.next()? == key).then(|| parts.next().map(String::from)).flatten()
        })
    };
    let num = |key: &str| value(key).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let flags = num("StateFlags");
    Some(AppState {
        install_dir: dir(bottle).join("steamapps/common").join(value("installdir").unwrap_or_else(|| "Wuthering Waves".into())),
        fully_installed: flags & STATE_FULLY_INSTALLED != 0,
        update_required: flags & STATE_UPDATE_REQUIRED != 0,
        build_id: value("buildid").filter(|b| b != "0"),
        size_on_disk: num("SizeOnDisk"),
        bytes_to_download: num("BytesToDownload"),
        bytes_downloaded: num("BytesDownloaded"),
    })
}

// valve's installer is nsis, /S installs without a window and without starting steam afterwards
pub fn install(task: &Task, mut wine: Command) -> Result<()> {
    let setup = data_dir().join("tools/SteamSetup.exe");
    if !setup.exists() {
        task.status("Downloading Steam");
        crate::download::fetch_simple(task, SETUP_URL, &setup)?;
    }
    task.status("Installing Steam");
    task.progress(None, "");
    let out = wine.arg(&setup).arg("/S").output()?;
    if !out.status.success() {
        task.log(String::from_utf8_lossy(&out.stderr).to_string());
        let _ = std::fs::remove_file(&setup);
        return err(format!("the Steam installer failed ({})", out.status));
    }
    Ok(())
}

// cef 126 paints steam's windows black under wine on macos unless its web helper gets two extra flags,
// which steam has no option for. steam updates put valve's helper back, so this runs before every start.
// the wrapper is tiny and valve's helper is several mb, which is how the two are told apart
pub fn wrap_webhelper(bottle: &Path) -> Result<()> {
    let cef = dir(bottle).join("bin/cef");
    let Ok(entries) = std::fs::read_dir(&cef) else { return Ok(()) };
    for d in entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()) {
        let helper = d.join(HELPER);
        let Ok(meta) = std::fs::metadata(&helper) else { continue };
        if meta.len() > 1_000_000 {
            std::fs::rename(&helper, d.join(HELPER_REAL))?;
        }
        if std::fs::read(&helper).ok().as_deref() != Some(WRAPPER) {
            std::fs::write(&helper, WRAPPER)?;
        }
    }
    Ok(())
}

// wine lists its processes by their windows command line
fn pids(exe_suffix: &str) -> Vec<String> {
    let Ok(out) = Command::new("/bin/ps").args(["-axo", "pid=,args="]).output() else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (pid, args) = l.trim_start().split_once(' ')?;
            let first = args.trim_start().split(" -").next().unwrap_or("").trim_end();
            first.to_ascii_lowercase().ends_with(&exe_suffix.to_ascii_lowercase()).then(|| pid.to_string())
        })
        .collect()
}

pub fn running() -> bool {
    !pids("\\steam.exe").is_empty()
}

// steam's windows need wine's own d3d, which the web helper loads through wined3d.dll. once it is mapped
// the runner can switch to dxmt for the game, the helper keeps what it loaded
pub fn wait_for_helper_d3d(task: &Task, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline && !task.cancelled() {
        for pid in pids(HELPER_REAL) {
            let Ok(out) = Command::new("/usr/sbin/lsof").args(["-p", &pid, "-Fn"]).output() else { continue };
            if String::from_utf8_lossy(&out.stdout).lines().any(|l| l.ends_with("/wined3d.dll")) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}
