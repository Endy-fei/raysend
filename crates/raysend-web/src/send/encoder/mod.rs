//! 网页发送画布：一模块一像素，整数倍放大；宫格错开翻页，避免一次曝光撕掉所有码。

use std::cell::RefCell;

use raysend_core::{clamp_grid, grid_dims, render_qr_native, Density};
use wasm_bindgen::Clamped;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

use super::SendSession;

struct Cell {
    px: u32,
    pixels: Vec<u8>,
}

struct Stage {
    staging: HtmlCanvasElement,
    cells: Vec<Option<Cell>>,
    grid: u8,
    density: Density,
    cursor: usize,
    next_at: f64,
}

thread_local! {
    static STAGE: RefCell<Option<Stage>> = const { RefCell::new(None) };
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

fn make_canvas() -> Option<HtmlCanvasElement> {
    let document = web_sys::window()?.document()?;
    document
        .create_element("canvas")
        .ok()?
        .dyn_into::<HtmlCanvasElement>()
        .ok()
}

fn stage_canvas() -> Option<HtmlCanvasElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id("qr-stage")?
        .dyn_into::<HtmlCanvasElement>()
        .ok()
}

/// 按发送 fps 错开翻格。每格仍是 txFps，相位错开，相机跨帧最多糊一格。
pub fn pump(fps: u32, grid: u8, density: Density, playing: bool) {
    if !playing {
        return;
    }
    let codes = clamp_grid(grid) as usize;
    let now = now_ms();
    let interval = 1000.0 / fps.max(1) as f64;
    let sub = interval / codes.max(1) as f64;

    STAGE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let reset = slot
            .as_ref()
            .map(|stage| stage.grid != clamp_grid(grid) || stage.density != density)
            .unwrap_or(true);
        if reset {
            let Some(staging) = make_canvas() else {
                return;
            };
            *slot = Some(Stage {
                staging,
                cells: (0..codes).map(|_| None).collect(),
                grid: clamp_grid(grid),
                density,
                cursor: 0,
                next_at: now,
            });
        }
        let Some(stage) = slot.as_mut() else {
            return;
        };
        if now - stage.next_at > interval {
            stage.next_at = now;
        }
        let mut flips = 0usize;
        while now >= stage.next_at && flips < codes {
            if let Some(payload) = SendSession::next_payloads(1).into_iter().next() {
                if let Some((px, pixels)) = render_qr_native(&payload, density) {
                    if stage.cursor < stage.cells.len() {
                        stage.cells[stage.cursor] = Some(Cell { px, pixels });
                    }
                    blit_stage(stage);
                }
            }
            stage.cursor = (stage.cursor + 1) % codes.max(1);
            stage.next_at += sub;
            flips += 1;
        }
    });
}

fn blit_stage(stage: &Stage) {
    let Some(canvas) = stage_canvas() else {
        return;
    };
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(ctx_obj)) = canvas.get_context("2d") else {
        return;
    };
    let Ok(ctx) = ctx_obj.dyn_into::<CanvasRenderingContext2d>() else {
        return;
    };
    let Ok(Some(stg_obj)) = stage.staging.get_context("2d") else {
        return;
    };
    let Ok(stg) = stg_obj.dyn_into::<CanvasRenderingContext2d>() else {
        return;
    };

    let (cols, rows) = grid_dims(stage.grid);
    let native = stage
        .cells
        .iter()
        .filter_map(|cell| cell.as_ref().map(|c| c.px))
        .max()
        .unwrap_or(21);
    let total_w = native * cols;
    let total_h = native * rows;
    if stage.staging.width() != total_w || stage.staging.height() != total_h {
        stage.staging.set_width(total_w);
        stage.staging.set_height(total_h);
    }

    let dpr = window.device_pixel_ratio().max(1.0);
    let full = window
        .document()
        .and_then(|doc| doc.document_element())
        .map(|el| el.class_list().contains("qr-full"))
        .unwrap_or(false);
    let budget_w = if full {
        window.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(680.0)
    } else {
        canvas.client_width().max(280) as f64
    };
    let budget_h = if full {
        window.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(680.0)
    } else {
        budget_w
    };
    let scale = ((budget_w * dpr) / total_w as f64)
        .min((budget_h * dpr) / total_h as f64)
        .floor()
        .max(1.0) as u32;
    let css_w = (total_w * scale) as f64 / dpr;
    let css_h = (total_h * scale) as f64 / dpr;
    let stretch = (budget_w / css_w).min(budget_h / css_h).max(1.0);

    canvas.set_width(total_w * scale);
    canvas.set_height(total_h * scale);
    let _ = canvas.set_attribute(
        "style",
        &format!(
            "width:{}px;height:{}px;image-rendering:auto;aspect-ratio:auto",
            css_w * stretch,
            css_h * stretch
        ),
    );

    for (i, cell) in stage.cells.iter().enumerate() {
        let Some(cell) = cell else {
            continue;
        };
        let mut pixels = cell.pixels.clone();
        let Ok(image) =
            ImageData::new_with_u8_clamped_array_and_sh(Clamped(pixels.as_mut_slice()), cell.px, cell.px)
        else {
            continue;
        };
        let cx = (i as u32 % cols) * native;
        let cy = (i as u32 / cols) * native;
        let _ = stg.put_image_data(&image, cx as f64, cy as f64);
    }
    ctx.set_image_smoothing_enabled(false);
    let _ = ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
        &stage.staging,
        0.0,
        0.0,
        canvas.width() as f64,
        canvas.height() as f64,
    );
}
