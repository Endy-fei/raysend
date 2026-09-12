//! 用上一帧四角直接采样模块，跳过寻像。失败则由调用方回退完整检测。

use rqrr::{Grid, SimpleGrid};

use crate::scan::{ScanHint, ScanRegion};

/// 从已知四角（图像坐标，TL/TR/BR/BL）采样并解码。
pub fn decode_from_quad(
    width: u32,
    height: u32,
    luma: &[u8],
    hint: ScanHint,
) -> Option<(Vec<u8>, ScanRegion, ScanHint)> {
    let modules = hint.modules as usize;
    if modules < 21 || modules > 177 || (modules - 17) % 4 != 0 {
        return None;
    }
    let w = width as usize;
    let h = height as usize;
    if luma.len() < w * h || w < 8 || h < 8 {
        return None;
    }
    let map = Homography::from_quad(&hint.corners, (modules + 1) as f64, (modules + 1) as f64)?;
    let mut samples = Vec::with_capacity(modules * modules);
    for y in 0..modules {
        for x in 0..modules {
            let (px, py) = map.map(x as f64 + 0.5, y as f64 + 0.5);
            samples.push(sample_luma(luma, w, h, px, py)?);
        }
    }
    let mean = samples.iter().map(|v| *v as u32).sum::<u32>() / samples.len() as u32;
    let mut contrast = 0u64;
    for v in &samples {
        let d = *v as i32 - mean as i32;
        contrast += (d * d) as u64;
    }
    if contrast / (samples.len() as u64) < 64 {
        return None;
    }
    let grid = SimpleGrid::from_func(modules, |x, y| {
        samples[y * modules + x] < mean as u8
    });
    let wrapped = Grid::new(grid);
    let mut bytes = Vec::new();
    if wrapped.decode_to(&mut bytes).is_err() || bytes.is_empty() {
        return None;
    }
    let region = region_from_corners(&hint.corners)?;
    Some((bytes, region, hint))
}

fn region_from_corners(corners: &[(i32, i32); 4]) -> Option<ScanRegion> {
    let min_x = corners.iter().map(|p| p.0).min()?;
    let max_x = corners.iter().map(|p| p.0).max()?;
    let min_y = corners.iter().map(|p| p.1).min()?;
    let max_y = corners.iter().map(|p| p.1).max()?;
    if max_x <= min_x || max_y <= min_y {
        return None;
    }
    Some(ScanRegion {
        x: min_x.max(0) as u32,
        y: min_y.max(0) as u32,
        w: (max_x - min_x) as u32,
        h: (max_y - min_y) as u32,
    })
}

struct Homography {
    c: [f64; 8],
}

impl Homography {
    fn from_quad(rect: &[(i32, i32); 4], w: f64, h: f64) -> Option<Self> {
        let x0 = rect[0].0 as f64;
        let y0 = rect[0].1 as f64;
        let x1 = rect[1].0 as f64;
        let y1 = rect[1].1 as f64;
        let x2 = rect[2].0 as f64;
        let y2 = rect[2].1 as f64;
        let x3 = rect[3].0 as f64;
        let y3 = rect[3].1 as f64;
        let wden = w * (x2 * y3 - x3 * y2 + (x3 - x2) * y1 + x1 * (y2 - y3));
        let hden = h * (x2 * y3 + x1 * (y2 - y3) - x3 * y2 + (x3 - x2) * y1);
        if wden.abs() < f64::EPSILON || hden.abs() < f64::EPSILON {
            return None;
        }
        let mut c = [0.0; 8];
        c[0] = (x1 * (x2 * y3 - x3 * y2)
            + x0 * (-x2 * y3 + x3 * y2 + (x2 - x3) * y1)
            + x1 * (x3 - x2) * y0)
            / wden;
        c[1] = -(x0 * (x2 * y3 + x1 * (y2 - y3) - x2 * y1) - x1 * x3 * y2
            + x2 * x3 * y1
            + (x1 * x3 - x2 * x3) * y0)
            / hden;
        c[2] = x0;
        c[3] = (y0 * (x1 * (y3 - y2) - x2 * y3 + x3 * y2)
            + y1 * (x2 * y3 - x3 * y2)
            + x0 * y1 * (y2 - y3))
            / wden;
        c[4] = (x0 * (y1 * y3 - y2 * y3) + x1 * y2 * y3 - x2 * y1 * y3
            + y0 * (x3 * y2 - x1 * y2 + (x2 - x3) * y1))
            / hden;
        c[5] = y0;
        c[6] = (x1 * (y3 - y2) + x0 * (y2 - y3) + (x2 - x3) * y1 + (x3 - x2) * y0) / wden;
        c[7] = (-x2 * y3 + x1 * y3 + x3 * y2 + x0 * (y1 - y2) - x3 * y1 + (x2 - x1) * y0) / hden;
        Some(Self { c })
    }

    fn map(&self, u: f64, v: f64) -> (f64, f64) {
        let den = self.c[6] * u + self.c[7] * v + 1.0;
        if den.abs() < 1e-9 {
            return (self.c[2], self.c[5]);
        }
        (
            (self.c[0] * u + self.c[1] * v + self.c[2]) / den,
            (self.c[3] * u + self.c[4] * v + self.c[5]) / den,
        )
    }
}

fn sample_luma(luma: &[u8], width: usize, height: usize, x: f64, y: f64) -> Option<u8> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    if x0 < 0 || y0 < 0 || x0 + 1 >= width as i32 || y0 + 1 >= height as i32 {
        let xi = x.round() as i32;
        let yi = y.round() as i32;
        if xi < 0 || yi < 0 || xi >= width as i32 || yi >= height as i32 {
            return None;
        }
        return Some(luma[yi as usize * width + xi as usize]);
    }
    let fx = (x - x0 as f64) as f32;
    let fy = (y - y0 as f64) as f32;
    let i00 = luma[y0 as usize * width + x0 as usize] as f32;
    let i10 = luma[y0 as usize * width + (x0 + 1) as usize] as f32;
    let i01 = luma[(y0 + 1) as usize * width + x0 as usize] as f32;
    let i11 = luma[(y0 + 1) as usize * width + (x0 + 1) as usize] as f32;
    let top = i00 + (i10 - i00) * fx;
    let bot = i01 + (i11 - i01) * fx;
    Some((top + (bot - top) * fy).round() as u8)
}

pub fn modules_from_version(version: usize) -> Option<u16> {
    if (1..=40).contains(&version) {
        Some((17 + 4 * version) as u16)
    } else {
        None
    }
}
