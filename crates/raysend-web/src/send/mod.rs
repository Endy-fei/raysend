//! 网页发送：选文件走浏览器 API，会话与喷泉码在 `raysend-core`。

#![allow(non_snake_case)]

pub mod encoder;

use std::cell::RefCell;

use crate::i18n::{t, LangSwitch, LANG};
use crate::utils::{format_bytes, log};
use crate::SendStatus;
use crate::SEND_SESSION;
use crate::SEND_STATUS;
use dioxus::prelude::*;
use raysend_core::session::{Outgoing, MAX_FILE_SIZE};
use raysend_core::Density;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

thread_local! {
    static OUTGOING: RefCell<Option<Outgoing>> = RefCell::new(None);
}

#[derive(Clone, PartialEq)]
pub struct SendSession {
    pub file_name: String,
    pub orig_size: u64,
    pub compressed_size: u64,
    pub density: Density,
    pub qr_version: i16,
    pub symbol_mtu: u16,
}

impl SendSession {
    fn from_outgoing(outgoing: &Outgoing) -> Self {
        Self {
            file_name: outgoing.file_name.clone(),
            orig_size: outgoing.orig_size,
            compressed_size: outgoing.compressed_size,
            density: outgoing.density,
            qr_version: outgoing.density.qr_version(),
            symbol_mtu: outgoing.symbol_mtu(),
        }
    }

    fn store(outgoing: Outgoing) -> Self {
        let meta = Self::from_outgoing(&outgoing);
        OUTGOING.with(|slot| *slot.borrow_mut() = Some(outgoing));
        meta
    }

    fn clear() {
        OUTGOING.with(|slot| *slot.borrow_mut() = None);
    }

    pub(super) fn next_payloads(n: usize) -> Vec<Vec<u8>> {
        OUTGOING.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .map(|outgoing| outgoing.next_payloads(n))
                .unwrap_or_default()
        })
    }

    fn set_density(density: Density) -> Option<Self> {
        OUTGOING.with(|slot| {
            let mut slot = slot.borrow_mut();
            let outgoing = slot.as_mut()?;
            outgoing.set_density(density);
            Some(Self::from_outgoing(outgoing))
        })
    }

    fn estimate_seconds(&self, fps: u32, grid: u8) -> u64 {
        let rate = self.symbol_mtu as u64 * fps as u64 * grid as u64 * 7 / 10;
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

async fn wait_frame() {
    let Some(window) = web_sys::window() else {
        gloo_timers::future::TimeoutFuture::new(16).await;
        return;
    };
    let (tx, rx) = futures::channel::oneshot::channel::<()>();
    let cb = Closure::once(move |_t: f64| {
        let _ = tx.send(());
    });
    if window
        .request_animation_frame(cb.as_ref().unchecked_ref())
        .is_ok()
    {
        cb.forget();
        let _ = rx.await;
    } else {
        gloo_timers::future::TimeoutFuture::new(16).await;
    }
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or_else(js_sys::Date::now)
}

pub fn PlayPage() -> Element {
    let mut playing = use_signal(|| true);
    let mut fps = use_signal(|| 60u32);
    let mut grid = use_signal(default_grid);
    let mut density = use_signal(|| Density::Fast);
    let mut fullscreen = use_signal(|| false);
    let mut tick = use_signal(|| 0u64);
    let mut running = use_signal(|| true);

    use_hook(|| {
        spawn(async move {
            let mut last_ui = now_ms();
            while *running.read() {
                wait_frame().await;
                if !*running.read() {
                    break;
                }
                encoder::pump(
                    *fps.read(),
                    *grid.read(),
                    *density.read(),
                    *playing.read(),
                );
                let t = now_ms();
                if t - last_ui > 200.0 {
                    last_ui = t;
                    *tick.write() += 1;
                }
            }
        });
    });

    use_effect(move || {
        let full = *fullscreen.read();
        if let Some(window) = web_sys::window() {
            if let Some(doc) = window.document() {
                if let Some(html) = doc.document_element() {
                    if full {
                        let _ = html.class_list().add_1("qr-full");
                    } else {
                        let _ = html.class_list().remove_1("qr-full");
                    }
                }
            }
        }
    });

    let session_info = SEND_SESSION.read();
    let Some(session) = session_info.as_ref() else {
        return rsx! { div {} };
    };

    let lang = *LANG.read();
    let fps_now = *fps.read();
    let grid_now = *grid.read();
    let density_now = *density.read();
    let eta = session.estimate_seconds(fps_now, grid_now);
    let ratio = if session.orig_size == 0 {
        1.0
    } else {
        session.compressed_size as f64 / session.orig_size as f64
    };
    let compress_label = lang.after_compress(ratio * 100.0);
    let grid_label = match grid_now {
        2 => t("grid_two"),
        4 => t("grid_quad"),
        6 => t("grid_six"),
        _ => t("grid_single"),
    };
    let play_label = if *playing.read() {
        t("pause")
    } else {
        t("play")
    };
    let hint = if *fullscreen.read() {
        t("hint_fullscreen")
    } else if grid_now > 1 {
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
    let density_label = t("density");
    let version_pill = format!("v{}", session.qr_version);
    let shell_class = if *fullscreen.read() {
        "play-shell qr-full"
    } else {
        "play-shell"
    };

    rsx! {
        div { class: "{shell_class}",
            header { class: "play-top",
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        running.set(false);
                        fullscreen.set(false);
                        SendSession::clear();
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
                canvas {
                    id: "qr-stage",
                    class: "qr-stage-canvas",
                    onclick: move |_| {
                        let next = !*fullscreen.peek();
                        fullscreen.set(next);
                    },
                }
                div { class: "qr-hint", "{hint}" }
            }

            div { class: "dock",
                div { class: "dock-stats",
                    span { class: "pill", "{fps_now} fps" }
                    span { class: "pill", "{grid_label}" }
                    span { class: "pill", "{version_pill}" }
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
                            option { value: "10", "10 fps" }
                            option { value: "15", "15 fps" }
                            option { value: "20", "20 fps" }
                            option { value: "24", "24 fps" }
                            option { value: "30", "30 fps" }
                            option { value: "55", "55 fps" }
                            option { value: "60", "60 fps" }
                        }
                    }
                    label { class: "field",
                        span { "{t(\"layout\")}" }
                        select {
                            value: "{grid_now}",
                            onchange: move |evt| {
                                if let Ok(v) = evt.value().parse::<u8>() {
                                    grid.set(v);
                                }
                            },
                            option { value: "1", "{t(\"grid_single\")}" }
                            option { value: "2", "{t(\"grid_two\")}" }
                            option { value: "4", "{t(\"grid_quad\")}" }
                            option { value: "6", "{t(\"grid_six\")}" }
                        }
                    }
                    label { class: "field",
                        span { "{density_label}" }
                        select {
                            value: "{density_now.id()}",
                            onchange: move |evt| {
                                if let Ok(v) = evt.value().parse::<u8>() {
                                    let next = Density::from_id(v);
                                    density.set(next);
                                    if let Some(meta) = SendSession::set_density(next) {
                                        *SEND_SESSION.write() = Some(meta);
                                    }
                                }
                            },
                            option { value: "0", "{t(\"density_stable\")}" }
                            option { value: "1", "{t(\"density_default\")}" }
                            option { value: "2", "{t(\"density_fast\")}" }
                        }
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

    *SEND_STATUS.write() = SendStatus::Building;
    yield_ui().await;
    log("正在生成喷泉码…");
    let outgoing = match Outgoing::prepare_with(file_name, data, Density::Fast) {
        Ok(outgoing) => outgoing,
        Err(_) => {
            *SEND_STATUS.write() = SendStatus::EmptyFile;
            return;
        }
    };
    let blocks = outgoing.source_blocks();
    let session = SendSession::store(outgoing);
    log(&format!("喷泉编码器已就绪（{blocks} 个源块）"));
    *SEND_SESSION.write() = Some(session);
    *SEND_STATUS.write() = SendStatus::Idle;
}
