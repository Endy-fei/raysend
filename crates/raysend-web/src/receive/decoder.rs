//! 网页接收解码器：复用 core，避免在 WASM 里再实现一遍喷泉码。

pub use raysend_core::Decoder;
