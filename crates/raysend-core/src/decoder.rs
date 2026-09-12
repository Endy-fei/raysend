//! 扫码跟踪 + 喷泉接收。解码 Worker 只链接 [`crate::scan`]，不进本模块。

use crate::fountain::{FountainReceiver, IngestResult};
use crate::protocol::Meta;
use crate::receipt::{ReceiveStats, TransferReceipt};
use crate::scan::{rgba_to_luma, DecodedWindow, PlannedCrop, QrScan, ScanSlice, ScanWindow};

/// 喷泉解码成功后的原文与元数据。
pub struct Finished {
    pub meta: Meta,
    /// 已校验的原始文件字节。
    pub data: Vec<u8>,
}

impl Finished {
    pub fn get_name(&self) -> String {
        if self.meta.name.is_empty() {
            "download.bin".into()
        } else {
            self.meta.name.clone()
        }
    }

    pub fn decompressed(&self) -> Result<Vec<u8>, String> {
        if self.data.len() as u32 != self.meta.orig_len {
            return Err("size_mismatch".into());
        }
        Ok(self.data.clone())
    }
}

/// 扫码与喷泉接收的组合状态机。
pub struct Decoder {
    tracker: QrScan,
    fountain: FountainReceiver,
    finished: Option<Finished>,
    failed: Option<String>,
    legacy: bool,
    stats: ReceiveStats,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            tracker: QrScan::new(),
            fountain: FountainReceiver::new(),
            finished: None,
            failed: None,
            legacy: false,
            stats: ReceiveStats::default(),
        }
    }

    pub fn stats(&self) -> &ReceiveStats {
        &self.stats
    }

    pub fn stats_mut(&mut self) -> &mut ReceiveStats {
        &mut self.stats
    }

    pub fn note_capture(&mut self) {
        self.stats.captures = self.stats.captures.saturating_add(1);
        self.tracker.advance_frame();
    }

    pub fn note_busy_drops(&mut self, n: u64) {
        self.stats.busy_drops = self.stats.busy_drops.saturating_add(n);
    }

    pub fn receipt(&self, name: impl Into<String>, bytes: u64, elapsed_ms: u64) -> TransferReceipt {
        TransferReceipt {
            name: name.into(),
            bytes,
            elapsed_ms,
            stats: self.stats.clone(),
        }
    }

    pub fn unique_count(&self) -> usize {
        self.fountain.unique_count()
    }

    pub fn needed(&self) -> usize {
        self.fountain.needed()
    }

    pub fn symbol_mtu(&self) -> u16 {
        self.fountain.symbol_mtu()
    }

    pub fn has_meta(&self) -> bool {
        self.fountain.has_meta()
    }

    pub fn is_finished(&self) -> bool {
        self.finished.is_some()
    }

    pub fn error(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    pub fn legacy_detected(&self) -> bool {
        self.legacy
    }

    pub fn planned_crops(&self, width: u32, height: u32) -> Option<Vec<PlannedCrop>> {
        self.tracker.planned_crops(width, height)
    }

    pub fn live_regions(&self) -> Vec<crate::scan::ScanRegion> {
        self.tracker.live_regions()
    }

    pub fn ingest(&mut self, frame: &[u8]) -> IngestResult {
        if self.finished.is_some() || self.failed.is_some() {
            return IngestResult::Ignored;
        }
        let result = self.fountain.ingest(frame);
        match &result {
            IngestResult::Complete { meta, data } => {
                self.finished = Some(Finished {
                    meta: meta.clone(),
                    data: data.clone(),
                });
            }
            IngestResult::Failed(err) => {
                self.failed = Some(err.clone());
            }
            IngestResult::Legacy => {
                self.legacy = true;
            }
            _ => {}
        }
        result
    }

    /// 直接喂一帧协议字节。返回新接受的帧数 0 或 1。
    pub fn process_frame(&mut self, frame: &[u8]) -> usize {
        match self.ingest(frame) {
            IngestResult::Accepted { .. } | IngestResult::Meta | IngestResult::Complete { .. } => 1,
            _ => 0,
        }
    }

    pub fn scan_rgba(&mut self, width: u32, height: u32, rgba: &[u8]) -> usize {
        let expected = width as usize * height as usize * 4;
        if width == 0 || height == 0 || rgba.len() < expected {
            return 0;
        }
        let luma = rgba_to_luma(width, height, rgba);
        self.scan_luma(width, height, &luma)
    }

    pub fn scan_luma(&mut self, width: u32, height: u32, luma: &[u8]) -> usize {
        let payloads = self.tracker.scan_luma(width, height, luma);
        self.stats.add_slice(self.tracker.last_slice());
        self.feed_payloads(&payloads)
    }

    pub fn scan_windows(&mut self, windows: &[ScanWindow<'_>]) -> usize {
        let payloads = self.tracker.scan_windows(windows);
        self.stats.add_slice(self.tracker.last_slice());
        self.feed_payloads(&payloads)
    }

    pub fn add_slice(&mut self, slice: ScanSlice) {
        self.stats.add_slice(slice);
    }

    pub fn ingest_payloads(&mut self, payloads: &[Vec<u8>]) -> usize {
        self.feed_payloads(payloads)
    }

    pub fn ingest_windows(&mut self, windows: &[DecodedWindow]) -> usize {
        self.stats.add_slice(ScanSlice::from_windows(windows));
        let payloads = self.tracker.absorb_windows(windows);
        self.feed_payloads(&payloads)
    }

    /// 单个裁剪窗回传：更新喷泉，不把其它宫格跟踪框记成未命中。
    pub fn ingest_partial(&mut self, window: &DecodedWindow) -> usize {
        self.stats.add_slice(ScanSlice::from_windows(std::slice::from_ref(window)));
        let payloads = self.tracker.absorb_partial(std::slice::from_ref(window));
        self.feed_payloads(&payloads)
    }

    fn feed_payloads(&mut self, payloads: &[Vec<u8>]) -> usize {
        let mut counter = 0;
        for payload in payloads {
            counter += self.process_frame(payload);
        }
        self.stats.accepted = self.stats.accepted.saturating_add(counter as u64);
        self.stats.unique = self.fountain.unique_count();
        self.stats.needed = self.fountain.needed();
        self.stats.symbol_mtu = self.fountain.symbol_mtu();
        counter
    }

    pub fn take_finished(self) -> Option<Finished> {
        self.finished
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}
