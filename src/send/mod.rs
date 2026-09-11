#![allow(non_snake_case)]

pub mod encoder;

use crate::compress;
use crate::fountain::FountainSender;
use crate::i18n::{t, LangSwitch, LANG};
use crate::utils::{format_bytes, hash_bytes, log};
use crate::SendStatus;
use crate::SEND_SESSION;
use crate::SEND_STATUS;
use dioxus::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

pub struct SendSession {
    pub file_name: String,
    pub orig_size: u64,
    pub compressed_size: u64,
    fountain: FountainSender,
}

impl SendSession {
    pub fn from_compressed(file_name: String, orig_size: u64, compressed: Vec<u8>) -> Self {
        let hash = hash_bytes(&compressed);
        let compressed_size = compressed.len() as u64;
        let fountain = FountainSender::new(&file_name, orig_size as u32, hash, &compressed);
        Self {
            file_name,
            orig_size,
            compressed_size,
            fountain,
        }
    }

    pub fn next_payloads(&mut self, n: usize) -> Vec<Vec<u8>> {
        (0..n).map(|_| self.fountain.next_payload()).collect()
    }

    pub fn estimate_seconds(&self, fps: u32, grid: u8) -> u64 {
        let rate = crate::fountain::SYMBOL_MTU as u64 * fps as u64 * grid as u64 * 7 / 10;
        self.compressed_size.div_ceil(rate.max(1))
    }
}

fn default_grid() -> u8 {
    web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .map(|w| if w >= 720.0 { 4 } else { 1 })
        .unwrap_or(1)
}

pub fn PlayPage() -> Element {
    let mut playing = use_signal(|| true);
    let mut fps = use_signal(|| 12u32);
    let mut grid = use_signal(default_grid);
    let mut tick = use_signal(|| 0u64);
    let mut running = use_signal(|| true);

    use_hook(|| {
        spawn(async move {
            while *running.read() {
                let wait = (1000 / (*fps.read()).max(1)).max(40);
                gloo_timers::future::TimeoutFuture::new(wait).await;
                if !*running.read() {
                    break;
                }
                if *playing.read() {
                    *tick.write() += 1;
                }
            }
        });
    });

    use_effect(move || {
        let _ = *tick.read();
        let cells = if *grid.read() == 4 { 4 } else { 1 };
        let payloads = {
            let mut session = SEND_SESSION.write();
            session
                .as_mut()
                .map(|s| s.next_payloads(cells))
                .unwrap_or_default()
        };
        if !payloads.is_empty() {
            encoder::paint_qr_stage(&payloads);
        }
    });

    let session_info = SEND_SESSION.read();
    let Some(session) = session_info.as_ref() else {
        return rsx! { div {} };
    };

    let lang = *LANG.read();
    let fps_now = *fps.read();
    let grid_now = *grid.read();
    let eta = session.estimate_seconds(fps_now, grid_now);
    let ratio = if session.orig_size == 0 {
        1.0
    } else {
        session.compressed_size as f64 / session.orig_size as f64
    };
    let compress_label = lang.after_compress(ratio * 100.0);
    let grid_label = if grid_now == 4 {
        t("grid_quad")
    } else {
        t("grid_single")
    };
    let play_label = if *playing.read() {
        t("pause")
    } else {
        t("play")
    };
    let grid_action = if grid_now == 4 {
        t("use_single")
    } else {
        t("use_quad")
    };
    let hint = if grid_now == 4 {
        t("hint_quad")
    } else {
        t("hint_single")
    };
    let transfer_line = lang.transfer_line(
        &format_bytes(session.orig_size),
        &format_bytes(session.compressed_size),
        &lang.duration(eta),
    );
    let file_name = session.file_name.clone();
    let back_label = t("back");
    let speed_label = t("speed");

    rsx! {
        div { class: "play-shell",
            header { class: "play-top",
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        running.set(false);
                        *SEND_SESSION.write() = None;
                    },
                    "{back_label}"
                }
                div { class: "play-file",
                    strong { "{file_name}" }
                    span { "{transfer_line}" }
                }
                LangSwitch {}
            }

            div { class: "qr-stage-wrap",
                canvas { id: "qr-stage", class: "qr-stage-canvas" }
                div { class: "qr-hint", "{hint}" }
            }

            div { class: "dock",
                div { class: "dock-stats",
                    span { class: "pill", "{fps_now} fps" }
                    span { class: "pill", "{grid_label}" }
                    span { class: "pill muted", "{compress_label}" }
                }
                div { class: "dock-controls",
                    button {
                        class: "btn btn-icon",
                        onclick: move |_| {
                            let next = !*playing.read();
                            playing.set(next);
                        },
                        "{play_label}"
                    }
                    label { class: "field",
                        span { "{speed_label}" }
                        select {
                            value: "{fps_now}",
                            onchange: move |evt| {
                                if let Ok(v) = evt.value().parse::<u32>() {
                                    fps.set(v);
                                }
                            },
                            option { value: "6", "6 fps" }
                            option { value: "8", "8 fps" }
                            option { value: "10", "10 fps" }
                            option { value: "12", "12 fps" }
                            option { value: "15", "15 fps" }
                            option { value: "20", "20 fps" }
                        }
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| {
                            grid.set(if *grid.read() == 4 { 1 } else { 4 });
                        },
                        "{grid_action}"
                    }
                }
            }
        }
    }
}

async fn yield_ui() {
    gloo_timers::future::TimeoutFuture::new(40).await;
}

pub async fn read_file_content() {
    *SEND_STATUS.write() = SendStatus::Reading;
    yield_ui().await;

    let window = web_sys::window().expect("no global window");
    let document = window.document().expect("document");
    let filelist = document
        .get_element_by_id("file-selector")
        .expect("file-selector")
        .dyn_into::<web_sys::HtmlInputElement>()
        .unwrap()
        .files();

    let Some(filelist) = filelist else {
        *SEND_STATUS.write() = SendStatus::NoFile;
        return;
    };
    let Some(file) = filelist.get(0) else {
        *SEND_STATUS.write() = SendStatus::NoFile;
        return;
    };

    let file_name = file.name();
    log(&file_name);

    const MAX_FILE_SIZE_MB: u64 = 20;
    const MAX_FILE_SIZE: u64 = MAX_FILE_SIZE_MB * 1024 * 1024;
    let file_size = file.size() as u64;
    if file_size == 0 {
        *SEND_STATUS.write() = SendStatus::EmptyFile;
        return;
    }
    if file_size > MAX_FILE_SIZE {
        *SEND_STATUS.write() = SendStatus::TooLarge(format_bytes(file_size));
        return;
    }

    let file_reader = web_sys::FileReader::new().unwrap();
    let fr_c = file_reader.clone();
    let (rx, tx) = futures::channel::oneshot::channel();
    let onloadend_cb: Closure<dyn FnMut()> = Closure::new({
        let mut rx = Some(rx);
        move || {
            let array = js_sys::Uint8Array::new(&fr_c.result().unwrap());
            let _ = rx
                .take()
                .expect("file read channel already used")
                .send(array.to_vec());
        }
    });
    file_reader.set_onloadend(Some(onloadend_cb.as_ref().unchecked_ref()));
    onloadend_cb.forget();
    file_reader
        .read_as_array_buffer(&file)
        .expect("blob not readable");

    let data = tx.await.unwrap();
    *SEND_STATUS.write() = SendStatus::Compressing(format_bytes(data.len() as u64));
    yield_ui().await;
    let orig_size = data.len() as u64;
    let compressed = compress::compress(data);

    *SEND_STATUS.write() = SendStatus::Building;
    yield_ui().await;
    let session = SendSession::from_compressed(file_name, orig_size, compressed);
    *SEND_SESSION.write() = Some(session);
    *SEND_STATUS.write() = SendStatus::Idle;
}
