use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRecord {
    pub device_name: String,
    pub pubkey_b64: String,

    #[serde(default)]
    pub added_at_ms: u64, // 可选：以后 UI 排序/显示“何时配对”
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub device_id: String,
    pub device_name: String,
    pub listen_port: u16,
    
    #[serde(default)]
    pub trusted_peers: BTreeMap<String, PeerRecord>, // peer_device_id -> peer_pubkey_b64
}

pub struct AppState {
    pub dir: PathBuf,         // 配置目录
    pub config_path: PathBuf, // config.json
    pub key_path: PathBuf,    // identity.key（base64 seed）
    pub config: AppConfig,
    pub identity: uniclip_crypto::Identity,
}

/// 获取配置目录（跨平台）
/// uniclip / daemon 作为组织名+应用名
fn config_dir() -> Result<PathBuf> {
    let proj = ProjectDirs::from("com", "uniclip", "uniclip")
        .ok_or_else(|| anyhow!("cannot determine config directory"))?;
    Ok(proj.config_dir().to_path_buf())
}

fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    Ok(())
}

fn load_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let s = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&s)?)
}

// TODO: 现在是 fs::write，如果进程崩溃可能写一半
// 改为：写到 config.json.tmp + fsync + rename 覆盖
fn save_json_pretty<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    let s = serde_json::to_string_pretty(v)?;
    fs::write(path, s)?;
    Ok(())
}

/// 初始化：加载 config + identity；不存在则创建
pub fn init_or_create(default_listen_port: u16) -> Result<AppState> {
    let dir = config_dir()?;
    ensure_dir(&dir)?;

    let config_path = dir.join("config.json");
    let key_path = dir.join("identity.key");

    // 1) identity：优先加载，否则生成并保存
    let identity = if key_path.exists() {
        uniclip_crypto::Identity::load_from_file(&key_path)?
    } else {
        let id = uniclip_crypto::Identity::generate();
        id.save_to_file(&key_path)?;
        id
    };

    // 2) config：优先加载，否则创建并保存
    let config: AppConfig = if config_path.exists() {
        load_json(&config_path)?
    } else {
        let cfg = AppConfig {
            device_id: uuid::Uuid::new_v4().to_string(),
            device_name: default_device_name(),
            listen_port: default_listen_port,
            trusted_peers: BTreeMap::new(),
        };
        save_json_pretty(&config_path, &cfg)?;
        cfg
    };

    Ok(AppState {
        dir,
        config_path,
        key_path,
        config,
        identity,
    })
}

/// 设备名
fn default_device_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "uniclip-device".to_string())
}

impl AppState {
    pub fn save_config(&self) -> Result<()> {
        let s = serde_json::to_string_pretty(&self.config)?;
        std::fs::write(&self.config_path, s)?;
        Ok(())
    }
}