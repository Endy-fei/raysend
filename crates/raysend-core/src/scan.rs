//! 从灰度帧找 QR：区域跟踪、四角快路径、rqrr / quircs。喷泉还原见 [`crate::decoder`]。

use quircs::Quirc;

use crate::scan_tracked::{decode_from_quad, modules_from_version};

/// 图像坐标系里的矩形（像素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanRegion {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// 已经裁好的窗口，原点是它在整帧里的左上角。
pub struct ScanWindow<'a> {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub luma: &'a [u8],
    pub hint: Option<ScanHint>,
    pub discover: bool,
}

/// 上一帧四角（图像坐标，TL / TR / BR / BL）和模块数，用来跳过寻像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanHint {
    pub corners: [(i32, i32); 4],
    pub modules: u16,
}

/// 下一帧要裁的窗口，以及可选的跟踪提示。
#[derive(Clone, Copy, Debug)]
pub struct PlannedCrop {
    pub region: ScanRegion,
    pub hint: Option<ScanHint>,
}

/// 工作线程或主线程解完的一个窗口，区域相对窗口原点。
#[derive(Clone, Debug)]
pub struct DecodedWindow {
    pub x: u32,
    pub y: u32,
    pub payloads: Vec<Vec<u8>>,
    pub regions: Vec<ScanRegion>,
    pub hints: Vec<ScanHint>,
    pub had_hint: bool,
    pub tracked: bool,
    pub discover: bool,
}

/// 一帧里各窗口的解码计数，给回执用。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanSlice {
    pub windows: u64,
    pub hits: u64,
    pub tracked_ok: u64,
    pub tracked_fail: u64,
    pub discover: u64,
    pub payloads: u64,
}

impl ScanSlice {
    pub fn from_windows(windows: &[DecodedWindow]) -> Self {
        let mut slice = Self {
            windows: windows.len() as u64,
            ..Self::default()
        };
        for window in windows {
            if !window.payloads.is_empty() {
                slice.hits += 1;
            }
            if window.tracked {
                slice.tracked_ok += 1;
            } else if window.had_hint {
                slice.tracked_fail += 1;
            }
            if window.discover {
                slice.discover += 1;
            }
            slice.payloads += window.payloads.len() as u64;
        }
        slice
    }
}

#[derive(Clone, Copy)]
struct Track {
    region: ScanRegion,
    vx: i32,
    vy: i32,
    misses: u32,
    hint: Option<ScanHint>,
}

/// 相对码边长的裁剪边距，外加 2× 位移，手持时框要超前而不是追。
const CROP_PAD_RATIO: f32 = 0.35;
const FULL_SCAN_COLD: u64 = 8;
const FULL_SCAN_HOT: u64 = 24;
const MAX_MISSES: u32 = 4;
const MIN_SIDE: u32 = 16;
const MAX_TRACKS: usize = 6;

/// 无喷泉状态的定位/解码器，可放在相机或工作线程上。
pub struct QrScan {
    scanner: Quirc,
    tracks: Vec<Track>,
    frame_idx: u64,
    last: ScanSlice,
}

impl QrScan {
    pub fn new() -> Self {
        Self {
            scanner: Quirc::default(),
            tracks: Vec::new(),
            frame_idx: 0,
            last: ScanSlice::default(),
        }
    }

    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// 正在读到的码框（未连续丢失），给取景叠加用。
    pub fn live_regions(&self) -> Vec<ScanRegion> {
        self.tracks
            .iter()
            .filter(|track| track.misses == 0)
            .map(|track| track.region)
            .collect()
    }

    pub fn last_slice(&self) -> ScanSlice {
        self.last
    }

    /// `None` 表示下一帧应做整图扫描；`Some` 是已加 pad、并按速度前移的裁剪框。
    pub fn planned_crops(&self, width: u32, height: u32) -> Option<Vec<PlannedCrop>> {
        if width < MIN_SIDE || height < MIN_SIDE || self.should_full_scan() {
            return None;
        }
        let crops: Vec<PlannedCrop> = self
            .tracks
            .iter()
            .map(|track| lead_crop(track, width, height))
            .filter(|crop| crop.region.w >= MIN_SIDE && crop.region.h >= MIN_SIDE)
            .collect();
        if crops.is_empty() {
            None
        } else {
            Some(crops)
        }
    }

    pub fn scan_luma(&mut self, width: u32, height: u32, luma: &[u8]) -> Vec<Vec<u8>> {
        if width == 0 || height == 0 {
            return Vec::new();
        }
        let expected = width as usize * height as usize;
        if luma.len() < expected {
            return Vec::new();
        }
        let luma = &luma[..expected];
        if let Some(crops) = self.planned_crops(width, height) {
            let windows: Vec<(PlannedCrop, u32, u32, Vec<u8>)> = crops
                .into_iter()
                .map(|crop| {
                    let (cw, ch, buf) = extract_luma(luma, width, height, crop.region);
                    (crop, cw, ch, buf)
                })
                .collect();
            let views: Vec<ScanWindow<'_>> = windows
                .iter()
                .map(|(crop, w, h, buf)| ScanWindow {
                    x: crop.region.x,
                    y: crop.region.y,
                    width: *w,
                    height: *h,
                    luma: buf,
                    hint: crop.hint.map(|hint| hint.offset(-(crop.region.x as i32), -(crop.region.y as i32))),
                    discover: false,
                })
                .collect();
            self.scan_windows(&views)
        } else {
            self.scan_full(width, height, luma)
        }
    }

    pub fn scan_full(&mut self, width: u32, height: u32, luma: &[u8]) -> Vec<Vec<u8>> {
        let (payloads, regions, hints) = decode_all(width, height, luma, &mut self.scanner, true);
        self.absorb_windows(&[DecodedWindow {
            x: 0,
            y: 0,
            payloads,
            regions,
            hints,
            had_hint: false,
            tracked: false,
            discover: true,
        }])
    }

    pub fn scan_windows(&mut self, windows: &[ScanWindow<'_>]) -> Vec<Vec<u8>> {
        let decoded = decode_windows(windows);
        self.absorb_windows(&decoded)
    }

    /// 把已经解完的窗口并进跟踪器，供 Worker / 并行线程回传。
    pub fn absorb_windows(&mut self, windows: &[DecodedWindow]) -> Vec<Vec<u8>> {
        self.absorb(windows, false)
    }

    /// 只更新这一窗里出现的码，其它跟踪框保持原样。
    /// 空闲 worker 按窗回传时用，避免一次失败把宫格里其它码判丢。
    pub fn absorb_partial(&mut self, windows: &[DecodedWindow]) -> Vec<Vec<u8>> {
        self.absorb(windows, true)
    }

    fn absorb(&mut self, windows: &[DecodedWindow], keep_unseen: bool) -> Vec<Vec<u8>> {
        self.last = ScanSlice::from_windows(windows);
        self.frame_idx = self.frame_idx.wrapping_add(1);
        let previous = std::mem::take(&mut self.tracks);
        let mut next = Vec::new();
        let mut payloads = Vec::new();
        for window in windows {
            if window.payloads.is_empty() {
                continue;
            }
            payloads.extend(window.payloads.iter().cloned());
            for (index, box_r) in window.regions.iter().enumerate() {
                let hint = window.hints.get(index).copied().map(|hint| {
                    hint.offset(window.x as i32, window.y as i32)
                });
                next.push(match_track(
                    &previous,
                    ScanRegion {
                        x: window.x.saturating_add(box_r.x),
                        y: window.y.saturating_add(box_r.y),
                        w: box_r.w,
                        h: box_r.h,
                    },
                    hint,
                ));
            }
        }
        for track in previous {
            if next.iter().any(|hit| iou(&hit.region, &track.region) > 0.25) {
                continue;
            }
            if keep_unseen {
                next.push(track);
                continue;
            }
            let mut missed = track;
            missed.misses = missed.misses.saturating_add(1);
            if missed.misses < MAX_MISSES {
                next.push(missed);
            }
        }
        self.tracks = merge_tracks(next);
        payloads
    }

    fn should_full_scan(&self) -> bool {
        if self.tracks.is_empty() {
            return true;
        }
        let interval = if self.tracks.iter().all(|track| track.misses == 0) {
            FULL_SCAN_HOT
        } else {
            FULL_SCAN_COLD
        };
        (self.frame_idx.wrapping_add(1)) % interval == 0
    }
}

impl Default for QrScan {
    fn default() -> Self {
        Self::new()
    }
}

/// 无状态解码：给工作线程或 FFI 用。整图会同时跑 rqrr 与 quircs。
pub fn decode_qr_luma(width: u32, height: u32, luma: &[u8]) -> (Vec<Vec<u8>>, Vec<ScanRegion>) {
    let (payloads, regions, _) = decode_qr_luma_ex(width, height, luma, true);
    (payloads, regions)
}

/// `discover` 为真时，rqrr 不够 4 个码会再跑 quircs（整图找宫格）。
pub fn decode_qr_luma_ex(
    width: u32,
    height: u32,
    luma: &[u8],
    discover: bool,
) -> (Vec<Vec<u8>>, Vec<ScanRegion>, Vec<ScanHint>) {
    let mut scanner = Quirc::default();
    decode_all(width, height, luma, &mut scanner, discover)
}

fn decode_windows(windows: &[ScanWindow<'_>]) -> Vec<DecodedWindow> {
    #[cfg(not(target_arch = "wasm32"))]
    if windows.len() > 1 {
        return std::thread::scope(|scope| {
            let handles: Vec<_> = windows
                .iter()
                .map(|window| scope.spawn(|| decode_one_window(window)))
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok().flatten())
                .collect()
        });
    }
    windows.iter().filter_map(decode_one_window).collect()
}

fn decode_one_window(window: &ScanWindow<'_>) -> Option<DecodedWindow> {
    if window.width < MIN_SIDE || window.height < MIN_SIDE {
        return None;
    }
    let expected = window.width as usize * window.height as usize;
    if window.luma.len() < expected {
        return None;
    }
    let luma = &window.luma[..expected];
    if let Some(hint) = window.hint {
        if let Some((payload, region, used)) = decode_from_quad(window.width, window.height, luma, hint)
        {
            return Some(DecodedWindow {
                x: window.x,
                y: window.y,
                payloads: vec![payload],
                regions: vec![region],
                hints: vec![used],
                had_hint: true,
                tracked: true,
                discover: window.discover,
            });
        }
    }
    let mut scanner = Quirc::default();
    let (payloads, regions, hints) = decode_all(window.width, window.height, luma, &mut scanner, false);
    Some(DecodedWindow {
        x: window.x,
        y: window.y,
        payloads,
        regions,
        hints,
        had_hint: window.hint.is_some(),
        tracked: false,
        discover: window.discover,
    })
}

fn extract_luma(luma: &[u8], width: u32, height: u32, region: ScanRegion) -> (u32, u32, Vec<u8>) {
    let region = clamp_region(region, width, height);
    let mut out = vec![0u8; region.w as usize * region.h as usize];
    for y in 0..region.h {
        let src = ((region.y + y) * width + region.x) as usize;
        let dst = (y * region.w) as usize;
        out[dst..dst + region.w as usize]
            .copy_from_slice(&luma[src..src + region.w as usize]);
    }
    (region.w, region.h, out)
}

fn decode_all(
    width: u32,
    height: u32,
    luma: &[u8],
    scanner: &mut Quirc,
    discover: bool,
) -> (Vec<Vec<u8>>, Vec<ScanRegion>, Vec<ScanHint>) {
    let mut payloads = Vec::new();
    let mut regions = Vec::new();
    let mut hints = Vec::new();

    if let Some((found, boxes, found_hints)) = decode_rqrr(width, height, luma) {
        payloads.extend(found);
        regions.extend(boxes);
        hints.extend(found_hints);
    }

    // 整图要尽量找齐宫格；裁剪窗口命中后不再跑 quircs。
    let need_quircs = payloads.is_empty() || (discover && payloads.len() < 4);
    if need_quircs {
        let codes: Vec<_> = scanner
            .identify(width as usize, height as usize, luma)
            .flatten()
            .collect();
        for code in codes {
            if let Some(region) = region_from_i32(
                code.corners.iter().map(|p| p.x),
                code.corners.iter().map(|p| p.y),
            ) {
                if !regions.iter().any(|existing| iou(existing, &region) > 0.55) {
                    regions.push(region);
                }
            }
            if let Ok(decoded) = code.decode() {
                if !payloads.iter().any(|p| p == &decoded.payload) {
                    payloads.push(decoded.payload);
                }
            }
        }
    }

    if regions.is_empty() && !payloads.is_empty() {
        regions.push(ScanRegion {
            x: 0,
            y: 0,
            w: width,
            h: height,
        });
    }
    (payloads, regions, hints)
}

fn decode_rqrr(
    width: u32,
    height: u32,
    luma: &[u8],
) -> Option<(Vec<Vec<u8>>, Vec<ScanRegion>, Vec<ScanHint>)> {
    let w = width as usize;
    let h = height as usize;
    if luma.len() < w * h || w < MIN_SIDE as usize || h < MIN_SIDE as usize {
        return None;
    }
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| luma[y * w + x]);
    let grids = img.detect_grids();
    if grids.is_empty() {
        return None;
    }
    let mut payloads = Vec::new();
    let mut regions = Vec::new();
    let mut hints = Vec::new();
    for grid in grids {
        let corners = [
            (grid.bounds[0].x, grid.bounds[0].y),
            (grid.bounds[1].x, grid.bounds[1].y),
            (grid.bounds[2].x, grid.bounds[2].y),
            (grid.bounds[3].x, grid.bounds[3].y),
        ];
        let mut bytes = Vec::new();
        let Ok(meta) = grid.decode_to(&mut bytes) else {
            continue;
        };
        if bytes.is_empty() {
            continue;
        }
        let Some(region) = region_from_i32(
            grid.bounds.iter().map(|p| p.x),
            grid.bounds.iter().map(|p| p.y),
        ) else {
            continue;
        };
        let Some(modules) = modules_from_version(meta.version.0) else {
            continue;
        };
        payloads.push(bytes);
        regions.push(region);
        hints.push(ScanHint { corners, modules });
    }
    if payloads.is_empty() && regions.is_empty() {
        None
    } else {
        Some((payloads, regions, hints))
    }
}

fn region_from_i32(
    xs: impl IntoIterator<Item = i32>,
    ys: impl IntoIterator<Item = i32>,
) -> Option<ScanRegion> {
    let xs: Vec<i32> = xs.into_iter().collect();
    let ys: Vec<i32> = ys.into_iter().collect();
    let min_x = *xs.iter().min()?;
    let max_x = *xs.iter().max()?;
    let min_y = *ys.iter().min()?;
    let max_y = *ys.iter().max()?;
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

impl ScanHint {
    pub fn offset(self, dx: i32, dy: i32) -> Self {
        let mut corners = self.corners;
        for point in &mut corners {
            point.0 += dx;
            point.1 += dy;
        }
        Self {
            corners,
            modules: self.modules,
        }
    }
}

fn crop_pad(size: u32, drift: u32) -> u32 {
    let base = (size as f32 * CROP_PAD_RATIO).round() as u32;
    base.saturating_add(drift.saturating_mul(2).min(size)).max(16)
}

fn lead_crop(track: &Track, width: u32, height: u32) -> PlannedCrop {
    let hint = track.hint.map(|hint| hint.offset(track.vx, track.vy));
    if let Some(hint) = hint {
        let min_x = hint.corners.iter().map(|p| p.0).min().unwrap_or(0);
        let max_x = hint.corners.iter().map(|p| p.0).max().unwrap_or(0);
        let min_y = hint.corners.iter().map(|p| p.1).min().unwrap_or(0);
        let max_y = hint.corners.iter().map(|p| p.1).max().unwrap_or(0);
        let bw = (max_x - min_x).max(1) as u32;
        let bh = (max_y - min_y).max(1) as u32;
        let pad = crop_pad(bw.max(bh), track.vx.unsigned_abs().max(track.vy.unsigned_abs()));
        let region = clamp_region(
            ScanRegion {
                x: (min_x.max(0) as u32).saturating_sub(pad),
                y: (min_y.max(0) as u32).saturating_sub(pad),
                w: bw.saturating_add(pad.saturating_mul(2)),
                h: bh.saturating_add(pad.saturating_mul(2)),
            },
            width,
            height,
        );
        return PlannedCrop {
            region,
            hint: Some(hint),
        };
    }
    let pred_x = (track.region.x as i32 + track.vx).clamp(0, width.saturating_sub(1) as i32) as u32;
    let pred_y = (track.region.y as i32 + track.vy).clamp(0, height.saturating_sub(1) as i32) as u32;
    let pad = crop_pad(
        track.region.w.max(track.region.h),
        track.vx.unsigned_abs().max(track.vy.unsigned_abs()),
    );
    let region = clamp_region(
        ScanRegion {
            x: pred_x.saturating_sub(pad),
            y: pred_y.saturating_sub(pad),
            w: track.region.w.saturating_add(pad.saturating_mul(2)),
            h: track.region.h.saturating_add(pad.saturating_mul(2)),
        },
        width,
        height,
    );
    PlannedCrop { region, hint: None }
}

fn clamp_region(region: ScanRegion, width: u32, height: u32) -> ScanRegion {
    if width == 0 || height == 0 {
        return ScanRegion {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        };
    }
    let x = region.x.min(width.saturating_sub(1));
    let y = region.y.min(height.saturating_sub(1));
    let w = region.w.min(width.saturating_sub(x)).max(1);
    let h = region.h.min(height.saturating_sub(y)).max(1);
    ScanRegion { x, y, w, h }
}

fn match_track(previous: &[Track], region: ScanRegion, hint: Option<ScanHint>) -> Track {
    let cx = region.x as i32 + region.w as i32 / 2;
    let cy = region.y as i32 + region.h as i32 / 2;
    let mut best: Option<(&Track, i32)> = None;
    for track in previous {
        let tcx = track.region.x as i32 + track.region.w as i32 / 2;
        let tcy = track.region.y as i32 + track.region.h as i32 / 2;
        let dist = (cx - tcx).abs() + (cy - tcy).abs();
        let thresh = ((track.region.w.max(track.region.h) + region.w.max(region.h)) as i32) / 2;
        if dist <= thresh.max(24) && best.map_or(true, |(_, best_d)| dist < best_d) {
            best = Some((track, dist));
        }
    }
    if let Some((old, _)) = best {
        Track {
            region,
            vx: region.x as i32 - old.region.x as i32,
            vy: region.y as i32 - old.region.y as i32,
            misses: 0,
            hint: hint.or(old.hint),
        }
    } else {
        Track {
            region,
            vx: 0,
            vy: 0,
            misses: 0,
            hint,
        }
    }
}

fn merge_tracks(mut tracks: Vec<Track>) -> Vec<Track> {
    tracks.sort_by_key(|track| track.misses);
    let mut out = Vec::new();
    for track in tracks {
        if out
            .iter()
            .any(|existing: &Track| iou(&existing.region, &track.region) > 0.55)
        {
            continue;
        }
        out.push(track);
        if out.len() >= MAX_TRACKS {
            break;
        }
    }
    out
}

fn iou(a: &ScanRegion, b: &ScanRegion) -> f32 {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    let inter = ((x1 - x0) * (y1 - y0)) as f32;
    let union = (a.w * a.h + b.w * b.h) as f32 - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// BT.601 近似：`(R*77 + G*150 + B*29) >> 8`。
pub fn rgba_to_luma(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut luma = vec![0u8; width as usize * height as usize];
    for (i, pixel) in luma.iter_mut().enumerate() {
        let r = rgba[i * 4] as u32;
        let g = rgba[i * 4 + 1] as u32;
        let b = rgba[i * 4 + 2] as u32;
        *pixel = ((r * 77 + g * 150 + b * 29) >> 8) as u8;
    }
    luma
}

#[cfg(all(test, feature = "codec"))]
mod tests {
    use super::*;
    use crate::decoder::Decoder;
    use crate::qr::{render_qr_rgba_density, Density};
    use crate::session::Outgoing;

    #[test]
    fn decoder_rebuilds_file() {
        let original = b"hello from raysend fountain".to_vec();
        let mut outgoing =
            Outgoing::prepare_with("note.txt".into(), original.clone(), Density::Stable).unwrap();
        let mut decoder = Decoder::new();

        for _ in 0..400 {
            decoder.process_frame(&outgoing.next_payloads(1)[0]);
            if decoder.is_finished() {
                break;
            }
        }

        let finished = decoder.take_finished().expect("finished");
        assert_eq!(finished.meta.name, "note.txt");
        assert_eq!(finished.data, original);
    }

    #[test]
    fn scan_rendered_qr() {
        let original = b"scan-me-please".to_vec();
        let mut outgoing =
            Outgoing::prepare_with("s.txt".into(), original.clone(), Density::Stable).unwrap();
        let mut decoder = Decoder::new();
        for _ in 0..200 {
            let payload = outgoing.next_payloads(1).remove(0);
            let (px, rgba) = render_qr_rgba_density(&payload, 480, Density::Stable).unwrap();
            decoder.scan_rgba(px, px, &rgba);
            if decoder.is_finished() {
                break;
            }
        }
        let finished = decoder.take_finished().expect("scanned");
        assert_eq!(finished.data, original);
    }

    #[test]
    fn decode_qr_luma_reads_bounds() {
        let mut outgoing =
            Outgoing::prepare_with("b.txt".into(), b"bound".to_vec(), Density::Stable).unwrap();
        let payload = outgoing.next_payloads(1).remove(0);
        let (px, rgba) = render_qr_rgba_density(&payload, 360, Density::Stable).unwrap();
        let luma = rgba_to_luma(px, px, &rgba);
        let (found, regions) = decode_qr_luma(px, px, &luma);
        assert!(!found.is_empty(), "rqrr/quircs should read the rendered QR");
        assert!(!regions.is_empty());
        assert!(found.iter().any(|p| p == &payload));
    }

    #[test]
    fn crop_path_keeps_decoding() {
        let mut outgoing =
            Outgoing::prepare_with("c.txt".into(), b"crop-path".to_vec(), Density::Stable).unwrap();
        let payload = outgoing.next_payloads(1).remove(0);
        let (px, rgba) = render_qr_rgba_density(&payload, 400, Density::Stable).unwrap();
        let luma = rgba_to_luma(px, px, &rgba);
        let mut tracker = QrScan::new();
        let first = tracker.scan_full(px, px, &luma);
        assert!(!first.is_empty());
        let crops = tracker
            .planned_crops(px, px)
            .expect("healthy track should prefer crops");
        let windows: Vec<(u32, u32, u32, u32, Vec<u8>, Option<ScanHint>)> = crops
            .into_iter()
            .map(|crop| {
                let (cw, ch, buf) = extract_luma(&luma, px, px, crop.region);
                (
                    crop.region.x,
                    crop.region.y,
                    cw,
                    ch,
                    buf,
                    crop.hint
                        .map(|hint| hint.offset(-(crop.region.x as i32), -(crop.region.y as i32))),
                )
            })
            .collect();
        let views: Vec<ScanWindow<'_>> = windows
            .iter()
            .map(|(x, y, w, h, buf, hint)| ScanWindow {
                x: *x,
                y: *y,
                width: *w,
                height: *h,
                luma: buf,
                hint: *hint,
                discover: false,
            })
            .collect();
        let again = tracker.scan_windows(&views);
        assert!(again.iter().any(|p| p == &payload));
    }

    #[test]
    fn absorb_windows_feeds_tracker() {
        let mut outgoing =
            Outgoing::prepare_with("a.txt".into(), b"absorb".to_vec(), Density::Stable).unwrap();
        let payload = outgoing.next_payloads(1).remove(0);
        let (px, rgba) = render_qr_rgba_density(&payload, 360, Density::Stable).unwrap();
        let luma = rgba_to_luma(px, px, &rgba);
        let (found, regions, hints) = decode_qr_luma_ex(px, px, &luma, true);
        let mut tracker = QrScan::new();
        let got = tracker.absorb_windows(&[DecodedWindow {
            x: 0,
            y: 0,
            payloads: found,
            regions,
            hints,
            had_hint: false,
            tracked: false,
            discover: true,
        }]);
        assert!(got.iter().any(|p| p == &payload));
        assert!(tracker.track_count() > 0);
    }

    #[test]
    fn tracked_quad_skips_finder() {
        let mut outgoing =
            Outgoing::prepare_with("t.txt".into(), b"tracked".to_vec(), Density::Stable).unwrap();
        let payload = outgoing.next_payloads(1).remove(0);
        let (px, rgba) = render_qr_rgba_density(&payload, 400, Density::Stable).unwrap();
        let luma = rgba_to_luma(px, px, &rgba);
        let mut tracker = QrScan::new();
        assert!(!tracker.scan_full(px, px, &luma).is_empty());
        let crop = tracker.planned_crops(px, px).unwrap().remove(0);
        let hint = crop.hint.expect("full scan should store a quad");
        let local = hint.offset(-(crop.region.x as i32), -(crop.region.y as i32));
        let (cw, ch, buf) = extract_luma(&luma, px, px, crop.region);
        let (bytes, _, _) = crate::decode_from_quad(cw, ch, &buf, local)
            .expect("quad sample should decode without finder search");
        assert_eq!(bytes, payload);
    }
}
