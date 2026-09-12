//! 取景框：每个正在解码的码画一副四角括号，颜色按宫格阅读顺序固定。

use std::cell::RefCell;

use raysend_core::ScanRegion;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlVideoElement};

const FADE_MS: f64 = 700.0;
const COLORS: [&str; 9] = [
    "#42e8ff", "#54ff7e", "#ffd94a", "#ff6ad5", "#c08bff", "#ff9a4d", "#8dff4a", "#ff5f5f",
    "#ffffff",
];

#[derive(Clone, Copy)]
struct Mark {
    region: ScanRegion,
    seen: f64,
}

thread_local! {
    static MARKS: RefCell<Vec<Mark>> = const { RefCell::new(Vec::new()) };
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

fn iou(a: ScanRegion, b: ScanRegion) -> f32 {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    let inter = (x1 - x0) * (y1 - y0);
    let union = a.w * a.h + b.w * b.h - inter;
    if union == 0 {
        0.0
    } else {
        inter as f32 / union as f32
    }
}

pub fn note_hits(regions: &[ScanRegion]) {
    if regions.is_empty() {
        return;
    }
    let now = now_ms();
    MARKS.with(|slot| {
        let mut marks = slot.borrow_mut();
        for region in regions {
            if let Some(mark) = marks
                .iter_mut()
                .find(|mark| iou(mark.region, *region) > 0.2)
            {
                mark.region = *region;
                mark.seen = now;
            } else {
                marks.push(Mark {
                    region: *region,
                    seen: now,
                });
            }
        }
        marks.retain(|mark| now - mark.seen < FADE_MS);
    });
}

pub fn clear() {
    MARKS.with(|slot| slot.borrow_mut().clear());
}

pub fn sync_preview_aspect(video: &HtmlVideoElement) {
    let vw = video.video_width();
    let vh = video.video_height();
    if vw == 0 || vh == 0 {
        return;
    }
    if let Some(wrap) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("scan-preview"))
    {
        let _ = wrap.set_attribute("style", &format!("aspect-ratio: {vw} / {vh}"));
    }
}

pub fn draw(video: &HtmlVideoElement) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(canvas) = document
        .get_element_by_id("detect-overlay")
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
    let cw = canvas.client_width() as f64;
    let ch = canvas.client_height() as f64;
    let vw = video.video_width() as f64;
    let vh = video.video_height() as f64;
    if cw < 2.0 || ch < 2.0 || vw < 2.0 || vh < 2.0 {
        return;
    }
    let dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0)
        .max(1.0);
    let pw = (cw * dpr).round() as u32;
    let ph = (ch * dpr).round() as u32;
    if canvas.width() != pw || canvas.height() != ph {
        canvas.set_width(pw);
        canvas.set_height(ph);
    }
    ctx.clear_rect(0.0, 0.0, pw as f64, ph as f64);

    let scale = (pw as f64 / vw).min(ph as f64 / vh);
    let off_x = (pw as f64 - vw * scale) / 2.0;
    let off_y = (ph as f64 - vh * scale) / 2.0;
    ctx.set_line_width((2.0 * dpr).max(2.0));
    set_str(&ctx, "lineCap", "round");
    set_str(&ctx, "lineJoin", "round");

    let now = now_ms();
    let mut marks = MARKS.with(|slot| slot.borrow().clone());
    marks.retain(|mark| now - mark.seen < FADE_MS);
    marks.sort_by(|a, b| {
        let dy = (a.region.y as i32 + a.region.h as i32 / 2)
            - (b.region.y as i32 + b.region.h as i32 / 2);
        let thresh = a.region.h.max(b.region.h) as i32 / 2;
        if dy.abs() > thresh {
            dy.cmp(&0)
        } else {
            (a.region.x + a.region.w / 2).cmp(&(b.region.x + b.region.w / 2))
        }
    });

    for (slot, mark) in marks.iter().enumerate() {
        let age = now - mark.seen;
        let color = COLORS[slot % COLORS.len()];
        set_str(&ctx, "strokeStyle", color);
        set_str(&ctx, "shadowColor", color);
        ctx.set_shadow_blur(4.0 * dpr);
        ctx.set_global_alpha(1.0 - age / FADE_MS);
        let pad = 0.06 * mark.region.w.max(mark.region.h) as f64 * scale;
        let x = off_x + mark.region.x as f64 * scale - pad;
        let y = off_y + mark.region.y as f64 * scale - pad;
        let w = mark.region.w as f64 * scale + 2.0 * pad;
        let h = mark.region.h as f64 * scale + 2.0 * pad;
        let len = 0.24 * w.min(h);
        ctx.begin_path();
        ctx.move_to(x, y + len);
        ctx.line_to(x, y);
        ctx.line_to(x + len, y);
        ctx.move_to(x + w - len, y);
        ctx.line_to(x + w, y);
        ctx.line_to(x + w, y + len);
        ctx.move_to(x + w, y + h - len);
        ctx.line_to(x + w, y + h);
        ctx.line_to(x + w - len, y + h);
        ctx.move_to(x + len, y + h);
        ctx.line_to(x, y + h);
        ctx.line_to(x, y + h - len);
        ctx.stroke();
    }
    ctx.set_global_alpha(1.0);
    ctx.set_shadow_blur(0.0);
}

fn set_str(ctx: &CanvasRenderingContext2d, key: &str, value: &str) {
    let _ = js_sys::Reflect::set(ctx, &JsValue::from_str(key), &JsValue::from_str(value));
}
