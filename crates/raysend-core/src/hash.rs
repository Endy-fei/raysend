//! SHA-256：传输完整性校验，不当作保密手段。

use sha2::{Digest, Sha256};

/// 计算 32 字节 SHA-256 摘要。
pub fn hash_bytes(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// 十六进制形式的 SHA-256，便于日志输出。
pub fn hash_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_empty() {
        let hex = hash_hex(b"");
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
