//! 后台拉相机。容量为 1 的通道只保留最新帧。

use std::fmt;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;

use nokhwa::utils::CameraIndex;
use raysend_core::{rgba_to_luma, QrScan, ScanSlice};

pub struct CamFrame {
    pub preview_w: u32,
    pub preview_h: u32,
    pub preview_rgba: Vec<u8>,
    pub payloads: Vec<Vec<u8>>,
    pub slice: ScanSlice,
    pub dropped: u64,
}

/// 采集线程交给解码线程的最新帧。
pub(crate) struct RawCam {
    pub width: u32,
    pub height: u32,
    pub luma: Vec<u8>,
    pub preview_w: u32,
    pub preview_h: u32,
    pub preview_rgba: Vec<u8>,
}

pub enum CamEvent {
    Frame(CamFrame),
    Error(String),
}

/// 给下拉框用的摄像头项。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CameraChoice {
    pub index: CameraIndex,
    pub label: String,
}

impl fmt::Display for CameraChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// 打开指定摄像头；失败时发一条 Error 后退出。
pub fn spawn(index: CameraIndex) -> (Receiver<CamEvent>, SyncSender<()>) {
    #[cfg(windows)]
    if crate::dshow::is_dshow_index(&index) {
        return crate::dshow::spawn(index);
    }
    spawn_msmf(index)
}

fn spawn_msmf(index: CameraIndex) -> (Receiver<CamEvent>, SyncSender<()>) {
    let (ui_tx, ui_rx) = mpsc::sync_channel::<CamEvent>(1);
    let (raw_tx, raw_rx) = mpsc::sync_channel::<RawCam>(1);
    let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);
    spawn_decode_loop(raw_rx, ui_tx.clone());
    let _ = thread::Builder::new()
        .name("raysend-camera".into())
        .spawn(move || camera_loop(index, raw_tx, ui_tx, stop_rx));
    (ui_rx, stop_tx)
}

pub(crate) fn spawn_decode_loop(raw_rx: Receiver<RawCam>, ui_tx: SyncSender<CamEvent>) {
    let _ = thread::Builder::new()
        .name("raysend-decode".into())
        .spawn(move || {
            let mut tracker = QrScan::new();
            while let Ok(raw) = raw_rx.recv() {
                let mut latest = raw;
                let mut dropped = 0u64;
                while let Ok(more) = raw_rx.try_recv() {
                    latest = more;
                    dropped += 1;
                }
                let payloads = tracker.scan_luma(latest.width, latest.height, &latest.luma);
                if ui_tx
                    .try_send(CamEvent::Frame(CamFrame {
                        preview_w: latest.preview_w,
                        preview_h: latest.preview_h,
                        preview_rgba: latest.preview_rgba,
                        payloads,
                        slice: tracker.last_slice(),
                        dropped,
                    }))
                    .is_err()
                {
                    // UI 还没取走上帧，这一帧的结果也算忙丢。
                }
            }
        });
}

fn camera_loop(
    index: CameraIndex,
    raw_tx: SyncSender<RawCam>,
    ui_tx: SyncSender<CamEvent>,
    stop: Receiver<()>,
) {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{CameraFormat, FrameFormat, RequestedFormat, RequestedFormatType, Resolution};
    use nokhwa::Camera;

    let mut camera = None;
    for requested in [
        RequestedFormatType::Closest(CameraFormat::new(
            Resolution::new(1280, 720),
            FrameFormat::MJPEG,
            60,
        )),
        RequestedFormatType::Closest(CameraFormat::new(
            Resolution::new(1280, 720),
            FrameFormat::YUYV,
            60,
        )),
        RequestedFormatType::HighestFrameRate(60),
        RequestedFormatType::None,
    ] {
        if let Ok(cam) = Camera::new(index.clone(), RequestedFormat::new::<RgbFormat>(requested)) {
            camera = Some(cam);
            break;
        }
    }
    let mut camera = match camera {
        Some(cam) => cam,
        None => {
            let _ = ui_tx.try_send(CamEvent::Error("无法按 1280@60 打开相机".into()));
            return;
        }
    };
    if let Err(err) = camera.open_stream() {
        let _ = ui_tx.try_send(CamEvent::Error(err.to_string()));
        return;
    }

    let mut misses = 0u32;
    loop {
        if stop.try_recv().is_ok() {
            break;
        }
        let Ok(frame) = camera.frame() else {
            misses += 1;
            if misses > 80 {
                let _ = ui_tx.try_send(CamEvent::Error("相机已打开但读不到画面".into()));
                return;
            }
            thread::sleep(std::time::Duration::from_millis(15));
            continue;
        };
        let Ok(rgb) = frame.decode_image::<RgbFormat>() else {
            misses += 1;
            if misses > 80 {
                let _ = ui_tx.try_send(CamEvent::Error("相机画面格式无法解码".into()));
                return;
            }
            continue;
        };
        misses = 0;
        let width = rgb.width();
        let height = rgb.height();
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for p in rgb.pixels() {
            rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
        let luma = rgba_to_luma(width, height, &rgba);
        let (preview_w, preview_h, preview_rgba) = downscale_rgba(width, height, &rgba, 480);
        let _ = raw_tx.try_send(RawCam {
            width,
            height,
            luma,
            preview_w,
            preview_h,
            preview_rgba,
        });
    }
}

pub(crate) fn downscale_rgba(w: u32, h: u32, rgba: &[u8], max_side: u32) -> (u32, u32, Vec<u8>) {
    let max_dim = w.max(h).max(1);
    if max_dim <= max_side {
        return (w, h, rgba.to_vec());
    }
    let nw = ((w as u64 * max_side as u64) / max_dim as u64).max(1) as u32;
    let nh = ((h as u64 * max_side as u64) / max_dim as u64).max(1) as u32;
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let sx = x * w / nw;
            let sy = y * h / nh;
            let si = ((sy * w + sx) * 4) as usize;
            let di = ((y * nw + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
        }
    }
    (nw, nh, out)
}

/// 枚举本机摄像头。Windows 上会合并 Media Foundation 与 DirectShow，
/// 后者才能看到 OBS 虚拟相机。
pub fn list_cameras() -> Vec<CameraChoice> {
    use nokhwa::query;
    use nokhwa::utils::ApiBackend;
    let mut choices: Vec<CameraChoice> = query(ApiBackend::Auto)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, info)| {
            let name = info.human_name();
            let label = if name.trim().is_empty() {
                format!("Camera {}", i + 1)
            } else {
                name
            };
            CameraChoice {
                index: info.index().clone(),
                label,
            }
        })
        .filter(|choice| !looks_like_obs_virtual(&choice.label))
        .collect();
    #[cfg(windows)]
    {
        for extra in crate::dshow::list_cameras() {
            let extra_key = extra.label.to_ascii_lowercase();
            if !choices
                .iter()
                .any(|c| c.label.to_ascii_lowercase() == extra_key)
            {
                choices.push(extra);
            }
        }
    }
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for choice in &choices {
        *counts.entry(choice.label.clone()).or_default() += 1;
    }
    for choice in &mut choices {
        if counts.get(&choice.label).copied().unwrap_or(0) > 1 {
            choice.label = format!("{} · {}", choice.label, choice.index.as_string());
        }
    }
    choices
}

fn looks_like_obs_virtual(label: &str) -> bool {
    let n = label.to_ascii_lowercase();
    n.contains("obs virtual") || n.contains("obs-camera")
}
