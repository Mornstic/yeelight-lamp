//! 读取 Python 一次性初始化脚本生成的设备配置。
//!
//! 配置位于 `%APPDATA%\yeelight-lamp\config.json`，包含 token 等机密，
//! 不进仓库、不写日志。发现灯的新 IP 时只回写 `ip` 字段，其余内容原样保留。

use std::env;
use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;

use crate::miio::Token;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("找不到配置文件 {0}，请先运行初始化脚本获取 token")]
    Missing(PathBuf),
    #[error("读写配置失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("配置不是合法 JSON：{0}")]
    Json(#[from] serde_json::Error),
    #[error("配置里的 {0} 无效")]
    Invalid(&'static str),
}

pub struct Config {
    path: PathBuf,
    raw: Value,
    pub name: String,
    pub model: String,
    pub token: Token,
    pub did: u64,
    pub ip: Option<Ipv4Addr>,
    pub mac: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(default_path())
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(ConfigError::Missing(path));
        }
        let text = fs::read_to_string(&path)?;
        Self::parse(path, &text)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn save_ip(&mut self, ip: Ipv4Addr) -> Result<(), ConfigError> {
        if self.ip == Some(ip) {
            return Ok(());
        }
        self.ip = Some(ip);
        self.raw["ip"] = Value::String(ip.to_string());

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = self.path.with_extension("json.tmp");
        fs::write(&temp, serde_json::to_string_pretty(&self.raw)?)?;
        fs::rename(&temp, &self.path)?;
        Ok(())
    }

    fn parse(path: PathBuf, text: &str) -> Result<Self, ConfigError> {
        let raw: Value = serde_json::from_str(text)?;

        let token = raw
            .get("token")
            .and_then(Value::as_str)
            .ok_or(ConfigError::Invalid("token"))
            .and_then(|hex| Token::from_hex(hex).map_err(|_| ConfigError::Invalid("token")))?;
        let did = parse_did(raw.get("did")).ok_or(ConfigError::Invalid("did"))?;

        Ok(Self {
            name: raw
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            model: raw
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            ip: raw
                .get("ip")
                .and_then(Value::as_str)
                .and_then(|text| text.parse().ok()),
            mac: raw
                .get("mac")
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase),
            token,
            did,
            path,
            raw,
        })
    }
}

pub fn default_path() -> PathBuf {
    if let Some(path) = env::var_os("YEELIGHT_LAMP_CONFIG") {
        return PathBuf::from(path);
    }
    let base = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("yeelight-lamp").join("config.json")
}

/// 初始化脚本可能把 did 写成字符串也可能写成数字，两种都接受。
fn parse_did(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::String(text) => text.trim().parse().ok(),
        Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("path", &self.path)
            .field("name", &self.name)
            .field("model", &self.model)
            .field("did", &self.did)
            .field("ip", &self.ip)
            .field("mac", &self.mac)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample(did: Value) -> String {
        json!({
            "name": "Yeelight 智能屏幕挂灯 2",
            "model": "yeelink.light.lamp43",
            "did": did,
            "token": "00112233445566778899aabbccddeeff",
            "ip": "192.168.137.24",
            "mac": "34:FA:1C:9E:99:CD"
        })
        .to_string()
    }

    #[test]
    fn accepts_string_and_number_did() {
        for did in [json!("2051293171"), json!(2051293171u64)] {
            let config = Config::parse(PathBuf::from("config.json"), &sample(did)).unwrap();
            assert_eq!(config.did, 2051293171);
            assert_eq!(config.ip, Some(Ipv4Addr::new(192, 168, 137, 24)));
            assert_eq!(config.mac.as_deref(), Some("34:fa:1c:9e:99:cd"));
        }
    }

    #[test]
    fn rejects_bad_token() {
        let mut raw: Value = serde_json::from_str(&sample(json!("2051293171"))).unwrap();
        raw["token"] = json!("not-a-token");
        assert!(Config::parse(PathBuf::from("config.json"), &raw.to_string()).is_err());
    }
}
