use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}
impl Default for Endpoint {
    fn default() -> Self {
        Self { host: "127.0.0.1".to_string(), port: 5899 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ObsSettings {
    pub host: String,
    pub port: u16,
    pub password: String,
}
impl Default for ObsSettings {
    fn default() -> Self {
        Self { host: "127.0.0.1".to_string(), port: 4455, password: String::new() }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Participant {
    pub nickname: String,
    pub scene: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unique_identifier: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub teamspeak: Endpoint,
    pub obs: ObsSettings,
    pub participants: Vec<Participant>,
    pub default_scene: String,
    pub switch_delay_ms: u64,
    pub min_scene_ms: u64,
    pub silence_timeout_ms: u64,
    pub release_grace_ms: u64,
    pub same_channel_only: bool,
    pub log_events: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            teamspeak: Endpoint::default(),
            obs: ObsSettings::default(),
            participants: vec![
                Participant { nickname: "YourNickname".into(), scene: "Main".into(), unique_identifier: None },
                Participant { nickname: "FriendNickname".into(), scene: "Friend".into(), unique_identifier: None },
            ],
            default_scene: "Main".into(),
            switch_delay_ms: 500,
            min_scene_ms: 2500,
            silence_timeout_ms: 4500,
            release_grace_ms: 700,
            same_channel_only: true,
            log_events: false,
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        if self.default_scene.trim().is_empty() { return Err("Default scene must not be empty".into()); }
        if self.teamspeak.host.trim().is_empty() || self.obs.host.trim().is_empty() {
            return Err("Host names must not be empty".into());
        }
        if self.teamspeak.port == 0 || self.obs.port == 0 {
            return Err("Ports must be greater than zero".into());
        }
        if self.switch_delay_ms > 30000 || self.min_scene_ms > 120000 ||
            self.silence_timeout_ms > 120000 || self.release_grace_ms > 30000 {
            return Err("Timing parameter out of range".into());
        }
        for (i, person) in self.participants.iter().enumerate() {
            if person.nickname.trim().is_empty() && person.unique_identifier.as_deref().unwrap_or("").trim().is_empty() {
                return Err(format!("Participant {} needs a nickname or unique identifier", i + 1));
            }
            if person.scene.trim().is_empty() {
                return Err(format!("Participant {} needs a scene", i + 1));
            }
        }
        Ok(())
    }
}

pub fn base_dir() -> PathBuf {
    // For cargo run, work in the repository root; for packaged .exe, use its own directory.
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join("config.example.json").exists() { return cwd; }
    }
    std::env::current_exe().ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub fn load_or_initialize(dir: &Path) -> Result<Config, String> {
    let config_path = dir.join("config.json");
    if !config_path.exists() {
        let example = dir.join("config.example.json");
        if example.exists() {
            fs::copy(&example, &config_path).map_err(|e| format!("Copy sample config: {e}"))?;
        } else {
            save(&config_path, &Config::default())?;
        }
    }
    let contents = fs::read_to_string(&config_path).map_err(|e| format!("Read config: {e}"))?;
    let cfg: Config = serde_json::from_str(&contents).map_err(|e| format!("Invalid config.json: {e}"))?;
    cfg.validate()?;
    Ok(cfg)
}

pub fn save(path: &Path, cfg: &Config) -> Result<(), String> {
    cfg.validate()?;
    let content = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, content + "\n").map_err(|e| format!("Write settings: {e}"))?;
    // On Windows, rename cannot replace an existing file. A backup protects the old settings.
    if path.exists() {
        let backup = path.with_extension("json.bak");
        fs::copy(path, &backup).map_err(|e| format!("Backup settings: {e}"))?;
        fs::remove_file(path).map_err(|e| format!("Update settings: {e}"))?;
    }
    if let Err(e) = fs::rename(temp, path) {
        let backup = path.with_extension("json.bak");
        if backup.exists() { let _ = fs::copy(backup, path); }
        return Err(format!("Finalize settings: {e}"));
    }
    Ok(())
}
