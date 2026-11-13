use std::path::PathBuf;

use serde::Serialize;

use crate::config::Config;
use crate::kuro;
use crate::steam;
use crate::util::{free_space, Result, Task};

// wuthering waves is the steam channel's build inside the game bottle. steam itself only ships kuro's
// launcher since build 26373309, so the game files come from kuro's download service (see kuro.rs) and
// land in the folder steam uses for the game

// the process the game runs as, the launcher exe hands off to it
pub const EXECUTABLE: &str = "Client-Win64-Shipping.exe";
pub const LAUNCHER: &str = "Wuthering Waves.exe";
const EXECUTABLE_PATH: &str = "Client/Binaries/Win64/Client-Win64-Shipping.exe";
// the hd install, for when kuro's index can't be reached
const INSTALL_SIZE: u64 = 89 << 30;

#[derive(Serialize, Debug)]
pub struct GameState {
    pub game_dir: String,
    pub installed: bool,
    pub version: Option<String>,
    pub latest: String,
    pub update_available: bool,
    pub can_update: bool,
    pub predownload: Option<String>,
    pub predownload_ready: bool,
    pub voice_packs: Vec<String>,
    pub download_size: u64,
    pub install_size: u64,
    pub free_space: u64,
    pub partial_download: bool,
    pub steam_installed: bool,
    pub steam_running: bool,
}

// the game's own folder inside steam's library in the bottle
pub fn game_dir(cfg: &Config) -> PathBuf {
    let bottle = cfg.game_path();
    steam::app_state(&bottle).map(|s| s.install_dir).unwrap_or_else(|| steam::dir(&bottle).join("steamapps/common/Wuthering Waves"))
}

// the version of the last finished download, as long as the game's files are still there
pub fn installed_version(cfg: &Config) -> Option<String> {
    let i = kuro::installed();
    let dir = game_dir(cfg);
    (i.complete && !i.version.is_empty() && dir.join(LAUNCHER).exists() && dir.join(EXECUTABLE_PATH).exists()).then_some(i.version)
}

pub fn state(cfg: &Config) -> Result<GameState> {
    let bottle = cfg.game_path();
    let dir = game_dir(cfg);
    let version = installed_version(cfg);
    let record = kuro::installed();
    // offline the launcher still shows what is installed
    let index = kuro::index(false).ok();
    let latest = index.as_ref().map(|i| i.version.clone()).or_else(|| version.clone()).unwrap_or_default();
    let total = index.as_ref().map(|i| i.total()).unwrap_or(INSTALL_SIZE);
    let download_size = index.as_ref().map(|i| kuro::remaining(i, &dir, &record)).unwrap_or(if version.is_some() { 0 } else { INSTALL_SIZE });
    Ok(GameState {
        game_dir: bottle.to_string_lossy().into_owned(),
        installed: version.is_some(),
        update_available: version.as_ref().is_some_and(|v| !latest.is_empty() && *v != latest),
        version,
        latest,
        can_update: true,
        predownload: None,
        predownload_ready: false,
        voice_packs: Vec::new(),
        download_size,
        install_size: total,
        free_space: free_space(&bottle).unwrap_or(0),
        // a download that was started and not finished
        partial_download: !record.complete && !record.verified.is_empty(),
        steam_installed: steam::installed(&bottle),
        steam_running: steam::running(),
    })
}

// the game sits in the bottle next to steam, which it signs in through, so both get set up first
pub fn install(task: &Task, cfg: &Config) -> Result<()> {
    crate::wine::ensure_steam(task, cfg)?;
    kuro::sync(task, &game_dir(cfg), false)
}

// an update is the same pass: files whose checksum changed are fetched again, the rest stay
pub fn update(task: &Task, cfg: &Config) -> Result<()> {
    kuro::sync(task, &game_dir(cfg), false)
}

// hashes every file again and replaces the ones that are wrong
pub fn verify(task: &Task, cfg: &Config) -> Result<()> {
    kuro::sync(task, &game_dir(cfg), true)
}
