//! 网页解码池：Worker 加载专用小 WASM（`public/qr-decode`），失败则退回主线程。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use raysend_core::{decode_from_quad, rgba_to_luma, DecodedWindow, PlannedCrop, ScanHint, ScanRegion};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::{MessageEvent, Worker, WorkerOptions, WorkerType};

pub struct DecodeJob {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub luma: Vec<u8>,
    pub discover: bool,
    pub hint: Option<ScanHint>,
}

enum PoolState {
    Idle,
    Booting,
    Ready,
    Dead,
}

struct Inflight {
    worker: usize,
    done: Box<dyn FnOnce(DecodedWindow)>,
}

struct PoolInner {
    state: PoolState,
    workers: Vec<Worker>,
    busy: Vec<bool>,
    frame_seq: u32,
    pending: HashMap<u32, Inflight>,
    on_message: Option<Closure<dyn FnMut(MessageEvent)>>,
}

pub struct DecodePool {
    inner: RefCell<PoolInner>,
}

thread_local! {
    static GLOBAL: Rc<DecodePool> = Rc::new(DecodePool::new());
}

impl DecodePool {
    fn new() -> Self {
        Self {
            inner: RefCell::new(PoolInner {
                state: PoolState::Idle,
                workers: Vec::new(),
                busy: Vec::new(),
                frame_seq: 0,
                pending: HashMap::new(),
                on_message: None,
            }),
        }
    }

    pub fn global() -> Rc<Self> {
        GLOBAL.with(Rc::clone)
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.inner.borrow().state, PoolState::Ready)
    }

    pub fn worker_len(&self) -> usize {
        if matches!(self.inner.borrow().state, PoolState::Ready) {
            self.inner.borrow().workers.len()
        } else {
            0
        }
    }

    pub fn ensure(&self) {
        let mut inner = self.inner.borrow_mut();
        if !matches!(inner.state, PoolState::Idle) {
            return;
        }
        let Some((js_url, wasm_url)) = dedicated_glue_urls().or_else(discover_glue_urls) else {
            inner.state = PoolState::Dead;
            return;
        };
        let count = worker_count();
        let opts = WorkerOptions::new();
        opts.set_type(WorkerType::Module);
        let worker_url = worker_script_url();
        let mut workers = Vec::new();
        for _ in 0..count {
            let Ok(worker) = Worker::new_with_options(&worker_url, &opts) else {
                inner.state = PoolState::Dead;
                return;
            };
            let boot = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&boot, &"op".into(), &"boot".into());
            let _ = js_sys::Reflect::set(&boot, &"js".into(), &JsValue::from_str(&js_url));
            let wasm_list = js_sys::Array::new();
            wasm_list.push(&JsValue::from_str(&wasm_url));
            if wasm_url.ends_with("_bg.wasm") {
                wasm_list.push(&JsValue::from_str(&wasm_url.replace("_bg.wasm", ".wasm")));
            } else if wasm_url.ends_with(".wasm") {
                let alt = format!("{}_bg.wasm", wasm_url.trim_end_matches(".wasm"));
                wasm_list.push(&JsValue::from_str(&alt));
            }
            let _ = js_sys::Reflect::set(&boot, &"wasm".into(), &wasm_list);
            if worker.post_message(&boot).is_err() {
                inner.state = PoolState::Dead;
                return;
            }
            workers.push(worker);
        }
        if workers.is_empty() {
            inner.state = PoolState::Dead;
            return;
        }

        let pool = DecodePool::global();
        let ready = Rc::new(Cell::new(0u32));
        let want = workers.len() as u32;
        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
            pool.handle_message(event, &ready, want);
        }) as Box<dyn FnMut(MessageEvent)>);
        for worker in &workers {
            worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        }
        inner.workers = workers;
        inner.on_message = Some(on_message);
        inner.state = PoolState::Booting;
    }

    pub fn free_count(&self) -> usize {
        let inner = self.inner.borrow();
        if !matches!(inner.state, PoolState::Ready) {
            return 0;
        }
        inner.busy.iter().filter(|busy| !**busy).count()
    }

    /// 投给一个空闲 worker。池满返回 false。回调按窗回来，不堵整批。
    pub fn submit_one(&self, job: DecodeJob, done: Box<dyn FnOnce(DecodedWindow)>) -> bool {
        let mut inner = self.inner.borrow_mut();
        if !matches!(inner.state, PoolState::Ready) || inner.workers.is_empty() {
            drop(inner);
            done(decode_job(&job));
            return true;
        }
        let Some(idx) = inner.busy.iter().position(|busy| !*busy) else {
            return false;
        };
        inner.busy[idx] = true;
        let job_id = inner.frame_seq.wrapping_add(1);
        inner.frame_seq = job_id;
        let worker = inner.workers[idx].clone();
        inner.pending.insert(
            job_id,
            Inflight {
                worker: idx,
                done,
            },
        );
        drop(inner);
        if post_job(&worker, job_id, &job).is_err() {
            let cb = {
                let mut inner = self.inner.borrow_mut();
                if let Some(slot) = inner.busy.get_mut(idx) {
                    *slot = false;
                }
                inner.pending.remove(&job_id).map(|job| job.done)
            };
            if let Some(cb) = cb {
                cb(decode_job(&job));
            }
        }
        true
    }

    fn handle_message(&self, event: MessageEvent, ready: &Rc<Cell<u32>>, want: u32) {
        let data = event.data();
        let op = js_sys::Reflect::get(&data, &"op".into())
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        match op.as_str() {
            "ready" => {
                let n = ready.get() + 1;
                ready.set(n);
                if n >= want {
                    let mut inner = self.inner.borrow_mut();
                    inner.state = PoolState::Ready;
                    inner.busy = vec![false; inner.workers.len()];
                }
            }
            "fail" => {
                self.inner.borrow_mut().state = PoolState::Dead;
            }
            "done" => self.finish_job(&data),
            _ => {}
        }
    }

    fn finish_job(&self, data: &JsValue) {
        let job_id = js_sys::Reflect::get(data, &"id".into())
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as u32;
        let window = parsed_window(data);
        let mut inner = self.inner.borrow_mut();
        let Some(inflight) = inner.pending.remove(&job_id) else {
            return;
        };
        if let Some(slot) = inner.busy.get_mut(inflight.worker) {
            *slot = false;
        }
        drop(inner);
        (inflight.done)(window);
    }
}

fn worker_count() -> usize {
    let cores = web_sys::window()
        .and_then(|w| {
            js_sys::Reflect::get(&w.navigator(), &"hardwareConcurrency".into())
                .ok()
                .and_then(|v| v.as_f64())
        })
        .unwrap_or(4.0) as usize;
    // 留一核给相机 / UI；小 WASM 才按宫格顶到 4，不复制整份 Dioxus。
    cores.saturating_sub(1).clamp(1, 4)
}

fn dedicated_glue_urls() -> Option<(String, String)> {
    let js = abs_url("./qr-decode/qr-decode.js");
    if js.is_empty() {
        return None;
    }
    Some((js, abs_url("./qr-decode/qr-decode_bg.wasm")))
}

fn worker_script_url() -> String {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.base_uri().ok().flatten())
        .and_then(|base| web_sys::Url::new_with_base("qr-decode-worker.js?v=3", &base).ok())
        .map(|u| u.href())
        .unwrap_or_else(|| "./qr-decode-worker.js?v=3".into())
}

fn discover_glue_urls() -> Option<(String, String)> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let mut js = None;
    let mut wasm = None;

    if let Ok(scripts) = document.query_selector_all("script[src]") {
        for i in 0..scripts.length() {
            let Some(el) = scripts
                .get(i)
                .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
            else {
                continue;
            };
            let Some(src) = el.get_attribute("src") else {
                continue;
            };
            if looks_like_app_js(&src) {
                js = Some(abs_url(&src));
            }
        }
    }
    if let Ok(links) = document.query_selector_all("link[href]") {
        for i in 0..links.length() {
            let Some(el) = links
                .get(i)
                .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
            else {
                continue;
            };
            let Some(href) = el.get_attribute("href") else {
                continue;
            };
            if href.ends_with(".wasm") {
                wasm = Some(abs_url(&href));
            } else if looks_like_app_js(&href) {
                js = js.or_else(|| Some(abs_url(&href)));
            }
        }
    }
    if let Some(perf) = window.performance() {
        let entries = perf.get_entries_by_type("resource");
        for i in 0..entries.length() {
            let name = js_sys::Reflect::get(&entries.get(i), &"name".into())
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default();
            if name.ends_with(".wasm") {
                wasm = Some(name);
            } else if looks_like_app_js(&name) {
                js = js.or_else(|| Some(name));
            }
        }
    }
    if wasm.is_none() {
        if let Some(html) = document.document_element().map(|el| el.outer_html()) {
            wasm = find_url_in_text(&html, ".wasm");
            if js.is_none() {
                js = find_url_in_text(&html, ".js").filter(|u| looks_like_app_js(u));
            }
        }
    }
    let js = js?;
    let wasm = wasm.unwrap_or_else(|| guess_wasm_url(&js));
    Some((js, wasm))
}

fn looks_like_app_js(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.ends_with(".js")
        && (lower.contains("dioxus") || lower.contains("raysend") || lower.contains("/wasm/"))
}

fn guess_wasm_url(js: &str) -> String {
    if let Some(base) = js.strip_suffix(".js") {
        format!("{base}_bg.wasm")
    } else {
        format!("{js}.wasm")
    }
}

fn find_url_in_text(html: &str, suffix: &str) -> Option<String> {
    let mut found = None;
    let mut rest = html;
    while let Some(idx) = rest.find(suffix) {
        let start = rest[..idx]
            .rfind(|c: char| c == '"' || c == '\'' || c == '(' || c == '`')
            .map(|i| i + 1)
            .unwrap_or(0);
        let raw = &rest[start..idx + suffix.len()];
        if raw.contains("://") || raw.starts_with('.') || raw.starts_with('/') {
            found = Some(abs_url(raw));
            if suffix == ".wasm" || looks_like_app_js(raw) {
                break;
            }
        }
        rest = &rest[idx + suffix.len()..];
    }
    found
}

fn abs_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("blob:") {
        return url.to_string();
    }
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.base_uri().ok().flatten())
        .and_then(|base| web_sys::Url::new_with_base(url, &base).ok())
        .map(|u| u.href())
        .unwrap_or_else(|| url.to_string())
}

fn post_job(worker: &Worker, frame_id: u32, job: &DecodeJob) -> Result<(), JsValue> {
    let msg = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&msg, &"op".into(), &"decode".into());
    let _ = js_sys::Reflect::set(&msg, &"id".into(), &JsValue::from(frame_id));
    let _ = js_sys::Reflect::set(&msg, &"x".into(), &JsValue::from(job.x));
    let _ = js_sys::Reflect::set(&msg, &"y".into(), &JsValue::from(job.y));
    let _ = js_sys::Reflect::set(&msg, &"width".into(), &JsValue::from(job.width));
    let _ = js_sys::Reflect::set(&msg, &"height".into(), &JsValue::from(job.height));
    let _ = js_sys::Reflect::set(&msg, &"discover".into(), &JsValue::from(job.discover));
    if let Some(hint) = job.hint {
        let _ = js_sys::Reflect::set(&msg, &"modules".into(), &JsValue::from(hint.modules));
        for (index, (x, y)) in hint.corners.iter().enumerate() {
            let _ = js_sys::Reflect::set(&msg, &format!("x{index}").into(), &JsValue::from(*x));
            let _ = js_sys::Reflect::set(&msg, &format!("y{index}").into(), &JsValue::from(*y));
        }
    }
    let bytes = js_sys::Uint8Array::new_with_length(job.luma.len() as u32);
    bytes.copy_from(&job.luma);
    let buffer = bytes.buffer();
    let _ = js_sys::Reflect::set(&msg, &"luma".into(), &buffer);
    let transfer = js_sys::Array::new();
    transfer.push(&buffer);
    worker.post_message_with_transfer(&msg, &transfer)
}

fn parsed_window(data: &JsValue) -> DecodedWindow {
    let x = num_field(data, "x");
    let y = num_field(data, "y");
    if js_sys::Reflect::get(data, &"err".into())
        .ok()
        .and_then(|v| v.as_string())
        .is_some()
    {
        return DecodedWindow {
            x,
            y,
            payloads: Vec::new(),
            regions: Vec::new(),
            hints: Vec::new(),
            had_hint: bool_field(data, "had_hint"),
            tracked: false,
            discover: bool_field(data, "discover"),
        };
    }
    let mut payloads = Vec::new();
    if let Ok(arr) = js_sys::Reflect::get(data, &"payloads".into()) {
        let arr = js_sys::Array::from(&arr);
        for i in 0..arr.length() {
            let bytes = js_sys::Uint8Array::new(&arr.get(i));
            let mut buf = vec![0u8; bytes.length() as usize];
            bytes.copy_to(&mut buf);
            if !buf.is_empty() {
                payloads.push(buf);
            }
        }
    }
    let mut regions = Vec::new();
    if let Ok(arr) = js_sys::Reflect::get(data, &"regions".into()) {
        let arr = js_sys::Array::from(&arr);
        for i in 0..arr.length() {
            let item = arr.get(i);
            let w = num_field(&item, "w");
            let h = num_field(&item, "h");
            if w > 0 && h > 0 {
                regions.push(ScanRegion {
                    x: num_field(&item, "x"),
                    y: num_field(&item, "y"),
                    w,
                    h,
                });
            }
        }
    }
    DecodedWindow {
        x,
        y,
        payloads,
        regions,
        hints: parse_hints(data),
        had_hint: bool_field(data, "had_hint"),
        tracked: bool_field(data, "tracked"),
        discover: bool_field(data, "discover"),
    }
}

fn parse_hints(data: &JsValue) -> Vec<ScanHint> {
    let Ok(arr) = js_sys::Reflect::get(data, &"hints".into()) else {
        return Vec::new();
    };
    let arr = js_sys::Array::from(&arr);
    let mut hints = Vec::new();
    for i in 0..arr.length() {
        let item = arr.get(i);
        let modules = num_field(&item, "modules") as u16;
        if modules < 21 {
            continue;
        }
        hints.push(ScanHint {
            corners: [
                (signed_field(&item, "x0"), signed_field(&item, "y0")),
                (signed_field(&item, "x1"), signed_field(&item, "y1")),
                (signed_field(&item, "x2"), signed_field(&item, "y2")),
                (signed_field(&item, "x3"), signed_field(&item, "y3")),
            ],
            modules,
        });
    }
    hints
}

fn signed_field(obj: &JsValue, key: &str) -> i32 {
    js_sys::Reflect::get(obj, &key.into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as i32
}

fn bool_field(obj: &JsValue, key: &str) -> bool {
    js_sys::Reflect::get(obj, &key.into())
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn num_field(obj: &JsValue, key: &str) -> u32 {
    js_sys::Reflect::get(obj, &key.into())
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as u32
}

pub fn extract_jobs(
    ctx: &web_sys::CanvasRenderingContext2d,
    planned: Option<Vec<PlannedCrop>>,
    vw: u32,
    vh: u32,
    rotate: usize,
    limit: usize,
) -> Vec<DecodeJob> {
    if limit == 0 {
        return Vec::new();
    }
    if let Some(crops) = planned {
        if crops.is_empty() {
            return Vec::new();
        }
        let mut jobs = Vec::new();
        let n = crops.len();
        for i in 0..n {
            if jobs.len() >= limit {
                break;
            }
            let crop = crops[(i + rotate) % n];
            let Ok(image) = ctx.get_image_data(
                crop.region.x as f64,
                crop.region.y as f64,
                crop.region.w as f64,
                crop.region.h as f64,
            ) else {
                continue;
            };
            jobs.push(DecodeJob {
                x: crop.region.x,
                y: crop.region.y,
                width: crop.region.w,
                height: crop.region.h,
                luma: rgba_to_luma(crop.region.w, crop.region.h, &image.data().0),
                discover: false,
                hint: crop
                    .hint
                    .map(|hint| hint.offset(-(crop.region.x as i32), -(crop.region.y as i32))),
            });
        }
        return jobs;
    }
    if let Ok(image) = ctx.get_image_data(0.0, 0.0, vw as f64, vh as f64) {
        return vec![DecodeJob {
            x: 0,
            y: 0,
            width: vw,
            height: vh,
            luma: rgba_to_luma(vw, vh, &image.data().0),
            discover: true,
            hint: None,
        }];
    }
    Vec::new()
}

pub(super) fn decode_job(job: &DecodeJob) -> DecodedWindow {
    if let Some(hint) = job.hint {
        if let Some((payload, region, used)) =
            decode_from_quad(job.width, job.height, &job.luma, hint)
        {
            return DecodedWindow {
                x: job.x,
                y: job.y,
                payloads: vec![payload],
                regions: vec![region],
                hints: vec![used],
                had_hint: true,
                tracked: true,
                discover: job.discover,
            };
        }
    }
    let (payloads, regions, hints) =
        raysend_core::decode_qr_luma_ex(job.width, job.height, &job.luma, job.discover);
    DecodedWindow {
        x: job.x,
        y: job.y,
        payloads,
        regions,
        hints,
        had_hint: job.hint.is_some(),
        tracked: false,
        discover: job.discover,
    }
}
