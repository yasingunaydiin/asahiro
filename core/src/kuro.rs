use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::agent;
use crate::download::{self, Meter};
use crate::util::{data_dir, err, human_size, md5_file, Result, Task};

// kuro's own download service, the one its launcher reads. steam only ships that launcher since build
// 26373309 and its window stays blank under wine, so the files are fetched from the same lists directly.
// this is the steam channel's index: the build signs in through steam like the old depot did

const INDEX_URL: &str = "https://prod-alicdn-gamestarter.kurogame.com/launcher/game/50019_OvWd4dYrxkUbVcxfVb3QztlYCOc2xNuc/G153/steam/index.json";
// the quality pack that goes next to the shared one. hd is what wwmi mods are built against and what
// xxmi passes as -krqlv
pub const TIER: &str = "hd";
const PACKS: [&str; 2] = ["common", TIER];

#[derive(Clone, Debug)]
pub struct File {
    pub dest: String,
    pub md5: String,
    pub size: u64,
    // path below the cdn host
    pub url_path: String,
}

#[derive(Clone, Debug)]
pub struct Index {
    pub version: String,
    pub cdns: Vec<String>,
    pub files: Vec<File>,
}

impl Index {
    pub fn total(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

// what has been installed so far. the md5 of every file that was downloaded or checked is kept here
// rather than as marker files inside the game's folder
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Installed {
    pub version: String,
    pub tier: String,
    pub complete: bool,
    pub verified: BTreeMap<String, String>,
}

fn installed_path() -> PathBuf {
    data_dir().join("game-install.json")
}

pub fn installed() -> Installed {
    load_installed(&installed_path())
}

fn load_installed(path: &Path) -> Installed {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_installed(path: &Path, i: &Installed) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(i)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

// the lists come gzipped, with or without the header that makes the http client unpack them
fn get_json(url: &str) -> Result<Value> {
    let mut raw = Vec::new();
    agent().get(url).call()?.into_reader().take(64 << 20).read_to_end(&mut raw)?;
    if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(raw.as_slice()).read_to_end(&mut out)?;
        raw = out;
    }
    Ok(serde_json::from_slice(&raw)?)
}

fn join(base: &str, rest: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), rest.trim_start_matches('/'))
}

// a dest is written below the game folder, so it has to stay inside it
fn safe_dest(dest: &str) -> bool {
    !dest.is_empty()
        && !dest.starts_with('/')
        && !dest.contains('\\')
        && !dest.contains(':')
        && dest.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}

static INDEX: Mutex<Option<(Instant, Index)>> = Mutex::new(None);

pub fn index(force: bool) -> Result<Index> {
    let mut cached = INDEX.lock().unwrap();
    if !force {
        if let Some((at, i)) = cached.as_ref() {
            if at.elapsed() < Duration::from_secs(300) {
                return Ok(i.clone());
            }
        }
    }
    let root = get_json(INDEX_URL)?;
    // the launcher's priority figure, highest first
    let mut cdns: Vec<(i64, String)> = root["cdnList"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| Some((c["P"].as_i64().unwrap_or(0), c["url"].as_str()?.to_string()))).collect())
        .unwrap_or_default();
    cdns.sort_by(|a, b| b.0.cmp(&a.0));
    let cdns: Vec<String> = cdns.into_iter().map(|c| c.1).filter(|u| u.starts_with("https://")).collect();
    if cdns.is_empty() {
        return err("kuro's index lists no download servers");
    }

    let mut version = String::new();
    let mut files: BTreeMap<String, File> = BTreeMap::new();
    for name in PACKS {
        let pack = &root["resourcePacks"][name];
        let (Some(list), Some(base)) = (pack["indexFile"].as_str(), pack["baseUrl"].as_str()) else {
            return err(format!("kuro's index has no {} pack", name));
        };
        if name == "common" {
            version = pack["version"].as_str().unwrap_or("").to_string();
        }
        let listing = get_json(&join(&cdns[0], list))?;
        let Some(resources) = listing["resource"].as_array() else {
            return err(format!("the {} file list is empty", name));
        };
        for r in resources {
            let (Some(dest), Some(md5), Some(size)) = (r["dest"].as_str(), r["md5"].as_str(), r["size"].as_u64()) else {
                return err(format!("the {} file list has an entry without a path, checksum or size", name));
            };
            let dest = dest.trim_start_matches('/').to_string();
            if !safe_dest(&dest) {
                return err(format!("the {} file list has an unsafe path: {}", name, dest));
            }
            let url_path = join(base, &dest);
            files.insert(dest.clone(), File { dest, md5: md5.to_ascii_lowercase(), size, url_path });
        }
    }
    if version.is_empty() || files.is_empty() {
        return err("kuro's index is missing the game version or its files");
    }
    let i = Index { version, cdns, files: files.into_values().collect() };
    *cached = Some((Instant::now(), i.clone()));
    Ok(i)
}

// bytes still to fetch, going by what is recorded and present. no hashing, this runs for the ui
pub fn remaining(index: &Index, root: &Path, state: &Installed) -> u64 {
    index.files.iter().filter(|f| !recorded(f, root, state)).map(|f| f.size).sum()
}

fn recorded(f: &File, root: &Path, state: &Installed) -> bool {
    state.verified.get(&f.dest).is_some_and(|m| *m == f.md5)
        && std::fs::metadata(root.join(&f.dest)).is_ok_and(|m| m.len() == f.size)
}

fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".asahiro-part");
    PathBuf::from(s)
}

// downloads whatever is missing or different below `root`. a file already there with the right size is
// hashed once and kept when it matches, so an interrupted run, an update and a repair are all this same
// pass. with `recheck` nothing recorded is trusted and every file is hashed again
pub fn sync(task: &Task, root: &Path, recheck: bool) -> Result<()> {
    task.phase("download");
    task.status("Reading the file list");
    task.progress(None, "");
    let index = index(true)?;
    sync_index(task, root, &index, &installed_path(), recheck)
}

fn sync_index(task: &Task, root: &Path, index: &Index, record: &Path, recheck: bool) -> Result<()> {
    let mut state = load_installed(record);
    if recheck || state.tier != TIER {
        state.verified.clear();
    }
    state.tier = TIER.into();
    state.complete = false;
    save_installed(record, &state)?;

    let need: u64 = remaining(index, root, &state);
    if let Some(free) = crate::util::free_space(root) {
        // files already on disk with the right size are counted as needed here, so this errs on the safe side
        let on_disk: u64 = index
            .files
            .iter()
            .filter(|f| !recorded(f, root, &state))
            .filter_map(|f| std::fs::metadata(root.join(&f.dest)).ok().map(|m| m.len().min(f.size)))
            .sum();
        let missing = need.saturating_sub(on_disk);
        if free < missing + (2 << 30) {
            return err(format!("not enough free space: {} needed, {} free", human_size(missing), human_size(free)));
        }
    }

    let mut meter = Meter::new(task, "Downloading", index.total());
    let mut cdn = 0;
    for f in &index.files {
        task.check()?;
        let dest = root.join(&f.dest);
        if recorded(f, root, &state) {
            meter.add(f.size);
            continue;
        }
        let name = f.dest.rsplit('/').next().unwrap_or(&f.dest);
        // left over from an earlier install or run: keep it when its checksum is right
        if std::fs::metadata(&dest).is_ok_and(|m| m.len() == f.size) {
            task.phase("verify");
            task.status(format!("Checking {}", name));
            let before = meter.done;
            let got = md5_file(&dest, |n| {
                meter.add(n);
                task.check()
            })?;
            if got.eq_ignore_ascii_case(&f.md5) {
                state.verified.insert(f.dest.clone(), f.md5.clone());
                save_installed(record, &state)?;
                continue;
            }
            meter.done = before;
        }

        let part = part_path(&dest);
        let before = meter.done;
        let mut tries = 0;
        loop {
            let url = join(&index.cdns[cdn % index.cdns.len()], &f.url_path);
            match download::fetch_verified(&mut meter, &url, &part, f.size, &f.md5) {
                Ok(()) => break,
                Err(e) if e.0 == "cancelled" => return Err(e),
                // the next server gets a go before giving up
                Err(e) => {
                    meter.done = before;
                    tries += 1;
                    task.log(format!("{} failed on server {}: {}", f.dest, cdn % index.cdns.len() + 1, e));
                    if tries >= index.cdns.len().max(2) {
                        return Err(e);
                    }
                    cdn += 1;
                }
            }
        }
        std::fs::rename(&part, &dest)?;
        state.verified.insert(f.dest.clone(), f.md5.clone());
        save_installed(record, &state)?;
    }

    // entries for files the game no longer has would only grow the record
    let wanted: std::collections::BTreeSet<&str> = index.files.iter().map(|f| f.dest.as_str()).collect();
    state.verified.retain(|k, _| wanted.contains(k.as_str()));
    state.version = index.version.clone();
    state.complete = true;
    save_installed(record, &state)?;
    task.progress(Some(1.0), "");
    task.status(format!("Wuthering Waves {} is installed", index.version));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dest_stays_inside_the_game_folder() {
        assert!(safe_dest("Client/Binaries/Win64/Client-Win64-Shipping.exe"));
        assert!(safe_dest("Wuthering Waves.exe"));
        for bad in ["", "/etc/passwd", "../x", "Client/../../x", "Client//x", "C:/x", "Client\\x", "Client/./x"] {
            assert!(!safe_dest(bad), "{}", bad);
        }
    }

    // downloads a handful of small real files into a temp folder: cargo test -- --ignored live
    #[test]
    #[ignore]
    fn live_sync_of_a_few_small_files() {
        let mut idx = index(true).unwrap();
        println!("version {} / {} files / {} / cdns {:?}", idx.version, idx.files.len(), human_size(idx.total()), idx.cdns);
        assert!(idx.files.iter().any(|f| f.dest == "Client/Binaries/Win64/Client-Win64-Shipping.exe"));
        let mut few: Vec<File> = idx.files.iter().filter(|f| f.size < 300_000 && f.size > 0).take(4).cloned().collect();
        few.extend(idx.files.iter().filter(|f| f.dest == "Wuthering Waves.exe").cloned());
        idx.files = few;
        let root = std::env::temp_dir().join(format!("asahiro-kuro-test-{}", std::process::id()));
        let record = root.join("record.json");
        std::fs::create_dir_all(&root).unwrap();
        let task = || {
            Task::new(std::sync::Arc::new(|_| {}), Default::default(), Default::default())
        };
        sync_index(&task(), &root, &idx, &record, false).unwrap();
        for f in &idx.files {
            assert_eq!(md5_file(&root.join(&f.dest), |_| Ok(())).unwrap(), f.md5, "{}", f.dest);
            assert!(!part_path(&root.join(&f.dest)).exists());
        }
        let st = load_installed(&record);
        assert!(st.complete && st.version == idx.version && st.verified.len() == idx.files.len());
        assert_eq!(remaining(&idx, &root, &st), 0);

        // a damaged file is noticed by a recheck and replaced, a deleted record entry is re-hashed, not re-fetched
        let victim = root.join(&idx.files[0].dest);
        let mut bytes = std::fs::read(&victim).unwrap();
        bytes[0] ^= 0xff;
        std::fs::write(&victim, bytes).unwrap();
        sync_index(&task(), &root, &idx, &record, true).unwrap();
        assert_eq!(md5_file(&victim, |_| Ok(())).unwrap(), idx.files[0].md5);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn urls_join_with_one_slash() {
        assert_eq!(join("https://a/", "/b/c"), "https://a/b/c");
        assert_eq!(join("https://a", "b"), "https://a/b");
        assert_eq!(join("x/zip/", "Client/a.pak"), "x/zip/Client/a.pak");
    }
}
