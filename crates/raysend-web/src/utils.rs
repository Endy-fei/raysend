//! 网页端工具：日志与 panic hook。格式化/哈希复用 core。

pub use raysend_core::{format_bytes, format_duration, hash_bytes};

/// WASM 下把 panic 打到浏览器控制台。
pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}

#[cfg(target_arch = "wasm32")]
pub fn log(msg: &str) {
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(msg));
}

#[cfg(not(target_arch = "wasm32"))]
pub fn log(msg: &str) {
    println!("{msg}");
}
