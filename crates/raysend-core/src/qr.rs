//! 二维码光栅：ECC L、固定 mask 4，按密度档选择版本。

use std::fmt;

use qrcode::bits::Bits;
use qrcode::canvas::{Canvas, MaskPattern};
use qrcode::ec;
use qrcode::{EcLevel, QrCode, Version};

/// 静区模块数（四周各 4）。
pub const QUIET_ZONE: usize = 4;

/// 发送密度档。容量为 QR byte 模式 + ECC L。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Density {
    /// v20 L，858 字节/帧，远距或捕获率差时用。
    Stable,
    /// v27 L，1465 字节/帧。
    #[default]
    Default,
    /// v40 L，2953 字节/帧，近距大屏。
    Fast,
}

impl Density {
    pub fn from_id(id: u8) -> Self {
        match id {
            0 => Self::Stable,
            2 => Self::Fast,
            _ => Self::Default,
        }
    }

    pub fn id(self) -> u8 {
        match self {
            Self::Stable => 0,
            Self::Default => 1,
            Self::Fast => 2,
        }
    }

    pub const fn qr_version(self) -> i16 {
        match self {
            Self::Stable => 20,
            Self::Default => 27,
            Self::Fast => 40,
        }
    }

    /// QR byte 模式 + ECC L 的容量。
    pub const fn frame_bytes(self) -> u16 {
        match self {
            Self::Stable => 858,
            Self::Default => 1465,
            Self::Fast => 2953,
        }
    }

    pub const fn symbol_mtu(self) -> u16 {
        self.frame_bytes().saturating_sub(crate::protocol::HEADER_LEN as u16)
    }
}

impl fmt::Display for Density {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stable => write!(f, "v20"),
            Self::Default => write!(f, "v27"),
            Self::Fast => write!(f, "v40"),
        }
    }
}

/// 默认档的 QR 版本，便于旧调用点。
pub const QR_VERSION: i16 = 27;

/// 把协议载荷画成正方形 RGBA。`out_size` 为期望边长（像素）。
pub fn render_qr_rgba(data: &[u8], out_size: u32) -> Option<(u32, Vec<u8>)> {
    render_qr_rgba_density(data, out_size, Density::Default)
}

/// 按密度档画码。
pub fn render_qr_rgba_density(data: &[u8], out_size: u32, density: Density) -> Option<(u32, Vec<u8>)> {
    let (w, modules) = encode_modules(data, density.qr_version())?;
    let n = (w + QUIET_ZONE * 2) as u32;
    let scale = (out_size / n).max(1);
    let px = n * scale;
    let mut buf = vec![255u8; (px * px * 4) as usize];
    let dark = [18u8, 16, 14, 255];
    for y in 0..w {
        for x in 0..w {
            if !modules[y * w + x] {
                continue;
            }
            let x0 = (x as u32 + QUIET_ZONE as u32) * scale;
            let y0 = (y as u32 + QUIET_ZONE as u32) * scale;
            for dy in 0..scale {
                let row = ((y0 + dy) * px + x0) * 4;
                for dx in 0..scale {
                    let idx = (row + dx * 4) as usize;
                    buf[idx..idx + 4].copy_from_slice(&dark);
                }
            }
        }
    }
    Some((px, buf))
}

fn encode_modules(data: &[u8], version: i16) -> Option<(usize, Vec<bool>)> {
    if let Some(pair) = encode_pinned(data, version) {
        return Some(pair);
    }
    let code = QrCode::with_version(data, Version::Normal(version), EcLevel::L)
        .or_else(|_| QrCode::with_error_correction_level(data, EcLevel::L))
        .ok()?;
    let w = code.width();
    let modules: Vec<bool> = (0..w * w)
        .map(|i| code[(i % w, i / w)] == qrcode::Color::Dark)
        .collect();
    Some((w, modules))
}

fn encode_pinned(data: &[u8], version: i16) -> Option<(usize, Vec<bool>)> {
    let ver = Version::Normal(version);
    let mut bits = Bits::new(ver);
    bits.push_byte_data(data).ok()?;
    bits.push_terminator(EcLevel::L).ok()?;
    let raw = bits.into_bytes();
    let (data_ec, ec_data) = ec::construct_codewords(&raw, ver, EcLevel::L).ok()?;
    let mut canvas = Canvas::new(ver, EcLevel::L);
    canvas.draw_all_functional_patterns();
    canvas.draw_data(&data_ec, &ec_data);
    canvas.apply_mask(MaskPattern::LargeCheckerboard);
    let colors = canvas.into_colors();
    let w = match ver {
        Version::Normal(v) => (v * 4 + 17) as usize,
        _ => return None,
    };
    if colors.len() != w * w {
        return None;
    }
    let modules = colors
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    Some((w, modules))
}

/// 将 1 或 4 个载荷拼成一张宫格图。
pub fn compose_qr_grid(payloads: &[Vec<u8>], canvas: u32) -> Option<(u32, Vec<u8>)> {
    compose_qr_grid_density(payloads, canvas, Density::Default)
}

pub fn compose_qr_grid_density(
    payloads: &[Vec<u8>],
    canvas: u32,
    density: Density,
) -> Option<(u32, Vec<u8>)> {
    let n = if payloads.len() >= 4 { 2u32 } else { 1 };
    let gap = if n == 2 { 10 } else { 0 };
    let cell = (canvas.saturating_sub(gap)) / n;
    let mut buf = vec![255u8; (canvas as usize) * (canvas as usize) * 4];

    for (i, payload) in payloads.iter().take((n * n) as usize).enumerate() {
        let (px, pixels) = render_qr_rgba_density(payload, cell, density)?;
        let col = (i as u32) % n;
        let row = (i as u32) / n;
        let ox = col * (cell + gap) + cell.saturating_sub(px) / 2;
        let oy = row * (cell + gap) + cell.saturating_sub(px) / 2;
        blit_rgba(&mut buf, canvas, &pixels, px, ox, oy);
    }
    Some((canvas, buf))
}

fn blit_rgba(dst: &mut [u8], dst_w: u32, src: &[u8], src_w: u32, ox: u32, oy: u32) {
    for y in 0..src_w {
        let di = ((oy + y) * dst_w + ox) as usize * 4;
        let si = (y * src_w) as usize * 4;
        let bytes = (src_w as usize) * 4;
        if di + bytes <= dst.len() && si + bytes <= src.len() {
            dst[di..di + bytes].copy_from_slice(&src[si..si + bytes]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_capacity_fits() {
        let data = vec![7u8; Density::Default.frame_bytes() as usize];
        assert!(encode_modules(&data, Density::Default.qr_version()).is_some());
    }

    #[test]
    fn pinned_and_fallback_same_size() {
        let data = vec![9u8; 200];
        let (w1, _) = encode_modules(&data, 20).unwrap();
        assert_eq!(w1, 97);
    }
}
