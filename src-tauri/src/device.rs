//! Yeelight 屏幕挂灯 2（`yeelink.light.lamp43`）的 MIoT 语义层。
//!
//! 负责找灯（配置 IP → ARP 表 → UDP 广播）、读取五项常用属性并设置
//! 开关/亮度/色温/情景模式/自动补光。灯在关闭状态会丢弃亮度、色温写入，
//! 所以调用方要先开灯再改值。

use std::net::Ipv4Addr;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use thiserror::Error;

use crate::config::{Config, ConfigError};
use crate::miio::{self, MiioClient, MiioError};

pub const SIID_LIGHT: u8 = 2;
pub const PIID_POWER: u8 = 1;
pub const PIID_BRIGHTNESS: u8 = 2;
pub const PIID_COLOR_TEMP: u8 = 3;
pub const PIID_MODE: u8 = 7;
pub const SIID_CUSTOM: u8 = 4;
pub const PIID_AUTO_FILL: u8 = 1;

pub const BRIGHTNESS_MIN: u8 = 1;
pub const BRIGHTNESS_MAX: u8 = 100;
pub const COLOR_TEMP_MIN: u16 = 2700;
pub const COLOR_TEMP_MAX: u16 = 6500;

const CONNECT_WAIT: Duration = Duration::from_millis(900);
const CALL_WAIT: Duration = Duration::from_millis(1600);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub mode: u8,
    pub brightness: u8,
    pub color_temp: u16,
}

/// 与米家/ Yeelight 固件内置情景一致的四个档位，写 piid 7 直接切场景。
pub const PRESETS: [Preset; 4] = [
    Preset {
        id: "office",
        label: "办公",
        mode: 1,
        brightness: 100,
        color_temp: 6000,
    },
    Preset {
        id: "focus",
        label: "专注",
        mode: 2,
        brightness: 80,
        color_temp: 5000,
    },
    Preset {
        id: "read",
        label: "阅读",
        mode: 3,
        brightness: 100,
        color_temp: 4000,
    },
    Preset {
        id: "relax",
        label: "休闲",
        mode: 4,
        brightness: 30,
        color_temp: 2700,
    },
];

pub fn preset_by_id(id: &str) -> Option<Preset> {
    PRESETS
        .iter()
        .copied()
        .find(|preset| preset.id == id || preset.label == id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub power: bool,
    pub brightness: u8,
    pub color_temp: u16,
    pub mode: u8,
    pub auto_fill: bool,
}

#[derive(Debug, Error)]
pub enum LampError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Miio(#[from] MiioError),
    #[error("局域网内找不到灯（请确认热点已开启、灯已上电）")]
    NotFound,
    #[error("响应的设备标识与配置不一致")]
    WrongDevice,
    #[error("设备返回异常：{0}")]
    Protocol(String),
}

pub struct Lamp {
    config: Config,
    client: Option<MiioClient>,
}

impl Lamp {
    pub fn load() -> Result<Self, LampError> {
        Ok(Self::new(Config::load()?))
    }

    pub fn new(config: Config) -> Self {
        Self {
            config,
            client: None,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn ip(&self) -> Option<Ipv4Addr> {
        self.client.as_ref().map(MiioClient::ip).or(self.config.ip)
    }

    /// 找到一个能握手、且设备 ID 对得上的 IP，并建立会话。
    pub async fn connect(&mut self) -> Result<Ipv4Addr, LampError> {
        let ip = self.resolve_ip().await?;
        let mut client = MiioClient::new(ip, self.config.token, CALL_WAIT);
        let info = client.handshake().await?;
        if u64::from(info.device_id) != self.config.did {
            return Err(LampError::WrongDevice);
        }
        self.config.save_ip(ip)?;
        self.client = Some(client);
        Ok(ip)
    }

    async fn resolve_ip(&mut self) -> Result<Ipv4Addr, LampError> {
        if let Some(ip) = self.config.ip {
            if hello_matches(ip, self.config.did).await {
                return Ok(ip);
            }
        }

        if let Some(mac) = self.config.mac.as_deref() {
            if let Some(ip) = arp_lookup(mac) {
                if hello_matches(ip, self.config.did).await {
                    return Ok(ip);
                }
            }
        }

        if let Some(info) = miio::scan(Duration::from_millis(1400))
            .await
            .into_iter()
            .find(|hello| u64::from(hello.device_id) == self.config.did)
        {
            return Ok(info.ip);
        }

        Err(LampError::NotFound)
    }

    pub async fn status(&mut self) -> Result<Status, LampError> {
        let did = self.config.did.to_string();
        let params: Vec<Value> = [
            (SIID_LIGHT, PIID_POWER),
            (SIID_LIGHT, PIID_BRIGHTNESS),
            (SIID_LIGHT, PIID_COLOR_TEMP),
            (SIID_LIGHT, PIID_MODE),
            (SIID_CUSTOM, PIID_AUTO_FILL),
        ]
        .into_iter()
        .map(|(siid, piid)| json!({ "did": did, "siid": siid, "piid": piid }))
        .collect();

        let result = self.call("get_properties", Value::Array(params)).await?;
        let items = result
            .as_array()
            .ok_or_else(|| LampError::Protocol("属性响应不是数组".into()))?;
        if items.len() < 5 {
            return Err(LampError::Protocol("属性响应缺少字段".into()));
        }

        let mut values = Vec::with_capacity(5);
        for item in &items[..5] {
            let code = item.get("code").and_then(Value::as_i64).unwrap_or(-1);
            if code != 0 {
                return Err(LampError::Protocol(format!("读取属性失败（code {code}）")));
            }
            values.push(item.get("value").cloned().unwrap_or(Value::Null));
        }

        Ok(Status {
            power: values[0].as_bool().unwrap_or(false),
            brightness: clamp_u8(&values[1], BRIGHTNESS_MIN, BRIGHTNESS_MAX),
            color_temp: clamp_u16(&values[2], COLOR_TEMP_MIN, COLOR_TEMP_MAX),
            mode: values[3].as_u64().unwrap_or(0).min(4) as u8,
            auto_fill: values[4].as_bool().unwrap_or(false),
        })
    }

    pub async fn set_power(&mut self, on: bool) -> Result<(), LampError> {
        self.set_property(SIID_LIGHT, PIID_POWER, json!(on)).await
    }

    pub async fn set_brightness(&mut self, value: u8) -> Result<(), LampError> {
        let value = value.clamp(BRIGHTNESS_MIN, BRIGHTNESS_MAX);
        self.set_property(SIID_LIGHT, PIID_BRIGHTNESS, json!(value))
            .await
    }

    pub async fn set_color_temp(&mut self, value: u16) -> Result<(), LampError> {
        let value = value.clamp(COLOR_TEMP_MIN, COLOR_TEMP_MAX);
        self.set_property(SIID_LIGHT, PIID_COLOR_TEMP, json!(value))
            .await
    }

    pub async fn set_mode(&mut self, mode: u8) -> Result<(), LampError> {
        self.set_property(SIID_LIGHT, PIID_MODE, json!(mode.min(4)))
            .await
    }

    pub async fn set_auto_fill(&mut self, on: bool) -> Result<(), LampError> {
        self.set_property(SIID_CUSTOM, PIID_AUTO_FILL, json!(on))
            .await
    }

    /// 把亮度/色温调到目标值；灯是关的就先开灯，否则写入会被固件丢弃。
    pub async fn apply_lighting(
        &mut self,
        brightness: u8,
        color_temp: u16,
    ) -> Result<(), LampError> {
        let current = self.status().await?;
        if !current.power {
            self.set_power(true).await?;
        }
        self.set_brightness(brightness).await?;
        self.set_color_temp(color_temp).await
    }

    pub async fn apply_preset(&mut self, preset: Preset) -> Result<(), LampError> {
        let current = self.status().await?;
        if !current.power {
            self.set_power(true).await?;
        }
        self.set_mode(preset.mode).await
    }

    /// 丢弃当前会话，下次调用会重新找灯并握手。
    pub fn invalidate(&mut self) {
        self.client = None;
    }

    /// 排障入口：发一条任意 miIO 指令。
    pub async fn raw_command(&mut self, method: &str, params: Value) -> Result<Value, LampError> {
        self.call(method, params).await
    }

    /// 测量一次状态读取的往返耗时，用于面板上的延迟显示。
    pub async fn status_with_latency(&mut self) -> Result<(Status, u32), LampError> {
        let started = Instant::now();
        let status = self.status().await?;
        Ok((status, started.elapsed().as_millis() as u32))
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value, LampError> {
        for attempt in 0..2 {
            if self.client.is_none() {
                self.connect().await?;
            }
            let client = self.client.as_mut().expect("client 刚刚建立");
            match client.send(method, params.clone()).await {
                Ok(value) => return Ok(value),
                Err(err) if err.is_transient() && attempt == 0 => {
                    self.client = None;
                }
                Err(err) => return Err(err.into()),
            }
        }
        Err(LampError::Protocol("设备连接反复失败".into()))
    }

    async fn set_property(&mut self, siid: u8, piid: u8, value: Value) -> Result<(), LampError> {
        let did = self.config.did.to_string();
        let result = self
            .call(
                "set_properties",
                json!([{ "did": did, "siid": siid, "piid": piid, "value": value }]),
            )
            .await?;
        match result.as_array().and_then(|items| items.first()) {
            Some(item) => {
                let code = item.get("code").and_then(Value::as_i64).unwrap_or(-1);
                if code == 0 {
                    Ok(())
                } else {
                    Err(LampError::Protocol(format!("写入属性失败（code {code}）")))
                }
            }
            None => Err(LampError::Protocol("写入属性没有响应体".into())),
        }
    }
}

async fn hello_matches(ip: Ipv4Addr, did: u64) -> bool {
    match miio::hello(ip, CONNECT_WAIT).await {
        Ok(info) => u64::from(info.device_id) == did,
        Err(_) => false,
    }
}

/// 通过 `arp -a` 按 MAC 找回 DHCP 换掉的 IP。中文系统输出是 GBK，
/// 但 MAC 和 IP 都是 ASCII，按行做字节级匹配即可。
fn arp_lookup(mac: &str) -> Option<Ipv4Addr> {
    let wanted = mac.to_ascii_lowercase().replace(':', "-");
    let mut command = Command::new("arp");
    command.arg("-a");
    // GUI 进程没有控制台；不隐藏会每轮重连都闪出一个 arp.exe 窗口。
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);

    text.lines()
        .filter(|line| line.to_ascii_lowercase().contains(&wanted))
        .find_map(|line| {
            line.split_whitespace()
                .find_map(|token| token.parse::<Ipv4Addr>().ok())
        })
}

fn clamp_u8(value: &Value, min: u8, max: u8) -> u8 {
    value
        .as_u64()
        .map(|number| number.clamp(u64::from(min), u64::from(max)) as u8)
        .unwrap_or(min)
}

fn clamp_u16(value: &Value, min: u16, max: u16) -> u16 {
    value
        .as_u64()
        .map(|number| number.clamp(u64::from(min), u64::from(max)) as u16)
        .unwrap_or(min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_addressed_by_id_or_label() {
        assert_eq!(preset_by_id("relax").map(|p| p.mode), Some(4));
        assert_eq!(preset_by_id("办公").map(|p| p.color_temp), Some(6000));
        assert!(preset_by_id("nope").is_none());
    }

    #[test]
    fn clamps_out_of_range_values() {
        assert_eq!(clamp_u8(&json!(0), 1, 100), 1);
        assert_eq!(clamp_u8(&json!(255), 1, 100), 100);
        assert_eq!(clamp_u8(&Value::Null, 1, 100), 1);
        assert_eq!(clamp_u16(&json!(9000), 2700, 6500), 6500);
        assert_eq!(clamp_u16(&json!(3000), 2700, 6500), 3000);
    }
}
