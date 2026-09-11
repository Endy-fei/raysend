pub const MAGIC: [u8; 2] = [b'Q', b'T'];
pub const TYPE_META: u8 = b'M';
pub const TYPE_DATA: u8 = b'D';

const META_MIN_LEN: usize = 2 + 1 + 4 + 20 + 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub name: String,
    pub orig_len: u32,
    pub hash: [u8; 20],
    pub oti: [u8; 12],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Meta(Meta),
    Data(Vec<u8>),
}

pub fn encode_meta(name: &str, orig_len: u32, hash: &[u8; 20], oti: &[u8; 12]) -> Vec<u8> {
    let mut name_bytes = name.as_bytes();
    if name_bytes.len() > 180 {
        name_bytes = &name_bytes[..180];
    }

    let mut out = Vec::with_capacity(META_MIN_LEN + name_bytes.len());
    out.extend_from_slice(&MAGIC);
    out.push(TYPE_META);
    out.extend_from_slice(&orig_len.to_be_bytes());
    out.extend_from_slice(hash);
    out.extend_from_slice(oti);
    out.extend_from_slice(name_bytes);
    out
}

pub fn encode_data(packet: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + packet.len());
    out.extend_from_slice(&MAGIC);
    out.push(TYPE_DATA);
    out.extend_from_slice(packet);
    out
}

pub fn parse_frame(bytes: &[u8]) -> Option<Frame> {
    if bytes.len() < 3 || bytes[0] != MAGIC[0] || bytes[1] != MAGIC[1] {
        return None;
    }

    match bytes[2] {
        TYPE_META => {
            if bytes.len() < META_MIN_LEN {
                return None;
            }
            let orig_len = u32::from_be_bytes(bytes[3..7].try_into().ok()?);
            let hash: [u8; 20] = bytes[7..27].try_into().ok()?;
            let oti: [u8; 12] = bytes[27..39].try_into().ok()?;
            let name = String::from_utf8_lossy(&bytes[39..]).into_owned();
            Some(Frame::Meta(Meta {
                name,
                orig_len,
                hash,
                oti,
            }))
        }
        TYPE_DATA => {
            if bytes.len() <= 3 {
                return None;
            }
            Some(Frame::Data(bytes[3..].to_vec()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_roundtrip() {
        let hash = [7u8; 20];
        let oti = [3u8; 12];
        let encoded = encode_meta("photo.png", 123456, &hash, &oti);
        match parse_frame(&encoded) {
            Some(Frame::Meta(meta)) => {
                assert_eq!(meta.name, "photo.png");
                assert_eq!(meta.orig_len, 123456);
                assert_eq!(meta.hash, hash);
                assert_eq!(meta.oti, oti);
            }
            _ => panic!("expected meta frame"),
        }
    }

    #[test]
    fn data_roundtrip() {
        let encoded = encode_data(&[1, 2, 3, 4]);
        match parse_frame(&encoded) {
            Some(Frame::Data(data)) => assert_eq!(data, vec![1, 2, 3, 4]),
            _ => panic!("expected data frame"),
        }
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_frame(b"hello").is_none());
        assert!(parse_frame(&[b'Q', b'T', 0x00]).is_none());
    }
}
