use crate::utils::log;
use crate::{ReceiveResult, ReceiveStatus, CAMERA_FACING, RECEIVE_RESULT, RECEIVE_UI};
use dioxus::prelude::*;
use std::sync::{Arc, Mutex};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Blob, BlobPropertyBag, CanvasRenderingContext2d, HtmlCanvasElement, HtmlVideoElement,
    MediaStream, MediaStreamConstraints, MediaStreamTrack, OscillatorType, Url,
};

pub use decoder::Decoder;

mod decoder;

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

pub fn stop_receiving() {
    let Some(window) = web_sys::window() else {
        return;
    };

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

    if let Ok(id_val) = js_sys::Reflect::get(&window, &"intervalId".into()) {
        if let Some(id) = id_val.as_f64() {
            window.clear_interval_with_handle(id as i32);
        }
        let _ = js_sys::Reflect::delete_property(&window, &"intervalId".into());
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
        ui.scanning = true;
    }

    let window = web_sys::window().unwrap();
    let navigator = window.navigator();
    let Ok(media_devices) = navigator.media_devices() else {
        RECEIVE_UI.write().status = ReceiveStatus::Unavailable;
        RECEIVE_UI.write().scanning = false;
        return;
    };

    let facing_mode = CAMERA_FACING.read().clone();
    let constraints = MediaStreamConstraints::new();
    let video_obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(
        &video_obj,
        &JsValue::from_str("facingMode"),
        &JsValue::from_str(&facing_mode),
    );
    constraints.set_video(&JsValue::from(video_obj));

    let stream_promise = match media_devices.get_user_media_with_constraints(&constraints) {
        Ok(p) => p,
        Err(_) => {
            RECEIVE_UI.write().status = ReceiveStatus::OpenFailed;
            RECEIVE_UI.write().scanning = false;
            return;
        }
    };

    let stream = match JsFuture::from(stream_promise).await {
        Ok(s) => s.dyn_into::<MediaStream>().unwrap(),
        Err(_) => {
            RECEIVE_UI.write().status = ReceiveStatus::Denied;
            RECEIVE_UI.write().scanning = false;
            return;
        }
    };

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
    RECEIVE_UI.write().status = ReceiveStatus::Aim;

    let decoder = Arc::new(Mutex::new(Some(Decoder::new())));
    let video_clone = video.clone();
    let finished = Arc::new(Mutex::new(false));

    let interval_closure = Closure::wrap(Box::new(move || {
        if *finished.lock().unwrap() {
            return;
        }
        let vw = video_clone.video_width();
        let vh = video_clone.video_height();
        if vw == 0 || vh == 0 {
            return;
        }

        let max_dim = 800.0f64;
        let scale = (max_dim / vw.max(vh) as f64).min(1.0);
        let dw = ((vw as f64) * scale).round().max(1.0) as u32;
        let dh = ((vh as f64) * scale).round().max(1.0) as u32;
        canvas.set_width(dw);
        canvas.set_height(dh);
        if ctx
            .draw_image_with_html_video_element_and_dw_and_dh(
                &video_clone,
                0.0,
                0.0,
                dw as f64,
                dh as f64,
            )
            .is_err()
        {
            return;
        }
        let Ok(image_data) = ctx.get_image_data(0.0, 0.0, dw as f64, dh as f64) else {
            return;
        };

        let mut decoder_guard = decoder.lock().unwrap();
        let Some(dec) = decoder_guard.as_mut() else {
            return;
        };
        let _ = dec.scan(dw, dh, image_data.data().to_vec());

        if let Some(err) = dec.error() {
            RECEIVE_UI.write().status = ReceiveStatus::Failed(err.to_string());
            *finished.lock().unwrap() = true;
            chime(false);
            stop_receiving();
            return;
        }

        if dec.is_finished() {
            *finished.lock().unwrap() = true;
            let finished_file = decoder_guard.take().unwrap().take_finished().unwrap();
            drop(decoder_guard);
            match finished_file.decompressed() {
                Ok(bytes) => {
                    let name = finished_file.get_name();
                    trigger_download(&bytes, &name);
                    *RECEIVE_RESULT.write() = Some(ReceiveResult {
                        name: name.clone(),
                        size: bytes.len() as u64,
                        data: bytes,
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
            return;
        }

        let unique = dec.unique_count();
        let needed = dec.needed();
        let percent = if needed > 0 {
            ((unique as f32 / needed as f32) * 100.0).min(99.0)
        } else {
            0.0
        };
        let mut ui = RECEIVE_UI.write();
        ui.unique = unique;
        ui.needed = needed;
        ui.percent = percent;
        ui.status = if needed == 0 {
            ReceiveStatus::Aim
        } else {
            ReceiveStatus::Scanning
        };
    }) as Box<dyn FnMut()>);

    let interval_id = window
        .set_interval_with_callback_and_timeout_and_arguments_0(
            interval_closure.as_ref().unchecked_ref(),
            50,
        )
        .unwrap();
    let _ = js_sys::Reflect::set(
        &window,
        &JsValue::from("intervalId"),
        &JsValue::from(interval_id),
    );
    interval_closure.forget();
}
