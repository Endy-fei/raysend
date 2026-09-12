//! 网页 Canvas 绘制：像素由 `raysend-core` 生成，这里只负责贴到 `#qr-stage`。

use raysend_core::{render_qr_rgba_density, Density};
use wasm_bindgen::Clamped;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

/// 按 1×1 或 2×2 把协议载荷画到发送页画布。
pub fn paint_qr_stage(payloads: &[Vec<u8>], density: Density) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let Some(canvas) = document
        .get_element_by_id("qr-stage")
        .and_then(|el| el.dyn_into::<HtmlCanvasElement>().ok())
    else {
        return;
    };
    let Ok(Some(ctx_obj)) = canvas.get_context("2d") else {
        return;
    };
    let Ok(ctx) = ctx_obj.dyn_into::<CanvasRenderingContext2d>() else {
        return;
    };

    let css = canvas.client_width().max(280) as u32;
    let n = if payloads.len() >= 4 { 2 } else { 1 };
    let gap = if n == 2 { 10 } else { 0 };
    let cell = (css.saturating_sub(gap)) / n as u32;

    canvas.set_width(css);
    canvas.set_height(css);
    ctx.clear_rect(0.0, 0.0, css as f64, css as f64);

    for (i, payload) in payloads.iter().take(n * n).enumerate() {
        let col = (i % n) as u32;
        let row = (i / n) as u32;
        if let Some((px, mut pixels)) = render_qr_rgba_density(payload, cell, density) {
            let x = col * (cell + gap) + cell.saturating_sub(px) / 2;
            let y = row * (cell + gap) + cell.saturating_sub(px) / 2;
            if let Ok(image) =
                ImageData::new_with_u8_clamped_array_and_sh(Clamped(pixels.as_mut_slice()), px, px)
            {
                let _ = ctx.put_image_data(&image, x as f64, y as f64);
            }
        }
    }
}
