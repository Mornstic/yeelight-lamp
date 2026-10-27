//! miIO（小米局域网 UDP 协议）最小客户端，与 python-miio 的数据包格式一致。
//!
//! 一个数据包由三部分组成：
//! - 16 字节头：magic `0x2131`、总长度、保留位、设备 ID、设备时间戳
//! - 16 字节 MD5 校验：`MD5(header ‖ token ‖ 密文)`
//! - AES-128-CBC/PKCS7 密文，明文是 JSON 加一个 `\0` 结尾
//!
//! 密钥不直接使用 token：`key = MD5(token)`，`iv = MD5(key ‖ token)`。
//! token 只以字节形式留在内存里，Debug 输出会被遮蔽。

use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use md5::{Digest, Md5};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::time::{timeout, Instant};

pub const PORT: u16 = 54321;

const MAGIC: u16 = 0x2131;
const HEADER_LEN: usize = 16;
const CHECKSUM_LEN: usize = 16;
const OVERHEAD: usize = HEADER_LEN + CHECKSUM_LEN;
const MAX_PACKET: usize = 1500;

#[derive(Debug, Error)]
pub enum MiioError {
    #[error("网络错误：{0}")]
    Io(#[from] io::Error),
    #[error("设备 {0} 无响应")]
    Timeout(Ipv4Addr),
    #[error("响应包格式不正确")]
    Malformed,
    #[error("响应校验失败，token 可能不正确")]
    Checksum,
    #[error("无法解密或解析响应")]
    Payload,
    #[error("设备返回错误码 {0}")]
    Device(i64),
    #[error("token 必须是 32 个十六进制字符")]
    InvalidToken,
}

impl MiioError {
    /// 网络抖动、丢包一类可以靠重连恢复的错误。
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            MiioError::Io(_) | MiioError::Timeout(_) | MiioError::Malformed | MiioError::Payload
        )
    }
}

#[derive(Clone, Copy)]
pub struct Token([u8; 16]);

impl Token {
    pub fn from_hex(hex: &str) -> Result<Self, MiioError> {
        let bytes = hex.as_bytes();
        if bytes.len() != 32 {
            return Err(MiioError::InvalidToken);
        }
        let mut out = [0u8; 16];
        for (index, slot) in out.iter_mut().enumerate() {
            let hi = hex_digit(bytes[index * 2]).ok_or(MiioError::InvalidToken)?;
            let lo = hex_digit(bytes[index * 2 + 1]).ok_or(MiioError::InvalidToken)?;
            *slot = (hi << 4) | lo;
        }
        Ok(Self(out))
    }

    fn key_iv(self) -> ([u8; 16], [u8; 16]) {
        let key = md5_16(&self.0);
        let mut material = [0u8; 32];
        material[..16].copy_from_slice(&key);
        material[16..].copy_from_slice(&self.0);
        (key, md5_16(&material))
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn md5_16(data: &[u8]) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(data);
    let digest = hasher.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest);
    out
}

fn encrypt(plain: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>, MiioError> {
    let pad = 16 - plain.len() % 16;
    let mut buffer = Vec::with_capacity(plain.len() + pad);
    buffer.extend_from_slice(plain);
    buffer.resize(plain.len() + pad, pad as u8);

    cbc::Encryptor::<aes::Aes128>::new_from_slices(key, iv)
        .map_err(|_| MiioError::Payload)?
        .encrypt_padded_mut::<Pkcs7>(&mut buffer, plain.len())
        .map(|ciphertext| ciphertext.to_vec())
        .map_err(|_| MiioError::Payload)
}

fn decrypt(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>, MiioError> {
    let mut buffer = data.to_vec();
    cbc::Decryptor::<aes::Aes128>::new_from_slices(key, iv)
        .map_err(|_| MiioError::Payload)?
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map(<[u8]>::to_vec)
        .map_err(|_| MiioError::Payload)
}

fn hello_packet() -> [u8; 32] {
    let mut packet = [0xff_u8; 32];
    packet[..4].copy_from_slice(&[0x21, 0x31, 0x00, 0x20]);
    packet
}

#[derive(Debug, Clone, Copy)]
pub struct HelloInfo {
    pub ip: Ipv4Addr,
    pub device_id: u32,
    pub stamp: u32,
}

fn parse_hello(buf: &[u8], ip: Ipv4Addr) -> Result<HelloInfo, MiioError> {
    if buf.len() < 32 || u16::from_be_bytes([buf[0], buf[1]]) != MAGIC {
        return Err(MiioError::Malformed);
    }
    Ok(HelloInfo {
        ip,
        device_id: u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
        stamp: u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]),
    })
}

/// 单播握手：确认某个 IP 上是不是我们要的那盏灯。
pub async fn hello(ip: Ipv4Addr, wait: Duration) -> Result<HelloInfo, MiioError> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    socket.connect(SocketAddr::from((ip, PORT))).await?;
    socket.send(&hello_packet()).await?;

    let mut buf = [0u8; 64];
    let len = match timeout(wait, socket.recv(&mut buf)).await {
        Ok(result) => result?,
        Err(_) => return Err(MiioError::Timeout(ip)),
    };
    parse_hello(&buf[..len], ip)
}

/// 向热点网段和全网广播握手包，收集 `window` 时间内的应答。
pub async fn scan(window: Duration) -> Vec<HelloInfo> {
    let mut found: Vec<HelloInfo> = Vec::new();
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await {
        Ok(socket) => socket,
        Err(_) => return found,
    };
    if socket.set_broadcast(true).is_err() {
        return found;
    }

    let packet = hello_packet();
    for target in [Ipv4Addr::new(192, 168, 137, 255), Ipv4Addr::BROADCAST] {
        let _ = socket
            .send_to(&packet, SocketAddr::from((target, PORT)))
            .await;
    }

    let deadline = Instant::now() + window;
    let mut buf = [0u8; MAX_PACKET];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, SocketAddr::V4(addr)))) => {
                if let Ok(info) = parse_hello(&buf[..len], *addr.ip()) {
                    if !found.iter().any(|item| item.device_id == info.device_id) {
                        found.push(info);
                    }
                }
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) | Err(_) => break,
        }
    }
    found
}

pub struct MiioClient {
    ip: Ipv4Addr,
    token: Token,
    device_id: u32,
    stamp: u32,
    next_id: u32,
    wait: Duration,
    handshaken: bool,
}

impl MiioClient {
    pub fn new(ip: Ipv4Addr, token: Token, wait: Duration) -> Self {
        Self {
            ip,
            token,
            device_id: 0,
            stamp: 0,
            next_id: 0,
            wait,
            handshaken: false,
        }
    }

    pub fn ip(&self) -> Ipv4Addr {
        self.ip
    }

    pub fn set_ip(&mut self, ip: Ipv4Addr) {
        if ip != self.ip {
            self.ip = ip;
            self.handshaken = false;
        }
    }

    pub async fn handshake(&mut self) -> Result<HelloInfo, MiioError> {
        let info = hello(self.ip, self.wait).await?;
        self.device_id = info.device_id;
        self.stamp = info.stamp;
        self.handshaken = true;
        Ok(info)
    }

    /// 发送一条 miIO 指令；网络类错误会自动重新握手并重试一次。
    pub async fn send(&mut self, method: &str, params: Value) -> Result<Value, MiioError> {
        if !self.handshaken {
            self.handshake().await?;
        }
        match self.exchange(method, &params).await {
            Ok(value) => Ok(value),
            Err(err) if err.is_transient() => {
                self.handshaken = false;
                self.handshake().await?;
                self.exchange(method, &params).await
            }
            Err(err) => Err(err),
        }
    }

    async fn exchange(&mut self, method: &str, params: &Value) -> Result<Value, MiioError> {
        let id = self.take_id();
        let mut plain = serde_json::to_vec(&json!({
            "id": id,
            "method": method,
            "params": params,
        }))
        .map_err(|_| MiioError::Payload)?;
        plain.push(0);

        let (key, iv) = self.token.key_iv();
        let encrypted = encrypt(&plain, &key, &iv)?;

        let total = encrypted.len() + OVERHEAD;
        if total > u16::MAX as usize {
            return Err(MiioError::Payload);
        }

        let mut header = [0u8; HEADER_LEN];
        header[..2].copy_from_slice(&MAGIC.to_be_bytes());
        header[2..4].copy_from_slice(&(total as u16).to_be_bytes());
        header[8..12].copy_from_slice(&self.device_id.to_be_bytes());
        header[12..16].copy_from_slice(&self.stamp.wrapping_add(1).to_be_bytes());

        let mut checksum_input = Vec::with_capacity(HEADER_LEN + 16 + encrypted.len());
        checksum_input.extend_from_slice(&header);
        checksum_input.extend_from_slice(&self.token.0);
        checksum_input.extend_from_slice(&encrypted);
        let checksum = md5_16(&checksum_input);

        let mut packet = Vec::with_capacity(total);
        packet.extend_from_slice(&header);
        packet.extend_from_slice(&checksum);
        packet.extend_from_slice(&encrypted);

        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
        socket.connect(SocketAddr::from((self.ip, PORT))).await?;
        socket.send(&packet).await?;

        let mut buf = [0u8; MAX_PACKET];
        let len = match timeout(self.wait, socket.recv(&mut buf)).await {
            Ok(result) => result?,
            Err(_) => return Err(MiioError::Timeout(self.ip)),
        };
        let response = &buf[..len];
        if response.len() < OVERHEAD || u16::from_be_bytes([response[0], response[1]]) != MAGIC {
            return Err(MiioError::Malformed);
        }
        let declared = u16::from_be_bytes([response[2], response[3]]) as usize;
        if declared < OVERHEAD || declared > response.len() {
            return Err(MiioError::Malformed);
        }
        let response = &response[..declared];

        let (head, rest) = response.split_at(HEADER_LEN);
        let (checksum, data) = rest.split_at(CHECKSUM_LEN);

        let mut checksum_input = Vec::with_capacity(HEADER_LEN + 16 + data.len());
        checksum_input.extend_from_slice(head);
        checksum_input.extend_from_slice(&self.token.0);
        checksum_input.extend_from_slice(data);
        if md5_16(&checksum_input) != *checksum {
            return Err(MiioError::Checksum);
        }

        self.stamp = u32::from_be_bytes([head[12], head[13], head[14], head[15]]);

        let decrypted = decrypt(data, &key, &iv)?;
        let end = decrypted
            .iter()
            .rposition(|byte| *byte != 0)
            .map(|index| index + 1)
            .unwrap_or(0);
        let value: Value =
            serde_json::from_slice(&decrypted[..end]).map_err(|_| MiioError::Payload)?;

        if let Some(error) = value.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
            return Err(MiioError::Device(code));
        }
        Ok(value.get("result").cloned().unwrap_or(Value::Null))
    }

    fn take_id(&mut self) -> u32 {
        self.next_id += 1;
        if self.next_id >= 9999 {
            self.next_id = 1;
        }
        self.next_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_parsing_rejects_garbage() {
        assert!(Token::from_hex("00112233445566778899aabbccddeeff").is_ok());
        assert!(Token::from_hex("00112233445566778899aabbccddee").is_err());
        assert!(Token::from_hex("zz112233445566778899aabbccddeeff").is_err());
    }

    #[test]
    fn hello_packet_layout() {
        let packet = hello_packet();
        assert_eq!(&packet[..4], &[0x21, 0x31, 0x00, 0x20]);
        assert!(packet[4..].iter().all(|byte| *byte == 0xff));
    }

    #[test]
    fn matches_python_miio_crypto() {
        // 期望值由 python-miio 的 key_iv/AES 实现算出，保证与既有工具链互通
        let token = Token::from_hex("00112233445566778899aabbccddeeff").unwrap();
        let (key, iv) = token.key_iv();
        assert_eq!(hex(&key), "6e8311168ee16d6aa1aa48c64145003c");
        assert_eq!(hex(&iv), "6f434fa9acd75da73e5fb999f641cda2");

        let mut plain = br#"{"id":1,"method":"miIO.info","params":[]}"#.to_vec();
        plain.push(0);
        let encrypted = encrypt(&plain, &key, &iv).unwrap();
        assert_eq!(
            hex(&encrypted),
            "a5516ec6151955dc2bb2d43e7c84c18352a39a7f22fd2903d282a5783c6d56acd8b5de0116c0ebf35983dd023f849c97"
        );
        assert_eq!(decrypt(&encrypted, &key, &iv).unwrap(), plain);
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
