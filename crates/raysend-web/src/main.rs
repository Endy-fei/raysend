//! RaySend 网页入口：主题、语言、发送/接收页切换。

#![allow(non_snake_case)]

use dioxus::prelude::*;
use raysend::i18n::{receive_status_text, send_status_text, t, LangSwitch, LANG};
use raysend::receive::{
    copy_receipt, start_receiving, stop_receiving, switch_camera, trigger_download,
};
use raysend::send::{self, PlayPage};
use raysend::utils::{format_bytes, set_panic_hook};
use raysend::{RECEIVE_RESULT, RECEIVE_UI, SEND_SESSION, SEND_STATUS};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

fn app() -> Element {
    let mut theme = use_signal(|| "light".to_string());
    let mut tab = use_signal(|| "send".to_string());
    let lang = *LANG.read();

    use_effect(move || {
        let window = web_sys::window().unwrap();
        let media_query = window
            .match_media("(prefers-color-scheme: dark)")
            .unwrap()
            .unwrap();

        theme.set(if media_query.matches() {
            "dark".into()
        } else {
            "light".into()
        });

        let closure = Closure::wrap(Box::new(move |event: web_sys::MediaQueryListEvent| {
            theme.set(if event.matches() {
                "dark".into()
            } else {
                "light".into()
            });
        }) as Box<dyn FnMut(_)>);

        media_query
            .add_listener_with_opt_callback(Some(closure.as_ref().unchecked_ref()))
            .unwrap();
        closure.forget();
    });

    use_effect(move || {
        let doc = web_sys::window().unwrap().document().unwrap();
        let html = doc.document_element().unwrap();
        html.set_attribute("data-theme", &theme.read()).unwrap();
        html.set_attribute("data-bs-theme", &theme.read()).unwrap();
        html.set_attribute("lang", LANG.read().html()).unwrap();
    });

    use_effect(move || {
        if *tab.read() != "receive" {
            stop_receiving();
        }
    });

    if SEND_SESSION.read().is_some() {
        return rsx! { PlayPage {} };
    }

    let is_send = *tab.read() == "send";
    let send_class = if is_send {
        "segment active"
    } else {
        "segment"
    };
    let receive_class = if is_send {
        "segment"
    } else {
        "segment active"
    };
    let theme_label = if *theme.read() == "dark" {
        t("theme_light")
    } else {
        t("theme_dark")
    };
    let receive = RECEIVE_UI.read().clone();
    let send_status = send_status_text(&SEND_STATUS.read());
    let result = RECEIVE_RESULT.read();
    let has_result = result.is_some();
    let result_name = result.as_ref().map(|r| r.name.clone()).unwrap_or_default();
    let result_size = result.as_ref().map(|r| r.size).unwrap_or(0);
    let receive_text = receive_status_text(&receive, result.as_ref());
    let saved_text = lang.saved(&result_name, &format_bytes(result_size));

    rsx! {
        div { class: "app-shell",
            header { class: "topbar",
                div { class: "brand",
                    span { class: "brand-mark", aria_hidden: "true" }
                    div {
                        h1 { "{t(\"brand\")}" }
                        p { class: "brand-sub", "{t(\"brand_sub\")}" }
                        p { class: "lede", "{t(\"lede\")}" }
                    }
                }
                div { class: "topbar-actions",
                    LangSwitch {}
                    button {
                        class: "icon-btn",
                        title: t("theme_toggle"),
                        onclick: move |_| {
                            theme.set(if *theme.read() == "dark" { "light".into() } else { "dark".into() });
                        },
                        "{theme_label}"
                    }
                    a {
                        class: "icon-btn",
                        href: "https://github.com/Endy-fei/raysend",
                        target: "_blank",
                        rel: "noreferrer",
                        title: "GitHub",
                        img {
                            src: "assets/images/GitHub-Mark.png",
                            alt: "GitHub",
                            class: "github-logo",
                        }
                    }
                }
            }

            div { class: "segmented", role: "tablist",
                button {
                    class: "{send_class}",
                    onclick: move |_| tab.set("send".into()),
                    "{t(\"send\")}"
                }
                button {
                    class: "{receive_class}",
                    onclick: move |_| tab.set("receive".into()),
                    "{t(\"receive\")}"
                }
            }

            if is_send {
                section { class: "card",
                    label { class: "dropzone", r#for: "file-selector",
                        input {
                            id: "file-selector",
                            class: "dropzone-input",
                            r#type: "file",
                            onchange: move |_| {
                                spawn(async move {
                                    send::read_file_content().await;
                                });
                            },
                        }
                        div { class: "dropzone-art", aria_hidden: "true",
                            span {}
                            span {}
                            span {}
                            span {}
                        }
                        h2 { "{t(\"choose_file\")}" }
                        p { "{t(\"dropzone_body\")}" }
                        span { class: "chip", "{t(\"max_size\")}" }
                    }
                    if !send_status.is_empty() {
                        p { class: "status-line", "{send_status}" }
                    }
                    ul { class: "tips",
                        li { "{t(\"tip_compress\")}" }
                        li { "{t(\"tip_grid\")}" }
                        li { "{t(\"tip_fountain\")}" }
                        li { "{t(\"tip_slow\")}" }
                    }
                }
            } else {
                section { class: "card receive-card",
                    div { class: "video-wrap",
                        video {
                            id: "scan-video",
                            playsinline: "true",
                            autoplay: "true",
                            muted: "true",
                            onclick: move |_| switch_camera(),
                        }
                        canvas { id: "scan-canvas", class: "scan-canvas" }
                        div { class: "video-frame", aria_hidden: "true" }
                        if receive.scanning {
                            div { class: "scan-live", "{t(\"live\")}" }
                        }
                    }
                    div { class: "progress-block",
                        div { class: "progress-track",
                            div {
                                class: "progress-fill",
                                style: format!("width: {:.1}%", receive.percent.max(0.0)),
                            }
                        }
                        p { class: "status-line", "{receive_text}" }
                    }
                    div { class: "receive-actions",
                        if receive.scanning {
                            button {
                                class: "btn btn-danger",
                                onclick: move |_| stop_receiving(),
                                "{t(\"stop\")}"
                            }
                        } else {
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| {
                                    spawn(async move {
                                        start_receiving().await;
                                    });
                                },
                                "{t(\"start_camera\")}"
                            }
                        }
                        button {
                            class: "btn btn-ghost",
                            onclick: move |_| switch_camera(),
                            "{t(\"flip_camera\")}"
                        }
                    }
                    if has_result {
                        div { class: "success-card",
                            p { "{saved_text}" }
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| {
                                    if let Some(result) = RECEIVE_RESULT.read().as_ref() {
                                        trigger_download(&result.data, &result.name);
                                    }
                                },
                                "{t(\"download_again\")}"
                            }
                            if let Some(receipt) = result.as_ref().and_then(|r| r.receipt.as_ref()) {
                                pre { class: "receipt", "{receipt.summary(lang == raysend::i18n::Lang::Zh)}" }
                                button {
                                    class: "btn btn-ghost",
                                    onclick: move |_| {
                                        if let Some(result) = RECEIVE_RESULT.read().as_ref() {
                                            if let Some(receipt) = result.receipt.as_ref() {
                                                copy_receipt(&receipt.to_json(), &result.name);
                                            }
                                        }
                                    },
                                    "{t(\"copy_receipt\")}"
                                }
                            }
                        }
                    }
                    p { class: "fine-print", "{t(\"video_hint\")}" }
                }
            }
        }
    }
}

fn main() {
    // 解码 Worker 应加载 public/qr-decode，不再实例化本包。若误加载仍不要挂页面。
    if web_sys::window().is_none() {
        return;
    }

    set_panic_hook();

    if let Some(spinner) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("spinner"))
    {
        let _ = spinner.set_attribute("style", "display: none;");
    }

    launch(app);
}
