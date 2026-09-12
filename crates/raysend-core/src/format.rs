//! 体积与时长的展示格式，供各端 UI 共用。

/// 将字节数格式化为 `B` / `KB` / `MB`。
pub fn format_bytes(n: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const KB: f64 = 1024.0;
    if n as f64 >= MB {
        format!("{:.2} MB", n as f64 / MB)
    } else if n as f64 >= KB {
        format!("{:.1} KB", n as f64 / KB)
    } else {
        format!("{n} B")
    }
}

/// 将秒数格式化为 `Xh YYm` / `Xm YYs` / `Xs`。
pub fn format_duration(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}
