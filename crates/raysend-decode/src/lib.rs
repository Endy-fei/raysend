//! 网页解码 Worker 用的小 WASM：只导出 `raysend_decode_luma`。

use raysend_core::{decode_from_quad, decode_qr_luma_ex, ScanHint};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn raysend_decode_luma(
    width: u32,
    height: u32,
    luma: &[u8],
    discover: bool,
    modules: u16,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    x3: i32,
    y3: i32,
) -> js_sys::Object {
    let hint = (modules >= 21).then_some(ScanHint {
        corners: [(x0, y0), (x1, y1), (x2, y2), (x3, y3)],
        modules,
    });
    let had_hint = hint.is_some();
    let mut tracked = false;
    let (payloads, regions, hints) = if let Some(hint) = hint {
        if let Some((payload, region, used)) = decode_from_quad(width, height, luma, hint) {
            tracked = true;
            (vec![payload], vec![region], vec![used])
        } else {
            decode_qr_luma_ex(width, height, luma, discover)
        }
    } else {
        decode_qr_luma_ex(width, height, luma, discover)
    };

    let payload_arr = js_sys::Array::new();
    for payload in payloads {
        let bytes = js_sys::Uint8Array::new_with_length(payload.len() as u32);
        bytes.copy_from(&payload);
        payload_arr.push(&bytes);
    }
    let region_arr = js_sys::Array::new();
    for region in regions {
        let obj = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&obj, &"x".into(), &JsValue::from(region.x));
        let _ = js_sys::Reflect::set(&obj, &"y".into(), &JsValue::from(region.y));
        let _ = js_sys::Reflect::set(&obj, &"w".into(), &JsValue::from(region.w));
        let _ = js_sys::Reflect::set(&obj, &"h".into(), &JsValue::from(region.h));
        region_arr.push(&obj);
    }
    let hint_arr = js_sys::Array::new();
    for hint in hints {
        let obj = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&obj, &"modules".into(), &JsValue::from(hint.modules));
        for (index, (x, y)) in hint.corners.iter().enumerate() {
            let _ = js_sys::Reflect::set(&obj, &format!("x{index}").into(), &JsValue::from(*x));
            let _ = js_sys::Reflect::set(&obj, &format!("y{index}").into(), &JsValue::from(*y));
        }
        hint_arr.push(&obj);
    }
    let out = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&out, &"payloads".into(), &payload_arr);
    let _ = js_sys::Reflect::set(&out, &"regions".into(), &region_arr);
    let _ = js_sys::Reflect::set(&out, &"hints".into(), &hint_arr);
    let _ = js_sys::Reflect::set(&out, &"tracked".into(), &JsValue::from(tracked));
    let _ = js_sys::Reflect::set(&out, &"had_hint".into(), &JsValue::from(had_hint));
    let _ = js_sys::Reflect::set(&out, &"discover".into(), &JsValue::from(discover));
    out
}
