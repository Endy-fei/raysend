//! `R2` 自描述帧：每帧带 OTI，容器里放文件名与 SHA-256。旧 `QT` 被拒绝。

use crate::compress::{compress_if_smaller, decompress};
use crate::hash::hash_bytes;

/// 帧头魔数 `R2`。
pub const MAGIC: [u8; 2] = [b'R', b'2'];
/// 线格式版本。
pub const WIRE_VERSION: u8 = 1;
/// 旧协议魔数，接收端据此提示升级。
pub const LEGACY_MAGIC: [u8; 2] = [b'Q', b'T'];
/// 固定头长度（含校验）。
pub const HEADER_LEN: usize = 28;
/// flags 低 4 位：未知则整帧丢弃。
pub const FLAG_MUST_MASK: u8 = 0x0F;

const CONTAINER_MAGIC: &[u8; 4] = b"RSc1";
const FLAG_COMPRESSED: u8 = 0x01;
const CONTAINER_MIN: usize = 4 + 1 + 4 + 32 + 2 + 2;

/// 一次传输的文件描述，从容器解出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// UTF-8 文件名，编码时最长 180 字节。
    pub name: String,
    /// 解压后的原始字节数。
    pub orig_len: u32,
    /// 原始字节的 SHA-256。
    pub hash: [u8; 32],
    /// MIME，可空。
    pub mime: String,
    /// RaptorQ Object Transmission Information。
    pub oti: [u8; 12],
    /// 发送端会话号。
    pub session_id: u16,
}

/// 一帧里除载荷外的字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    pub session_id: u16,
    pub seq: u32,
    pub oti: [u8; 12],
    pub container_len: u32,
}

/// 一帧二维码里的协议内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// RaptorQ 编码包（含 4 字节包头）。
    Data(FrameHeader, Vec<u8>),
    /// 旧 `QT` 协议。
    Legacy,
}

/// FNV-1a 折叠为 16 位，校验头前 26 字节。
pub fn header_checksum(bytes: &[u8]) -> u16 {
    let mut h: u32 = 2_166_136_261;
    for &b in bytes {
        h ^= u32::from(b);
        h = h.wrapping_mul(16_777_619);
    }
    (h ^ (h >> 16)) as u16
}

/// 把文件打成喷泉容器：可选 Brotli、SHA-256、文件名、MIME。
pub fn encode_container(name: &str, mime: &str, original: &[u8]) -> Vec<u8> {
    let hash = hash_bytes(original);
    let (payload, compressed) = compress_if_smaller(original);
    let mut name_bytes = name.as_bytes();
    if name_bytes.len() > 180 {
        name_bytes = &name_bytes[..180];
    }
    let mime_bytes = mime.as_bytes();
    let mime_len = mime_bytes.len().min(u16::MAX as usize) as u16;
    let mime_bytes = &mime_bytes[..mime_len as usize];

    let mut out = Vec::with_capacity(CONTAINER_MIN + name_bytes.len() + mime_bytes.len() + payload.len());
    out.extend_from_slice(CONTAINER_MAGIC);
    out.push(if compressed { FLAG_COMPRESSED } else { 0 });
    out.extend_from_slice(&(original.len() as u32).to_be_bytes());
    out.extend_from_slice(&hash);
    out.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(name_bytes);
    out.extend_from_slice(&mime_len.to_be_bytes());
    out.extend_from_slice(mime_bytes);
    out.extend_from_slice(&payload);
    out
}

/// 解析容器并还原原始文件。
pub fn decode_container(bytes: &[u8]) -> Result<(Meta, Vec<u8>), String> {
    if bytes.len() < CONTAINER_MIN || &bytes[..4] != CONTAINER_MAGIC {
        return Err("container".into());
    }
    let flags = bytes[4];
    let orig_len = u32::from_be_bytes(bytes[5..9].try_into().map_err(|_| "container")?);
    let hash: [u8; 32] = bytes[9..41].try_into().map_err(|_| "container")?;
    let name_len = u16::from_be_bytes(bytes[41..43].try_into().map_err(|_| "container")?) as usize;
    let name_end = 43 + name_len;
    if bytes.len() < name_end + 2 {
        return Err("container".into());
    }
    let name = String::from_utf8_lossy(&bytes[43..name_end]).into_owned();
    let mime_len = u16::from_be_bytes(bytes[name_end..name_end + 2].try_into().map_err(|_| "container")?) as usize;
    let mime_end = name_end + 2 + mime_len;
    if bytes.len() < mime_end {
        return Err("container".into());
    }
    let mime = String::from_utf8_lossy(&bytes[name_end + 2..mime_end]).into_owned();
    let payload = &bytes[mime_end..];
    let data = if flags & FLAG_COMPRESSED != 0 {
        decompress(payload)
    } else {
        payload.to_vec()
    };
    if data.len() as u32 != orig_len {
        return Err("size_mismatch".into());
    }
    if hash_bytes(&data) != hash {
        return Err("integrity".into());
    }
    Ok((
        Meta {
            name,
            orig_len,
            hash,
            mime,
            oti: [0; 12],
            session_id: 0,
        },
        data,
    ))
}

/// 编码一帧：头 + RaptorQ 包。
pub fn encode_data(
    session_id: u16,
    seq: u32,
    oti: &[u8; 12],
    container_len: u32,
    packet: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + packet.len());
    out.extend_from_slice(&MAGIC);
    out.push(WIRE_VERSION);
    out.push(0);
    out.extend_from_slice(&session_id.to_be_bytes());
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(oti);
    out.extend_from_slice(&container_len.to_be_bytes());
    let sum = header_checksum(&out);
    out.extend_from_slice(&sum.to_be_bytes());
    out.extend_from_slice(packet);
    out
}

/// 解析一帧。无法识别时返回 `None`；旧协议返回 [`Frame::Legacy`]。
pub fn parse_frame(bytes: &[u8]) -> Option<Frame> {
    if bytes.len() >= 2 && bytes[0] == LEGACY_MAGIC[0] && bytes[1] == LEGACY_MAGIC[1] {
        return Some(Frame::Legacy);
    }
    if bytes.len() < HEADER_LEN + 4 {
        return None;
    }
    if bytes[0] != MAGIC[0] || bytes[1] != MAGIC[1] {
        return None;
    }
    if bytes[2] != WIRE_VERSION {
        return None;
    }
    if bytes[3] & FLAG_MUST_MASK != 0 {
        return None;
    }
    let expect = header_checksum(&bytes[..26]);
    let got = u16::from_be_bytes(bytes[26..28].try_into().ok()?);
    if expect != got {
        return None;
    }
    let session_id = u16::from_be_bytes(bytes[4..6].try_into().ok()?);
    let seq = u32::from_be_bytes(bytes[6..10].try_into().ok()?);
    let oti: [u8; 12] = bytes[10..22].try_into().ok()?;
    let container_len = u32::from_be_bytes(bytes[22..26].try_into().ok()?);
    Some(Frame::Data(
        FrameHeader {
            session_id,
            seq,
            oti,
            container_len,
        },
        bytes[HEADER_LEN..].to_vec(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_roundtrip_text() {
        let raw = b"hello raysend v2".to_vec();
        let encoded = encode_container("note.txt", "text/plain", &raw);
        let (meta, data) = decode_container(&encoded).expect("container");
        assert_eq!(meta.name, "note.txt");
        assert_eq!(meta.mime, "text/plain");
        assert_eq!(data, raw);
        assert_eq!(meta.hash, hash_bytes(&raw));
    }

    #[test]
    fn container_keeps_incompressible() {
        let raw: Vec<u8> = (0..200u8).map(|i| i.wrapping_mul(91)).collect();
        let encoded = encode_container("x.bin", "", &raw);
        assert_eq!(encoded[4] & FLAG_COMPRESSED, 0);
        let (_, data) = decode_container(&encoded).unwrap();
        assert_eq!(data, raw);
    }

    #[test]
    fn frame_roundtrip() {
        let oti = [3u8; 12];
        let encoded = encode_data(7, 11, &oti, 99, &[1, 2, 3, 4]);
        match parse_frame(&encoded) {
            Some(Frame::Data(header, packet)) => {
                assert_eq!(header.session_id, 7);
                assert_eq!(header.seq, 11);
                assert_eq!(header.oti, oti);
                assert_eq!(header.container_len, 99);
                assert_eq!(packet, vec![1, 2, 3, 4]);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn rejects_legacy_qt() {
        assert!(matches!(parse_frame(b"QTM"), Some(Frame::Legacy)));
        assert!(parse_frame(b"hello").is_none());
    }

    #[test]
    fn rejects_bad_checksum() {
        let oti = [1u8; 12];
        let mut encoded = encode_data(1, 0, &oti, 1, &[9, 8, 7, 6]);
        encoded[26] ^= 0xFF;
        assert!(parse_frame(&encoded).is_none());
    }
}
