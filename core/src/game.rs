use std::path::PathBuf;

use serde::Serialize;

use crate::config::Config;
use crate::steam;
use crate::util::{free_space, Result, Task};

// wuthering waves runs as the steam release in the game bottle. steam downloads, updates and verifies it,
// this reports what steam's manifest says and opens steam on the right page

// the process the game runs as, the launcher exe steam starts hands off to it
pub const EXECUTABLE: &str = "Client-Win64-Shipping.exe";
pub const LAUNCHER: &str = "Wuthering Waves.exe";
// steam's figure for the full install plus the resources the game downloads on first start
const INSTALL_SIZE: u64 = 110 << 30;

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

// steam's build id, there is no version file outside the pak files
pub fn installed_version(cfg: &Config) -> Option<String> {
    let s = steam::app_state(&cfg.game_path())?;
    (s.fully_installed && s.install_dir.join(LAUNCHER).exists()).then(|| s.build_id.unwrap_or_else(|| "installed".into()))
}

pub fn state(cfg: &Config) -> Result<GameState> {
    let bottle = cfg.game_path();
    let app = steam::app_state(&bottle);
    let version = installed_version(cfg);
    let latest = version.clone().unwrap_or_default();
    let (to_download, downloaded) = app.as_ref().map(|a| (a.bytes_to_download, a.bytes_downloaded)).unwrap_or((0, 0));
    Ok(GameState {
        game_dir: bottle.to_string_lossy().into_owned(),
        installed: version.is_some(),
        version,
        latest,
        update_available: app.as_ref().is_some_and(|a| a.fully_installed && a.update_required),
        // steam applies updates itself when it starts
        can_update: true,
        predownload: None,
        predownload_ready: false,
        voice_packs: Vec::new(),
        download_size: to_download.saturating_sub(downloaded),
        install_size: app.as_ref().map(|a| a.size_on_disk).filter(|s| *s > 0).unwrap_or(INSTALL_SIZE),
        free_space: free_space(&bottle).unwrap_or(0),
        partial_download: app.as_ref().is_some_and(|a| !a.fully_installed),
        steam_installed: steam::installed(&bottle),
        steam_running: steam::running(),
    })
}

// sets up the bottle and steam, then opens steam on the game's install page. signing in and the download
// itself happen in steam's own window
pub fn install(task: &Task, cfg: &Config) -> Result<()> {
    crate::wine::ensure_steam(task, cfg)?;
    crate::wine::start_steam(task, cfg, &[format!("steam://install/{}", steam::APP_ID)])?;
    task.status("Sign in to Steam and install Wuthering Waves there");
    Ok(())
}

// steam updates the game when it starts, opening it on the library page is enough
pub fn update(task: &Task, cfg: &Config) -> Result<()> {
    crate::wine::ensure_steam(task, cfg)?;
    crate::wine::start_steam(task, cfg, &[format!("steam://nav/games/details/{}", steam::APP_ID)])?;
    task.status("Steam is updating Wuthering Waves");
    Ok(())
}

pub fn verify(task: &Task, cfg: &Config) -> Result<()> {
    crate::wine::ensure_steam(task, cfg)?;
    crate::wine::start_steam(task, cfg, &[format!("steam://validate/{}", steam::APP_ID)])?;
    task.status("Steam is verifying the game files");
    Ok(())
}
