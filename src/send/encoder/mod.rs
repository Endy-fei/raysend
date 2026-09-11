use qrcode::{EcLevel, QrCode, Version};
use wasm_bindgen::Clamped;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

pub const QR_VERSION: i16 = 20;
const QUIET_ZONE: usize = 4;

pub fn paint_qr_stage(payloads: &[Vec<u8>]) {
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
        if let Some((px, mut pixels)) = render_qr_pixels(payload, cell) {
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

fn render_qr_pixels(data: &[u8], out_size: u32) -> Option<(u32, Vec<u8>)> {
    let code = QrCode::with_version(data, Version::Normal(QR_VERSION), EcLevel::M)
        .or_else(|_| QrCode::with_error_correction_level(data, EcLevel::M))
        .ok()?;
    let w = code.width();
    let n = (w + QUIET_ZONE * 2) as u32;
    let scale = (out_size / n).max(1);
    let px = n * scale;
    let mut buf = vec![255u8; (px * px * 4) as usize];

    for y in 0..w {
        for x in 0..w {
            if code[(x, y)] != qrcode::Color::Dark {
                continue;
            }
            let x0 = (x as u32 + QUIET_ZONE as u32) * scale;
            let y0 = (y as u32 + QUIET_ZONE as u32) * scale;
            for dy in 0..scale {
                for dx in 0..scale {
                    let idx = (((y0 + dy) * px + (x0 + dx)) * 4) as usize;
                    buf[idx] = 18;
                    buf[idx + 1] = 16;
                    buf[idx + 2] = 14;
                    buf[idx + 3] = 255;
                }
            }
        }
    }
    Some((px, buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_20_fits_protocol_payload() {
        let data = vec![7u8; 650];
        assert!(QrCode::with_version(data, Version::Normal(QR_VERSION), EcLevel::M).is_ok());
    }
}
