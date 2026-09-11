use crate::fountain::SYMBOL_MTU;
use crate::utils::format_bytes;
use crate::{ReceiveResult, ReceiveStatus, ReceiveUi, SendStatus};
use dioxus::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    En,
    Zh,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh",
        }
    }

    pub fn html(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh-CN",
        }
    }

    pub fn from_code(code: &str) -> Self {
        let lower = code.to_ascii_lowercase();
        if lower.starts_with("zh") {
            Lang::Zh
        } else {
            Lang::En
        }
    }

    fn pick(self, en: &'static str, zh: &'static str) -> &'static str {
        match self {
            Lang::En => en,
            Lang::Zh => zh,
        }
    }

    pub fn t(self, key: &'static str) -> &'static str {
        match key {
            "brand" => self.pick("RaySend", "光传"),
            "brand_sub" => self.pick("光传", "RaySend"),
            "lede" => self.pick(
                "Air-gapped files over the camera. Up to 20 MB.",
                "用摄像头离线传文件，最大 20 MB。",
            ),
            "theme_toggle" => self.pick("Toggle theme", "切换主题"),
            "theme_light" => self.pick("Light", "浅色"),
            "theme_dark" => self.pick("Dark", "深色"),
            "send" => self.pick("Send", "发送"),
            "receive" => self.pick("Receive", "接收"),
            "choose_file" => self.pick("Choose a file", "选择文件"),
            "dropzone_body" => self.pick(
                "Tap to browse, or drop a file here. Compressed in the browser, then played as animated QR codes. Nothing is uploaded.",
                "点击选择或将文件拖到这里。在浏览器内压缩后以动画二维码播放，文件不会上传到任何服务器。",
            ),
            "max_size" => self.pick("Max 20 MB", "最大 20 MB"),
            "tip_compress" => self.pick(
                "Text and documents usually transfer much faster after compression.",
                "文本和文档压缩后通常会快很多。",
            ),
            "tip_grid" => self.pick(
                "On a larger screen, use 2×2 grid for roughly 4× throughput.",
                "屏幕较大时可用 2×2 宫格，吞吐大约提升到 4 倍。",
            ),
            "tip_fountain" => self.pick(
                "The receiver only needs enough frames, not every frame in order.",
                "接收端只需扫到足够帧数，不必按顺序扫完每一帧。",
            ),
            "live" => self.pick("Live", "实时"),
            "receive_idle" => self.pick(
                "Start the camera, then point it at the sender.",
                "先开启相机，再对准发送端的二维码。",
            ),
            "start_camera" => self.pick("Start camera", "开启相机"),
            "stop" => self.pick("Stop", "停止"),
            "flip_camera" => self.pick("Flip camera", "切换镜头"),
            "download_again" => self.pick("Download again", "再次下载"),
            "video_hint" => self.pick(
                "Tap the video to switch between front and rear cameras.",
                "点按画面可在前后摄像头之间切换。",
            ),
            "back" => self.pick("← Back", "← 返回"),
            "play" => self.pick("Play", "播放"),
            "pause" => self.pick("Pause", "暂停"),
            "speed" => self.pick("Speed", "速度"),
            "grid_single" => self.pick("Single QR", "单码"),
            "grid_quad" => self.pick("2×2 grid", "2×2 宫格"),
            "use_single" => self.pick("Use 1×1", "使用 1×1"),
            "use_quad" => self.pick("Use 2×2", "使用 2×2"),
            "hint_single" => self.pick(
                "Keep the other camera on this code",
                "请将另一台设备的相机对准此码",
            ),
            "hint_quad" => self.pick(
                "2×2 fountain codes · any frames are enough",
                "2×2 喷泉码 · 扫到足够帧即可",
            ),
            "reading" => self.pick("Reading file…", "正在读取文件…"),
            "no_file" => self.pick("No file selected.", "未选择文件。"),
            "empty_file" => self.pick("Empty files cannot be sent.", "不能发送空文件。"),
            "building" => self.pick("Building fountain codes…", "正在生成喷泉码…"),
            "cam_requesting" => self.pick("Requesting camera…", "正在请求相机…"),
            "cam_unavailable" => self.pick(
                "Camera is not available in this browser.",
                "当前浏览器无法使用相机。",
            ),
            "cam_open_failed" => self.pick("Unable to open the camera.", "无法打开相机。"),
            "cam_denied" => self.pick("Camera permission denied.", "相机权限被拒绝。"),
            "view_not_ready" => self.pick("Receive view is not ready.", "接收界面尚未就绪。"),
            "canvas_unavailable" => self.pick("Canvas is unavailable.", "画布不可用。"),
            "aim" => self.pick(
                "Point the camera at the QR codes",
                "请将相机对准二维码",
            ),
            "progress_handshake" => self.pick("Looking for handshake QR…", "正在寻找握手二维码…"),
            "progress_finished" => self.pick("Finished.", "完成。"),
            "err_integrity" => self.pick("Integrity check failed.", "完整性校验失败。"),
            "err_missing_meta" => self.pick("Missing metadata.", "缺少元数据。"),
            "err_size" => self.pick("Decompressed size mismatch.", "解压后大小不一致。"),
            "err_blob" => self.pick("Failed to create download blob.", "无法创建下载文件。"),
            "lang_group" => self.pick("Language", "语言"),
            _ => key,
        }
    }

    pub fn duration(self, secs: u64) -> String {
        match self {
            Lang::En => {
                if secs >= 3600 {
                    format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
                } else if secs >= 60 {
                    format!("{}m {:02}s", secs / 60, secs % 60)
                } else {
                    format!("{secs}s")
                }
            }
            Lang::Zh => {
                if secs >= 3600 {
                    format!("{}小时{:02}分", secs / 3600, (secs % 3600) / 60)
                } else if secs >= 60 {
                    format!("{}分{:02}秒", secs / 60, secs % 60)
                } else {
                    format!("{secs}秒")
                }
            }
        }
    }

    pub fn file_too_large(self, size: &str) -> String {
        match self {
            Lang::En => format!("File too large: {size}. Maximum is 20 MB."),
            Lang::Zh => format!("文件过大：{size}。最大支持 20 MB。"),
        }
    }

    pub fn compressing(self, size: &str) -> String {
        match self {
            Lang::En => format!("Compressing {size}…"),
            Lang::Zh => format!("正在压缩 {size}…"),
        }
    }

    pub fn after_compress(self, percent: f64) -> String {
        match self {
            Lang::En => format!("{percent:.0}% after compress"),
            Lang::Zh => format!("压缩后 {percent:.0}%"),
        }
    }

    pub fn transfer_line(self, orig: &str, compressed: &str, eta: &str) -> String {
        match self {
            Lang::En => format!("{orig} → {compressed} · {eta} est."),
            Lang::Zh => format!("{orig} → {compressed} · 约 {eta}"),
        }
    }

    pub fn scanning(self, got: &str, need: &str) -> String {
        match self {
            Lang::En => format!("{got} of {need} · keep scanning"),
            Lang::Zh => format!("已收到 {got} / {need} · 请继续扫描"),
        }
    }

    pub fn received(self, name: &str) -> String {
        match self {
            Lang::En => format!("Received {name}"),
            Lang::Zh => format!("已收到 {name}"),
        }
    }

    pub fn saved(self, name: &str, size: &str) -> String {
        match self {
            Lang::En => format!("Saved {name} ({size})"),
            Lang::Zh => format!("已保存 {name}（{size}）"),
        }
    }

    pub fn symbols(self, unique: usize, needed: usize) -> String {
        match self {
            Lang::En => format!("{unique}/{needed} symbols"),
            Lang::Zh => format!("{unique}/{needed} 个符号"),
        }
    }

    pub fn error(self, key: &str) -> String {
        match key {
            "integrity" | "Integrity check failed" | "Integrity check failed." => {
                self.t("err_integrity").to_string()
            }
            "missing_metadata" | "Missing metadata" | "Missing metadata." => {
                self.t("err_missing_meta").to_string()
            }
            key if key == "size_mismatch"
                || key.starts_with("size_mismatch")
                || key.starts_with("Size mismatch") =>
            {
                self.t("err_size").to_string()
            }
            "blob" | "Failed to create download blob." => self.t("err_blob").to_string(),
            other => other.to_string(),
        }
    }
}

pub static LANG: GlobalSignal<Lang> = Signal::global(detect_lang);

pub fn current() -> Lang {
    *LANG.read()
}

pub fn t(key: &'static str) -> &'static str {
    current().t(key)
}

pub fn detect_lang() -> Lang {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                if let Ok(Some(saved)) = storage.get_item("raysend.lang") {
                    return Lang::from_code(&saved);
                }
                if let Ok(Some(saved)) = storage.get_item("qrtransfer.lang") {
                    return Lang::from_code(&saved);
                }
            }
            if let Some(code) = window.navigator().language() {
                return Lang::from_code(&code);
            }
        }
    }
    Lang::En
}

pub fn send_status_text(status: &SendStatus) -> String {
    let lang = current();
    match status {
        SendStatus::Idle => String::new(),
        SendStatus::Reading => lang.t("reading").to_string(),
        SendStatus::NoFile => lang.t("no_file").to_string(),
        SendStatus::EmptyFile => lang.t("empty_file").to_string(),
        SendStatus::TooLarge(size) => lang.file_too_large(size),
        SendStatus::Compressing(size) => lang.compressing(size),
        SendStatus::Building => lang.t("building").to_string(),
    }
}

pub fn receive_status_text(ui: &ReceiveUi, result: Option<&ReceiveResult>) -> String {
    let lang = current();
    match &ui.status {
        ReceiveStatus::Idle => lang.t("receive_idle").to_string(),
        ReceiveStatus::Requesting => lang.t("cam_requesting").to_string(),
        ReceiveStatus::Unavailable => lang.t("cam_unavailable").to_string(),
        ReceiveStatus::OpenFailed => lang.t("cam_open_failed").to_string(),
        ReceiveStatus::Denied => lang.t("cam_denied").to_string(),
        ReceiveStatus::ViewNotReady => lang.t("view_not_ready").to_string(),
        ReceiveStatus::CanvasUnavailable => lang.t("canvas_unavailable").to_string(),
        ReceiveStatus::Aim => lang.t("aim").to_string(),
        ReceiveStatus::Scanning => {
            if ui.needed == 0 {
                lang.t("aim").to_string()
            } else {
                lang.scanning(
                    &format_bytes(ui.unique as u64 * SYMBOL_MTU as u64),
                    &format_bytes(ui.needed as u64 * SYMBOL_MTU as u64),
                )
            }
        }
        ReceiveStatus::Received => {
            let name = result.map(|r| r.name.as_str()).unwrap_or_default();
            lang.received(name)
        }
        ReceiveStatus::Failed(key) => lang.error(key),
    }
}

pub fn set_lang(lang: Lang) {
    *LANG.write() = lang;
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let _ = storage.set_item("raysend.lang", lang.code());
            }
            if let Some(html) = window.document().and_then(|d| d.document_element()) {
                let _ = html.set_attribute("lang", lang.html());
            }
        }
    }
}

#[allow(non_snake_case)]
pub fn LangSwitch() -> Element {
    let lang = *LANG.read();
    let en_class = if lang == Lang::En { "active" } else { "" };
    let zh_class = if lang == Lang::Zh { "active" } else { "" };
    rsx! {
        div { class: "lang-switch", role: "group", "aria-label": t("lang_group"),
            button {
                class: "{en_class}",
                onclick: move |_| set_lang(Lang::En),
                "EN"
            }
            button {
                class: "{zh_class}",
                onclick: move |_| set_lang(Lang::Zh),
                "中文"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_and_chinese_differ() {
        assert_eq!(Lang::En.t("send"), "Send");
        assert_eq!(Lang::Zh.t("send"), "发送");
        assert_ne!(Lang::En.t("lede"), Lang::Zh.t("lede"));
        assert_eq!(Lang::En.error("integrity"), "Integrity check failed.");
        assert_eq!(Lang::Zh.error("missing_metadata"), "缺少元数据。");
        assert_eq!(Lang::Zh.error("size_mismatch"), "解压后大小不一致。");
    }
}
