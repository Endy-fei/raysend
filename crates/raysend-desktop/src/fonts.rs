//! 加载系统中文字体。iced 默认 shaping 对汉字要用 Advanced。

use iced::Font;

/// 给界面用的默认字体；找不到中文时回落到 iced 内置字体。
pub fn default_font() -> Font {
    #[cfg(target_os = "windows")]
    {
        Font::with_name("Microsoft YaHei")
    }
    #[cfg(target_os = "macos")]
    {
        Font::with_name("PingFang SC")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Font::with_name("Noto Sans CJK SC")
    }
}

/// 把系统字体文件喂给 iced，避免只靠家族名找不到字形。
pub fn load_cjk_bytes() -> Option<Vec<u8>> {
    for path in candidates() {
        if let Ok(bytes) = std::fs::read(&path) {
            return Some(bytes);
        }
    }
    None
}

fn candidates() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "windows")]
    {
        let fonts = std::env::var_os("WINDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"))
            .join("Fonts");
        for name in [
            "msyh.ttc",
            "msyh.ttf",
            "msjh.ttc",
            "simsun.ttc",
            "simhei.ttf",
            "Deng.ttf",
        ] {
            paths.push(fonts.join(name));
        }
    }
    #[cfg(target_os = "macos")]
    {
        paths.extend(
            [
                "/System/Library/Fonts/PingFang.ttc",
                "/System/Library/Fonts/STHeiti Light.ttc",
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
                "/Library/Fonts/Arial Unicode.ttf",
            ]
            .iter()
            .map(std::path::PathBuf::from),
        );
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        paths.extend(
            [
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/truetype/noto/NotoSansSC-Regular.otf",
                "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
                "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
            ]
            .iter()
            .map(std::path::PathBuf::from),
        );
    }
    paths
}
