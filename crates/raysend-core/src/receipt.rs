//! 一次接收的回执：捕获率、解码 fps、快路径命中。用来决定要不要提高默认档。

use crate::format::{format_bytes, format_duration};
use crate::scan::ScanSlice;

/// 接收过程中累加的计数。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReceiveStats {
    /// 送到解码的相机帧（含随后被合并丢掉的）。
    pub captures: u64,
    /// 因上一帧还在解而丢掉的帧。
    pub busy_drops: u64,
    pub windows: u64,
    pub hits: u64,
    pub tracked_ok: u64,
    pub tracked_fail: u64,
    pub discover: u64,
    pub payloads: u64,
    pub accepted: u64,
    pub unique: usize,
    pub needed: usize,
    pub symbol_mtu: u16,
    pub workers: u8,
    pub camera: String,
}

impl ReceiveStats {
    pub fn add_slice(&mut self, slice: ScanSlice) {
        self.windows += slice.windows;
        self.hits += slice.hits;
        self.tracked_ok += slice.tracked_ok;
        self.tracked_fail += slice.tracked_fail;
        self.discover += slice.discover;
        self.payloads += slice.payloads;
    }

    pub fn live_line(&self, elapsed_ms: u64, zh: bool) -> String {
        let receipt = TransferReceipt {
            name: String::new(),
            bytes: 0,
            elapsed_ms,
            stats: self.clone(),
        };
        if zh {
            format!(
                "捕获 {:.0}% · 取帧 {:.0} fps · 快路径 {:.0}%",
                receipt.catch_rate() * 100.0,
                receipt.capture_fps(),
                receipt.tracked_rate() * 100.0
            )
        } else {
            format!(
                "catch {:.0}% · capture {:.0} fps · track {:.0}%",
                receipt.catch_rate() * 100.0,
                receipt.capture_fps(),
                receipt.tracked_rate() * 100.0
            )
        }
    }
}

/// 传完后的一份可复制 JSON 回执。
#[derive(Clone, Debug, PartialEq)]
pub struct TransferReceipt {
    pub name: String,
    pub bytes: u64,
    pub elapsed_ms: u64,
    pub stats: ReceiveStats,
}

impl TransferReceipt {
    pub fn elapsed_secs(&self) -> f64 {
        self.elapsed_ms as f64 / 1000.0
    }

    pub fn kbps(&self) -> f64 {
        if self.elapsed_ms == 0 {
            return 0.0;
        }
        self.bytes as f64 / 1024.0 / self.elapsed_secs()
    }

    /// 解出码的窗口 / 全部窗口。不是发送端曝光帧的比例。
    pub fn catch_rate(&self) -> f64 {
        ratio(self.stats.hits, self.stats.windows)
    }

    /// 送到解码的相机帧 / 时间。这才是取帧节奏。
    pub fn capture_fps(&self) -> f64 {
        if self.elapsed_ms == 0 {
            return 0.0;
        }
        self.stats.captures as f64 / self.elapsed_secs()
    }

    /// 解码窗 / 时间。宫格时会高于 `capture_fps`，不要当成相机帧率。
    pub fn decode_fps(&self) -> f64 {
        if self.elapsed_ms == 0 {
            return 0.0;
        }
        self.stats.windows as f64 / self.elapsed_secs()
    }

    pub fn tracked_rate(&self) -> f64 {
        ratio(
            self.stats.tracked_ok,
            self.stats.tracked_ok + self.stats.tracked_fail,
        )
    }

    pub fn overhead(&self) -> f64 {
        if self.stats.needed == 0 {
            return 0.0;
        }
        self.stats.unique as f64 / self.stats.needed as f64
    }

    pub fn summary(&self, zh: bool) -> String {
        if zh {
            format!(
                "{} · {} · {:.1} KB/s\n捕获 {:.0}% · 取帧 {:.0} fps · 窗 {:.0}/s · 快路径 {:.0}%\n{} 次取帧 · {} 次忙丢 · {} Worker",
                format_bytes(self.bytes),
                format_duration(self.elapsed_secs().ceil() as u64),
                self.kbps(),
                self.catch_rate() * 100.0,
                self.capture_fps(),
                self.decode_fps(),
                self.tracked_rate() * 100.0,
                self.stats.captures,
                self.stats.busy_drops,
                self.stats.workers
            )
        } else {
            format!(
                "{} · {} · {:.1} KB/s\ncatch {:.0}% · capture {:.0} fps · windows {:.0}/s · track {:.0}%\n{} captures · {} busy · {} workers",
                format_bytes(self.bytes),
                format_duration(self.elapsed_secs().ceil() as u64),
                self.kbps(),
                self.catch_rate() * 100.0,
                self.capture_fps(),
                self.decode_fps(),
                self.tracked_rate() * 100.0,
                self.stats.captures,
                self.stats.busy_drops,
                self.stats.workers
            )
        }
    }

    pub fn to_json(&self) -> String {
        format!(
            "{{\n  \"name\": \"{}\",\n  \"bytes\": {},\n  \"elapsed_ms\": {},\n  \"kbps\": {:.1},\n  \"unique\": {},\n  \"needed\": {},\n  \"overhead\": {:.3},\n  \"captures\": {},\n  \"busy_drops\": {},\n  \"windows\": {},\n  \"hits\": {},\n  \"catch\": {:.3},\n  \"tracked_ok\": {},\n  \"tracked_fail\": {},\n  \"tracked\": {:.3},\n  \"discover\": {},\n  \"payloads\": {},\n  \"accepted\": {},\n  \"capture_fps\": {:.1},\n  \"decode_fps\": {:.1},\n  \"workers\": {},\n  \"camera\": \"{}\"\n}}\n",
            json_escape(&self.name),
            self.bytes,
            self.elapsed_ms,
            self.kbps(),
            self.stats.unique,
            self.stats.needed,
            self.overhead(),
            self.stats.captures,
            self.stats.busy_drops,
            self.stats.windows,
            self.stats.hits,
            self.catch_rate(),
            self.stats.tracked_ok,
            self.stats.tracked_fail,
            self.tracked_rate(),
            self.stats.discover,
            self.stats.payloads,
            self.stats.accepted,
            self.capture_fps(),
            self.decode_fps(),
            self.stats.workers,
            json_escape(&self.stats.camera)
        )
    }
}

fn ratio(num: u64, den: u64) -> f64 {
    if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    }
}

fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_and_json() {
        let receipt = TransferReceipt {
            name: "a\"b.bin".into(),
            bytes: 1024 * 50,
            elapsed_ms: 2000,
            stats: ReceiveStats {
                captures: 100,
                busy_drops: 4,
                windows: 80,
                hits: 56,
                tracked_ok: 40,
                tracked_fail: 10,
                discover: 2,
                payloads: 60,
                accepted: 50,
                unique: 50,
                needed: 48,
                symbol_mtu: 1465,
                workers: 2,
                camera: "1280x720".into(),
            },
        };
        assert!((receipt.kbps() - 25.0).abs() < 0.01);
        assert!((receipt.catch_rate() - 0.7).abs() < 0.001);
        assert!((receipt.capture_fps() - 50.0).abs() < 0.01);
        assert!((receipt.decode_fps() - 40.0).abs() < 0.01);
        assert!((receipt.tracked_rate() - 0.8).abs() < 0.001);
        let json = receipt.to_json();
        assert!(json.contains("\"catch\": 0.700"));
        assert!(json.contains("a\\\"b.bin"));
        assert!(json.contains("\"capture_fps\": 50.0"));
        assert!(json.contains("\"decode_fps\": 40.0"));
    }
}
