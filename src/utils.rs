use sha1::{Digest, Sha1};

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

pub fn hash_bytes(data: &[u8]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hasher.finalize().into()
}

#[allow(dead_code)]
pub fn hash_hex(data: &[u8]) -> String {
    format!("{:x}", {
        let mut hasher = Sha1::new();
        hasher.update(data);
        hasher.finalize()
    })
}

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

pub fn format_duration(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}
