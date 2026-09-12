//! 网页接收：getUserMedia + 全分辨率取帧，识别与还原交给 `raysend-core`。

use crate::i18n::{current, Lang};
use crate::utils::log;
use crate::{ReceiveResult, ReceiveStatus, CAMERA_FACING, RECEIVE_RESULT, RECEIVE_UI};
use dioxus::prelude::*;
use raysend_core::{DecodedWindow, TransferReceipt};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Blob, BlobPropertyBag, CanvasRenderingContext2d, HtmlCanvasElement, HtmlVideoElement,
    MediaStream, MediaStreamConstraints, MediaStreamTrack, OscillatorType, Url, Window,
};

pub use decoder::Decoder;
pub use wasm_api::raysend_decode_luma;

mod decoder;
mod pool;
mod wasm_api;

use pool::{extract_jobs, DecodePool};

pub fn trigger_download(data: &[u8], file_name: &str) {
    let mime_type = mime_guess::from_path(file_name)
        .first_or_octet_stream()
        .to_string();
    let uint8 = js_sys::Uint8Array::new_with_length(data.len() as u32);
    uint8.copy_from(data);
    let parts = js_sys::Array::new();
    parts.push(&uint8);

    let props = BlobPropertyBag::new();
    props.set_type(&mime_type);

    let Ok(blob) = Blob::new_with_u8_array_sequence_and_options(&parts, &props) else {
        log("Failed to create download blob");
        return;
    };
    let Ok(url) = Url::create_object_url_with_blob(&blob) else {
        return;
    };

    let document = web_sys::window().unwrap().document().unwrap();
    let a = document.create_element("a").unwrap();
    a.set_attribute("href", &url).unwrap();
    a.set_attribute("download", file_name).unwrap();
    let a: web_sys::HtmlElement = a.dyn_into().unwrap();
    document.body().unwrap().append_child(&a).unwrap();
    a.click();
    if let Some(parent) = a.parent_node() {
        let _ = parent.remove_child(&a);
    }
    let _ = Url::revoke_object_url(&url);
}

fn chime(success: bool) {
    let Ok(audio) = web_sys::AudioContext::new() else {
        return;
    };
    let freq = if success { 880.0 } else { 420.0 };
    let oscillator = audio.create_oscillator().unwrap();
    let gain = audio.create_gain().unwrap();
    oscillator.connect_with_audio_node(&gain).unwrap();
    gain.connect_with_audio_node(&audio.destination()).unwrap();
    oscillator.frequency().set_value(freq);
    oscillator.set_type(OscillatorType::Sine);
    gain.gain().set_value(0.05);
    oscillator.start_with_when(audio.current_time()).unwrap();
    oscillator
        .stop_with_when(audio.current_time() + 0.18)
        .unwrap();
}

fn bump_capture_gen(window: &Window) -> u32 {
    let next = js_sys::Reflect::get(window, &"captureGen".into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as u32
        + 1;
    let _ = js_sys::Reflect::set(window, &"captureGen".into(), &JsValue::from(next));
    next
}

fn capture_gen(window: &Window) -> u32 {
    js_sys::Reflect::get(window, &"captureGen".into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as u32
}

fn video_object(facing: &str, exact_fps: bool) -> js_sys::Object {
    let video_obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &video_obj,
        &JsValue::from_str("facingMode"),
        &JsValue::from_str(facing),
    );
    let width = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&width, &"ideal".into(), &JsValue::from(1280));
    let _ = js_sys::Reflect::set(&video_obj, &"width".into(), &width);
    let height = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&height, &"ideal".into(), &JsValue::from(960));
    let _ = js_sys::Reflect::set(&video_obj, &"height".into(), &height);
    let fps = js_sys::Object::new();
    let key = if exact_fps { "exact" } else { "ideal" };
    let _ = js_sys::Reflect::set(&fps, &key.into(), &JsValue::from(60));
    let _ = js_sys::Reflect::set(&video_obj, &"frameRate".into(), &fps);
    video_obj
}

async fn open_camera(facing: &str) -> Result<MediaStream, ()> {
    let window = web_sys::window().ok_or(())?;
    let media_devices = window.navigator().media_devices().map_err(|_| ())?;

    for exact in [true, false] {
        let constraints = MediaStreamConstraints::new();
        constraints.set_video(&JsValue::from(video_object(facing, exact)));
        let Ok(promise) = media_devices.get_user_media_with_constraints(&constraints) else {
            continue;
        };
        if let Ok(stream) = JsFuture::from(promise).await {
            if let Ok(stream) = stream.dyn_into::<MediaStream>() {
                return Ok(stream);
            }
        }
    }

    let constraints = MediaStreamConstraints::new();
    let video_obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &video_obj,
        &JsValue::from_str("facingMode"),
        &JsValue::from_str(facing),
    );
    constraints.set_video(&JsValue::from(video_obj));
    let promise = media_devices
        .get_user_media_with_constraints(&constraints)
        .map_err(|_| ())?;
    JsFuture::from(promise)
        .await
        .ok()
        .and_then(|s| s.dyn_into::<MediaStream>().ok())
        .ok_or(())
}

fn apply_continuous_focus(track: &MediaStreamTrack) {
    let advanced = js_sys::Array::new();
    let focus = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&focus, &"focusMode".into(), &"continuous".into());
    advanced.push(&focus);
    let constraints = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&constraints, &"advanced".into(), &advanced);
    if let Ok(apply) = js_sys::Reflect::get(track, &"applyConstraints".into()) {
        if let Ok(apply) = apply.dyn_into::<js_sys::Function>() {
            let args = js_sys::Array::new();
            args.push(&constraints);
            let _ = apply.apply(track, &args);
        }
    }
}

fn read_track_info(stream: &MediaStream) -> String {
    let tracks = stream.get_video_tracks();
    let Some(track) = tracks.get(0).dyn_into::<MediaStreamTrack>().ok() else {
        return String::new();
    };
    apply_continuous_focus(&track);
    let Ok(get_settings) = js_sys::Reflect::get(&track, &"getSettings".into()) else {
        return String::new();
    };
    let Ok(get_settings) = get_settings.dyn_into::<js_sys::Function>() else {
        return String::new();
    };
    let Ok(settings) = get_settings.call0(&track) else {
        return String::new();
    };
    let w = js_sys::Reflect::get(&settings, &"width".into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as u32;
    let h = js_sys::Reflect::get(&settings, &"height".into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as u32;
    let fps = js_sys::Reflect::get(&settings, &"frameRate".into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .round() as u32;
    if w == 0 {
        String::new()
    } else {
        format!("{w}×{h}@{fps}")
    }
}

fn schedule_frame(window: &Window, video: &HtmlVideoElement, cb: &JsValue) {
    if let Ok(rvfc) = js_sys::Reflect::get(video, &"requestVideoFrameCallback".into()) {
        if let Ok(rvfc) = rvfc.dyn_into::<js_sys::Function>() {
            let args = js_sys::Array::new();
            args.push(cb);
            let _ = rvfc.apply(video, &args);
            return;
        }
    }
    if let Ok(cb) = cb.clone().dyn_into::<js_sys::Function>() {
        let _ = window.request_animation_frame(&cb);
    }
}

pub fn stop_receiving() {
    let Some(window) = web_sys::window() else {
        return;
    };
    bump_capture_gen(&window);

    if let Ok(stream_val) = js_sys::Reflect::get(&window, &"stream".into()) {
        if let Ok(stream) = stream_val.dyn_into::<MediaStream>() {
            for track in stream.get_tracks().to_vec() {
                if let Ok(track) = track.dyn_into::<MediaStreamTrack>() {
                    track.stop();
                }
            }
        }
        let _ = js_sys::Reflect::delete_property(&window, &"stream".into());
    }

    if let Some(document) = window.document() {
        if let Some(canvas) = document
            .get_element_by_id("scan-canvas")
            .and_then(|el| el.dyn_into::<HtmlCanvasElement>().ok())
        {
            if let Ok(Some(ctx)) = canvas.get_context("2d") {
                if let Ok(ctx) = ctx.dyn_into::<CanvasRenderingContext2d>() {
                    ctx.clear_rect(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);
                }
            }
        }
        if let Some(video) = document
            .get_element_by_id("scan-video")
            .and_then(|el| el.dyn_into::<HtmlVideoElement>().ok())
        {
            video.set_src_object(None);
        }
    }

    RECEIVE_UI.write().scanning = false;
}

pub fn switch_camera() {
    let mut camera = CAMERA_FACING.write();
    *camera = if *camera == "environment" {
        "user".to_string()
    } else {
        "environment".to_string()
    };
    drop(camera);

    if RECEIVE_UI.read().scanning {
        stop_receiving();
        wasm_bindgen_futures::spawn_local(start_receiving());
    }
}

pub async fn start_receiving() {
    stop_receiving();
    *RECEIVE_RESULT.write() = None;
    {
        let mut ui = RECEIVE_UI.write();
        ui.status = ReceiveStatus::Requesting;
        ui.percent = 0.0;
        ui.unique = 0;
        ui.needed = 0;
        ui.symbol_mtu = 0;
        ui.camera_info.clear();
        ui.live_line.clear();
        ui.scanning = true;
    }

    let window = web_sys::window().unwrap();
    let facing_mode = CAMERA_FACING.read().clone();
    let stream = match open_camera(&facing_mode).await {
        Ok(stream) => stream,
        Err(()) => {
            RECEIVE_UI.write().status = ReceiveStatus::Denied;
            RECEIVE_UI.write().scanning = false;
            return;
        }
    };

    let camera_info = read_track_info(&stream);
    let _ = js_sys::Reflect::set(&window, &JsValue::from("stream"), &JsValue::from(&stream));

    let document = window.document().unwrap();
    let Some(video) = document
        .get_element_by_id("scan-video")
        .and_then(|el| el.dyn_into::<HtmlVideoElement>().ok())
    else {
        RECEIVE_UI.write().status = ReceiveStatus::ViewNotReady;
        RECEIVE_UI.write().scanning = false;
        return;
    };
    let Some(canvas) = document
        .get_element_by_id("scan-canvas")
        .and_then(|el| el.dyn_into::<HtmlCanvasElement>().ok())
    else {
        RECEIVE_UI.write().status = ReceiveStatus::ViewNotReady;
        RECEIVE_UI.write().scanning = false;
        return;
    };

    let ctx_opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&ctx_opts, &"willReadFrequently".into(), &JsValue::TRUE);
    let ctx = canvas
        .get_context_with_context_options("2d", &ctx_opts)
        .ok()
        .flatten()
        .and_then(|ctx| ctx.dyn_into::<CanvasRenderingContext2d>().ok());
    let Some(ctx) = ctx else {
        RECEIVE_UI.write().status = ReceiveStatus::CanvasUnavailable;
        RECEIVE_UI.write().scanning = false;
        return;
    };

    video.set_src_object(Some(&stream));
    let _ = video.play();
    {
        let mut ui = RECEIVE_UI.write();
        ui.status = ReceiveStatus::Aim;
        ui.camera_info = camera_info.clone();
    }

    let pool = DecodePool::global();
    pool.ensure();
    let decoder = Arc::new(Mutex::new(Some({
        let mut dec = Decoder::new();
        dec.stats_mut().camera = camera_info.clone();
        dec.stats_mut().workers = pool.worker_len() as u8;
        dec
    })));
    let transfer_start = Rc::new(Cell::new(0.0));
    let video_clone = video.clone();
    let finished = Arc::new(Mutex::new(false));
    let busy = Rc::new(Cell::new(false));
    let gen = bump_capture_gen(&window);

    let tick: Rc<Cell<Option<Closure<dyn FnMut()>>>> = Rc::new(Cell::new(None));
    let tick_hold = tick.clone();
    let window_clone = window.clone();
    let busy_flag = busy.clone();

    let closure = Closure::wrap(Box::new(move || {
        if capture_gen(&window_clone) != gen || *finished.lock().unwrap() {
            return;
        }
        if busy_flag.get() {
            if let Ok(mut guard) = decoder.lock() {
                if let Some(dec) = guard.as_mut() {
                    dec.note_busy_drops(1);
                }
            }
            reschedule_frame(&window_clone, &video_clone, &tick_hold);
            return;
        }
        busy_flag.set(true);

        let vw = video_clone.video_width();
        let vh = video_clone.video_height();
        if vw > 0 && vh > 0 {
            canvas.set_width(vw);
            canvas.set_height(vh);
            if ctx
                .draw_image_with_html_video_element_and_dw_and_dh(
                    &video_clone,
                    0.0,
                    0.0,
                    vw as f64,
                    vh as f64,
                )
                .is_ok()
            {
                let jobs = {
                    let mut decoder_guard = decoder.lock().unwrap();
                    if let Some(dec) = decoder_guard.as_mut() {
                        dec.note_capture();
                        dec.stats_mut().workers = pool.worker_len() as u8;
                    }
                    decoder_guard
                        .as_ref()
                        .map(|dec| extract_jobs(&ctx, dec.planned_crops(vw, vh), vw, vh))
                        .unwrap_or_default()
                };

                if pool.is_ready() {
                    let decoder = decoder.clone();
                    let finished = finished.clone();
                    let busy_flag = busy_flag.clone();
                    let tick_hold = tick_hold.clone();
                    let window_clone = window_clone.clone();
                    let video_clone = video_clone.clone();
                    let transfer_start = transfer_start.clone();
                    pool.dispatch(
                        jobs,
                        Box::new(move |results| {
                            if capture_gen(&window_clone) != gen || *finished.lock().unwrap() {
                                busy_flag.set(false);
                                return;
                            }
                            let stop =
                                apply_decoded(&decoder, &results, &finished, &transfer_start);
                            busy_flag.set(false);
                            if !stop && capture_gen(&window_clone) == gen {
                                reschedule_frame(&window_clone, &video_clone, &tick_hold);
                            }
                        }),
                    );
                    return;
                }

                let results = jobs.iter().map(pool::decode_job).collect::<Vec<_>>();
                if apply_decoded(&decoder, &results, &finished, &transfer_start) {
                    busy_flag.set(false);
                    return;
                }
            }
        }

        busy_flag.set(false);
        if capture_gen(&window_clone) == gen {
            reschedule_frame(&window_clone, &video_clone, &tick_hold);
        }
    }) as Box<dyn FnMut()>);

    schedule_frame(&window, &video, closure.as_ref().unchecked_ref());
    tick.set(Some(closure));
}

fn reschedule_frame(
    window: &Window,
    video: &HtmlVideoElement,
    tick_hold: &Rc<Cell<Option<Closure<dyn FnMut()>>>>,
) {
    if let Some(cb) = tick_hold.take() {
        schedule_frame(window, video, cb.as_ref().unchecked_ref());
        tick_hold.set(Some(cb));
    }
}

fn apply_decoded(
    decoder: &Arc<Mutex<Option<Decoder>>>,
    results: &[DecodedWindow],
    finished: &Arc<Mutex<bool>>,
    transfer_start: &Rc<Cell<f64>>,
) -> bool {
    let mut decoder_guard = decoder.lock().unwrap();
    let Some(dec) = decoder_guard.as_mut() else {
        return true;
    };
    let _ = dec.ingest_windows(results);
    mark_transfer_start(dec.unique_count(), transfer_start);

    if dec.legacy_detected() {
        RECEIVE_UI.write().status = ReceiveStatus::Failed("legacy".into());
        *finished.lock().unwrap() = true;
        chime(false);
        stop_receiving();
        return true;
    }

    if let Some(err) = dec.error() {
        RECEIVE_UI.write().status = ReceiveStatus::Failed(err.to_string());
        *finished.lock().unwrap() = true;
        chime(false);
        stop_receiving();
        return true;
    }

    if dec.is_finished() {
        *finished.lock().unwrap() = true;
        let elapsed = elapsed_ms(transfer_start);
        let stats = dec.stats().clone();
        let finished_file = decoder_guard.take().unwrap().take_finished().unwrap();
        drop(decoder_guard);
        match finished_file.decompressed() {
            Ok(bytes) => {
                let name = finished_file.get_name();
                trigger_download(&bytes, &name);
                let receipt = TransferReceipt {
                    name: name.clone(),
                    bytes: bytes.len() as u64,
                    elapsed_ms: elapsed,
                    stats,
                };
                *RECEIVE_RESULT.write() = Some(ReceiveResult {
                    name: name.clone(),
                    size: bytes.len() as u64,
                    data: bytes,
                    receipt: Some(receipt),
                });
                RECEIVE_UI.write().status = ReceiveStatus::Received;
                RECEIVE_UI.write().percent = 100.0;
                chime(true);
            }
            Err(err) => {
                RECEIVE_UI.write().status = ReceiveStatus::Failed(err);
                chime(false);
            }
        }
        stop_receiving();
        return true;
    }

    let unique = dec.unique_count();
    let needed = dec.needed();
    let symbol_mtu = u32::from(dec.symbol_mtu());
    let percent = if needed > 0 {
        ((unique as f32 / needed as f32) * 100.0).min(99.0)
    } else {
        0.0
    };
    let live = dec
        .stats()
        .live_line(elapsed_ms(transfer_start), current() == Lang::Zh);
    let mut ui = RECEIVE_UI.write();
    ui.unique = unique;
    ui.needed = needed;
    ui.symbol_mtu = symbol_mtu;
    ui.percent = percent;
    ui.live_line = live;
    ui.status = if needed == 0 {
        ReceiveStatus::Aim
    } else {
        ReceiveStatus::Scanning
    };
    false
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

fn mark_transfer_start(unique: usize, transfer_start: &Rc<Cell<f64>>) {
    if unique > 0 && transfer_start.get() == 0.0 {
        transfer_start.set(now_ms());
    }
}

fn elapsed_ms(transfer_start: &Rc<Cell<f64>>) -> u64 {
    let start = transfer_start.get();
    if start <= 0.0 {
        return 0;
    }
    (now_ms() - start).max(0.0).round() as u64
}

pub fn copy_receipt(json: &str, file_name: &str) {
    let window = web_sys::window();
    if let Some(window) = window {
        let clipboard = window.navigator().clipboard();
        let text = json.to_string();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = JsFuture::from(clipboard.write_text(&text)).await;
        });
        return;
    }
    trigger_download(json.as_bytes(), &format!("{file_name}.receipt.json"));
}
