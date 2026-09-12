//! 中英文案，键与网页端对齐；拖放说明改成「本机」而不是浏览器。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    En,
    #[default]
    Zh,
}

impl Lang {
    pub fn detect() -> Self {
        let loc = sys_locale::get_locale().unwrap_or_default();
        if loc.to_ascii_lowercase().starts_with("zh") {
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
                "Air-gapped files over the camera. Up to 64 MB.",
                "用摄像头离线传文件，最大 64 MB。",
            ),
            "theme_light" => self.pick("Light", "浅色"),
            "theme_dark" => self.pick("Dark", "深色"),
            "send" => self.pick("Send", "发送"),
            "receive" => self.pick("Receive", "接收"),
            "choose_file" => self.pick("Choose a file", "选择文件"),
            "dropzone_body" => self.pick(
                "Click to browse, or drop a file here. Compressed on this device, then played as animated QR codes. Nothing is uploaded.",
                "点击选择或将文件拖到这里。在本机压缩后以动画二维码播放，文件不会上传到任何服务器。",
            ),
            "max_size" => self.pick("Max 64 MB", "最大 64 MB"),
            "density" => self.pick("Density", "密度"),
            "hint_fullscreen" => self.pick(
                "Click the codes to go fullscreen",
                "点击二维码可全屏",
            ),
            "tip_slow" => self.pick(
                "Default is v27 @ 24. Use Fast + 60 only up close. If it stalls, drop density first.",
                "默认 v27 @ 24。近距再开快档和 60 fps。扫不动先降密度。",
            ),
            "err_legacy" => self.pick(
                "Sender is on an old RaySend protocol. Both devices need the new version.",
                "对方仍是旧版 RaySend 协议，两端都需要升级。",
            ),
            "tip_compress" => self.pick(
                "Text and documents usually transfer much faster after compression.",
                "文本和文档压缩后通常会快很多。",
            ),
            "tip_grid" => self.pick(
                "On a larger screen, use 4 or 6 codes. Cells flip out of phase so a torn exposure loses one code, not all of them.",
                "大屏用 4 或 6 码。格子错开翻页，一次糊帧只会丢掉一格。",
            ),
            "tip_fountain" => self.pick(
                "The receiver only needs enough frames, not every frame in order.",
                "接收端只需扫到足够帧数，不必按顺序扫完每一帧。",
            ),
            "live" => self.pick("LIVE", "实时"),
            "receive_idle" => self.pick(
                "Start the camera, then point it at the sender.",
                "先开启相机，再对准发送端的二维码。",
            ),
            "start_camera" => self.pick("Start camera", "开启相机"),
            "stop" => self.pick("Stop", "停止"),
            "camera" => self.pick("Camera", "摄像头"),
            "no_camera" => self.pick("No camera found", "未找到摄像头"),
            "download_again" => self.pick("Save again", "再次保存"),
            "copy_receipt" => self.pick("Copy receipt", "复制回执"),
            "receipt" => self.pick("Receipt", "回执"),
            "video_hint" => self.pick(
                "Pick a camera from the list, then start.",
                "从列表里选择摄像头，再开启。",
            ),
            "back" => self.pick("← Back", "← 返回"),
            "play" => self.pick("Play", "播放"),
            "pause" => self.pick("Pause", "暂停"),
            "speed" => self.pick("Speed", "速度"),
            "layout" => self.pick("Layout", "宫格"),
            "grid_single" => self.pick("1 code", "单码"),
            "grid_two" => self.pick("2 codes", "2 码"),
            "grid_quad" => self.pick("4 codes", "4 码"),
            "grid_six" => self.pick("6 codes", "6 码"),
            "use_single" => self.pick("Next layout", "下一宫格"),
            "use_quad" => self.pick("Next layout", "下一宫格"),
            "hint_single" => self.pick(
                "Keep the other camera on this code",
                "请将另一台设备的相机对准此码",
            ),
            "hint_quad" => self.pick(
                "Staggered fountain codes · any frames are enough",
                "错开翻格喷泉码 · 扫到足够帧即可",
            ),
            "reading" => self.pick("Reading file…", "正在读取文件…"),
            "building" => self.pick("Building fountain codes…", "正在生成喷泉码…"),
            "empty_file" => self.pick("Empty files cannot be sent.", "不能发送空文件。"),
            "cam_requesting" => self.pick("Opening camera…", "正在打开相机…"),
            "cam_open_failed" => self.pick("Unable to open the camera.", "无法打开相机。"),
            "cam_timeout" => self.pick(
                "Camera timed out. For OBS, click Start Virtual Camera and select it here. Also check Windows camera privacy.",
                "打开相机超时。若用 OBS，请先点「开始虚拟摄像机」并在列表里选中它；同时检查 Windows 的相机隐私权限。",
            ),
            "aim" => self.pick(
                "Point the camera at the QR codes",
                "请将相机对准二维码",
            ),
            "progress_finished" => self.pick("Finished.", "完成。"),
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
            Lang::En => format!("File too large: {size}. Maximum is 64 MB."),
            Lang::Zh => format!("文件过大：{size}。最大支持 64 MB。"),
        }
    }

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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
}
