//! 平台无关的 RaySend 核心。
//!
//! Windows / Android / iOS / 鸿蒙应把相机、窗口、文件对话框留在平台层。
//! 本 crate 只处理字节与灰度/RGBA 帧，不依赖浏览器或 WebView。
//!
//! 后续接入方式：
//! - Rust：直接依赖本 crate（见 `raysend-desktop`）
//! - Swift / Kotlin / ArkTS：链接 `raysend-ffi`（`staticlib` 或 `cdylib`），
//!   调用 `crates/raysend-ffi/include/raysend.h` 中的 C ABI
//!
//! 相机侧需把 NV21 / YUV_420_888 / BGRA 转成 8 位灰度或 RGBA。
//! 喷泉还原用 [`decoder::Decoder`]；只找码可用 [`scan::QrScan`] 或 [`scan::decode_qr_luma`]。

#[cfg(feature = "codec")]
pub mod compress;
#[cfg(feature = "codec")]
pub mod decoder;
pub mod format;
#[cfg(feature = "codec")]
pub mod fountain;
#[cfg(feature = "codec")]
pub mod hash;
#[cfg(feature = "codec")]
pub mod protocol;
#[cfg(feature = "codec")]
pub mod qr;
pub mod receipt;
pub mod scan;
mod scan_tracked;
#[cfg(feature = "codec")]
pub mod session;

#[cfg(feature = "codec")]
pub use compress::{compress, compress_if_smaller, decompress};
pub use format::{format_bytes, format_duration};
#[cfg(feature = "codec")]
pub use decoder::{Decoder, Finished};
#[cfg(feature = "codec")]
pub use fountain::{FountainReceiver, FountainSender, IngestResult, SYMBOL_MTU};
#[cfg(feature = "codec")]
pub use hash::hash_bytes;
#[cfg(feature = "codec")]
pub use protocol::{decode_container, encode_container, parse_frame, Frame, Meta};
#[cfg(feature = "codec")]
pub use qr::{
    compose_qr_grid, compose_qr_grid_density, render_qr_rgba, render_qr_rgba_density, Density,
    QR_VERSION,
};
pub use receipt::{ReceiveStats, TransferReceipt};
pub use scan::{
    decode_qr_luma, decode_qr_luma_ex, rgba_to_luma, DecodedWindow, PlannedCrop, QrScan, ScanHint,
    ScanRegion, ScanSlice, ScanWindow,
};
pub use scan_tracked::decode_from_quad;
#[cfg(feature = "codec")]
pub use session::{Outgoing, PrepareError, MAX_FILE_SIZE, MAX_FILE_SIZE_MB};

#[cfg(all(test, feature = "codec"))]
mod tests {
    use super::*;

    #[test]
    fn end_to_end_with_loss() {
        let file_name = "test_raysend.txt";
        let file_content = "Transfer your file from an air gapped computer to iOS/iPhone/iPad using only qrcode, no wifi/usb/bluetooth needed. This is a proof-of-concept project, implemented in Rust WebAssembly.";
        let original = Vec::from(file_content.as_bytes());
        let mut outgoing =
            Outgoing::prepare_with(file_name.into(), original.clone(), Density::Stable).unwrap();
        let mut receiver = FountainReceiver::new();

        let mut recovered = None;
        for _ in 0..2_000 {
            match receiver.ingest(&outgoing.next_payloads(1)[0]) {
                IngestResult::Complete { data, .. } => {
                    recovered = Some(data);
                    break;
                }
                IngestResult::Failed(err) => panic!("{err}"),
                _ => {}
            }
        }

        assert_eq!(recovered.expect("should recover"), original);
    }
}
