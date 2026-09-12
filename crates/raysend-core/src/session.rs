//! 发送会话：选文件后打容器、建喷泉编码器、按格输出载荷。

use crate::format::format_bytes;
use crate::fountain::FountainSender;
use crate::protocol::encode_container;
use crate::qr::Density;

/// 单文件上限（MB）。密度上去后 20 MB 不再是硬瓶颈。
pub const MAX_FILE_SIZE_MB: u64 = 64;
/// 单文件上限（字节）。
pub const MAX_FILE_SIZE: u64 = MAX_FILE_SIZE_MB * 1024 * 1024;

/// 准备发送失败的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    /// 空文件不允许发送。
    Empty,
    /// 超出 [`MAX_FILE_SIZE`]，附带实际字节数。
    TooLarge(u64),
}

impl PrepareError {
    /// 给原生 UI 用的英文短句；网页端应走 i18n。
    pub fn message(&self) -> String {
        match self {
            PrepareError::Empty => "empty file".into(),
            PrepareError::TooLarge(size) => {
                format!(
                    "file too large: {} (max {} MB)",
                    format_bytes(*size),
                    MAX_FILE_SIZE_MB
                )
            }
        }
    }
}

fn new_session_id(seed: u64) -> u16 {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(seed);
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0),
    );
    hasher.finish() as u16
}

/// 一份已打包、可循环产出二维码载荷的发送会话。
pub struct Outgoing {
    /// 原始文件名。
    pub file_name: String,
    /// 压缩前字节数。
    pub orig_size: u64,
    /// 上屏字节数（容器），用于估算剩余时间。
    pub compressed_size: u64,
    /// 当前密度档。
    pub density: Density,
    session_id: u16,
    container: Vec<u8>,
    sender: FountainSender,
}

impl Outgoing {
    pub fn prepare(file_name: String, original: Vec<u8>) -> Result<Self, PrepareError> {
        Self::prepare_with(file_name, original, Density::Default)
    }

    pub fn prepare_with(
        file_name: String,
        original: Vec<u8>,
        density: Density,
    ) -> Result<Self, PrepareError> {
        if original.is_empty() {
            return Err(PrepareError::Empty);
        }
        if original.len() as u64 > MAX_FILE_SIZE {
            return Err(PrepareError::TooLarge(original.len() as u64));
        }
        let orig_size = original.len() as u64;
        let container = encode_container(&file_name, "", &original);
        let session_id = new_session_id(orig_size);
        Ok(Self::from_container(
            file_name,
            orig_size,
            container,
            density,
            session_id,
        ))
    }

    /// 已打好的容器直接建会话（网页端分步显示时使用）。
    pub fn from_container(
        file_name: String,
        orig_size: u64,
        container: Vec<u8>,
        density: Density,
        session_id: u16,
    ) -> Self {
        let compressed_size = container.len() as u64;
        let sender = FountainSender::new(session_id, &container, density.symbol_mtu());
        Self {
            file_name,
            orig_size,
            compressed_size,
            density,
            session_id,
            container,
            sender,
        }
    }

    pub fn set_density(&mut self, density: Density) {
        if self.density == density {
            return;
        }
        self.density = density;
        self.sender = FountainSender::new(self.session_id, &self.container, density.symbol_mtu());
    }

    pub fn symbol_mtu(&self) -> u16 {
        self.density.symbol_mtu()
    }

    /// 取出 `n` 个协议载荷，供 1×1 或 2×2 宫格同时绘制。
    pub fn next_payloads(&mut self, n: usize) -> Vec<Vec<u8>> {
        (0..n).map(|_| self.sender.next_payload()).collect()
    }

    /// RaptorQ 源块数。
    pub fn source_blocks(&self) -> u8 {
        self.sender.config().source_blocks()
    }

    /// 按 `符号大小 × fps × 宫格 × 0.7` 估算秒数。
    pub fn estimate_seconds(&self, fps: u32, grid: u8) -> u64 {
        let rate = self.symbol_mtu() as u64 * fps as u64 * grid as u64 * 7 / 10;
        self.compressed_size.div_ceil(rate.max(1))
    }
}
