//! Windows DirectShow：补上 Media Foundation 看不到的设备（例如 OBS 虚拟相机）。
//!
//! OBS 虚拟相机只注册 DirectShow，不出现在 Media Foundation 设备列表里。

use std::ffi::c_void;
use std::sync::mpsc::{Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use nokhwa::utils::CameraIndex;
use raysend_core::rgba_to_luma;
use windows::core::{Interface, BSTR, GUID, HRESULT, VARIANT};
use windows::Win32::Foundation::{BOOL, E_FAIL};
use windows::Win32::Media::DirectShow::{
    IBaseFilter, ICaptureGraphBuilder2, ICreateDevEnum, IGraphBuilder, IMediaControl,
};
use windows::Win32::Media::MediaFoundation::{
    AM_MEDIA_TYPE, CLSID_CaptureGraphBuilder2, CLSID_FilterGraph, CLSID_SystemDeviceEnum,
    CLSID_VideoInputDeviceCategory, FORMAT_VideoInfo2, MEDIASUBTYPE_NV12, MEDIASUBTYPE_RGB32,
    MEDIASUBTYPE_YUY2, MEDIATYPE_Video,
    PIN_CATEGORY_CAPTURE, PIN_CATEGORY_PREVIEW,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IBindCtx, IEnumMoniker,
    IErrorLog, IMoniker, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED,
};
use windows::Win32::System::Com::StructuredStorage::IPropertyBag;

use crate::camera::{downscale_rgba, spawn_decode_loop, CamEvent, CameraChoice, RawCam};

const CLSID_SAMPLE_GRABBER: GUID = GUID::from_u128(0xC1F400A0_3F08_11d3_9F0B_006008039E37);
const CLSID_NULL_RENDERER: GUID = GUID::from_u128(0xC1F400A4_3F08_11d3_9F0B_006008039E37);
const IID_SAMPLE_GRABBER: GUID = GUID::from_u128(0x6B652FFF_11FE_4fce_92AD_0266B5D7C78F);
const DSHOW_PREFIX: &str = "dshow:";

/// qedit.dll 里的 ISampleGrabber，windows crate 不提供绑定。
#[repr(transparent)]
#[derive(Clone, PartialEq, Eq, Debug)]
struct ISampleGrabber(windows::core::IUnknown);

#[repr(C)]
struct ISampleGrabber_Vtbl {
    base: windows::core::IUnknown_Vtbl,
    set_one_shot: unsafe extern "system" fn(*mut c_void, BOOL) -> HRESULT,
    set_media_type: unsafe extern "system" fn(*mut c_void, *const AM_MEDIA_TYPE) -> HRESULT,
    get_connected_media_type: unsafe extern "system" fn(*mut c_void, *mut AM_MEDIA_TYPE) -> HRESULT,
    set_buffer_samples: unsafe extern "system" fn(*mut c_void, BOOL) -> HRESULT,
    get_current_buffer: unsafe extern "system" fn(*mut c_void, *mut i32, *mut u8) -> HRESULT,
    get_current_sample: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    set_callback: unsafe extern "system" fn(*mut c_void, *mut c_void, i32) -> HRESULT,
}

unsafe impl Interface for ISampleGrabber {
    type Vtable = ISampleGrabber_Vtbl;
    const IID: GUID = IID_SAMPLE_GRABBER;
}

impl ISampleGrabber {
    unsafe fn set_one_shot(&self, oneshot: bool) -> windows::core::Result<()> {
        (Interface::vtable(self).set_one_shot)(Interface::as_raw(self), BOOL(oneshot as i32)).ok()
    }

    unsafe fn set_media_type(&self, media_type: &AM_MEDIA_TYPE) -> windows::core::Result<()> {
        (Interface::vtable(self).set_media_type)(Interface::as_raw(self), media_type).ok()
    }

    unsafe fn get_connected_media_type(
        &self,
        media_type: &mut AM_MEDIA_TYPE,
    ) -> windows::core::Result<()> {
        (Interface::vtable(self).get_connected_media_type)(Interface::as_raw(self), media_type).ok()
    }

    unsafe fn set_buffer_samples(&self, buffer_them: bool) -> windows::core::Result<()> {
        (Interface::vtable(self).set_buffer_samples)(
            Interface::as_raw(self),
            BOOL(buffer_them as i32),
        )
        .ok()
    }

    unsafe fn get_current_buffer(&self, size: &mut i32, buffer: *mut u8) -> HRESULT {
        (Interface::vtable(self).get_current_buffer)(Interface::as_raw(self), size, buffer)
    }
}

fn init_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

/// 枚举 DirectShow 视频输入设备。OBS 虚拟相机走这条路径。
pub fn list_cameras() -> Vec<CameraChoice> {
    // 可能跑在 GUI 线程上，只确保 COM 已初始化，不要 CoUninitialize。
    init_com();
    match list_cameras_inner() {
        Ok(list) => list,
        Err(err) => {
            eprintln!("RaySend DirectShow 枚举失败：{err}");
            Vec::new()
        }
    }
}

fn list_cameras_inner() -> windows::core::Result<Vec<CameraChoice>> {
    unsafe {
        let enumerator: ICreateDevEnum =
            CoCreateInstance(&CLSID_SystemDeviceEnum, None, CLSCTX_INPROC_SERVER)?;
        let mut monikers: Option<IEnumMoniker> = None;
        enumerator.CreateClassEnumerator(&CLSID_VideoInputDeviceCategory, &mut monikers, 0)?;
        let Some(monikers) = monikers else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        loop {
            let mut slot = [None];
            let mut fetched = 0u32;
            let hr = monikers.Next(&mut slot, Some(&mut fetched as *mut u32));
            if hr.is_err() || fetched == 0 {
                break;
            }
            let Some(moniker) = slot[0].take() else {
                continue;
            };
            if let Ok((name, path)) = read_moniker(&moniker) {
                let id = if path.is_empty() {
                    format!("{DSHOW_PREFIX}name:{name}")
                } else {
                    format!("{DSHOW_PREFIX}{path}")
                };
                let label = if name.trim().is_empty() {
                    "DirectShow Camera".into()
                } else {
                    name
                };
                out.push(CameraChoice {
                    index: CameraIndex::String(id),
                    label,
                });
            }
        }
        Ok(out)
    }
}

unsafe fn read_moniker(moniker: &IMoniker) -> windows::core::Result<(String, String)> {
    let bag: IPropertyBag = moniker.BindToStorage::<_, _, IPropertyBag>(
        None::<&IBindCtx>,
        None::<&IMoniker>,
    )?;
    let name = read_bag_string(&bag, windows::core::w!("FriendlyName")).unwrap_or_default();
    let path = read_bag_string(&bag, windows::core::w!("DevicePath")).unwrap_or_default();
    Ok((name, path))
}

unsafe fn read_bag_string(
    bag: &IPropertyBag,
    key: windows::core::PCWSTR,
) -> windows::core::Result<String> {
    let mut value = VARIANT::default();
    bag.Read(key, &mut value, None::<&IErrorLog>)?;
    variant_to_string(&value)
}

fn variant_to_string(value: &VARIANT) -> windows::core::Result<String> {
    let bstr = BSTR::try_from(value).unwrap_or_default();
    Ok(bstr.to_string())
}

pub fn is_dshow_index(index: &CameraIndex) -> bool {
    matches!(index, CameraIndex::String(s) if s.starts_with(DSHOW_PREFIX))
}

pub fn spawn(index: CameraIndex) -> (Receiver<CamEvent>, SyncSender<()>) {
    let (ui_tx, ui_rx) = std::sync::mpsc::sync_channel::<CamEvent>(1);
    let (raw_tx, raw_rx) = std::sync::mpsc::sync_channel::<RawCam>(1);
    let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel::<()>(1);
    spawn_decode_loop(raw_rx, ui_tx.clone());
    let _ = thread::Builder::new()
        .name("raysend-dshow".into())
        .spawn(move || {
            if let Err(err) = capture_loop(index, raw_tx, ui_tx.clone(), stop_rx) {
                let _ = ui_tx.try_send(CamEvent::Error(format!("DirectShow：{err}")));
            }
        });
    (ui_rx, stop_tx)
}

fn capture_loop(
    index: CameraIndex,
    raw_tx: SyncSender<RawCam>,
    ui_tx: SyncSender<CamEvent>,
    stop: std::sync::mpsc::Receiver<()>,
) -> windows::core::Result<()> {
    // 采集线程用 MTA，避免 STA 下 RenderStream / 无消息循环时一直卡住。
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let result = (|| {
        let CameraIndex::String(id) = index else {
            return Ok(());
        };
        let key = id.strip_prefix(DSHOW_PREFIX).unwrap_or(&id);
        unsafe { capture_loop_inner(key, &raw_tx, &ui_tx, &stop) }
    })();
    unsafe {
        CoUninitialize();
    }
    result
}

unsafe fn capture_loop_inner(
    key: &str,
    raw_tx: &SyncSender<RawCam>,
    ui_tx: &SyncSender<CamEvent>,
    stop: &std::sync::mpsc::Receiver<()>,
) -> windows::core::Result<()> {
    let source = bind_source(key)?;
    let graph: IGraphBuilder = CoCreateInstance(&CLSID_FilterGraph, None, CLSCTX_INPROC_SERVER)?;
    let builder: ICaptureGraphBuilder2 =
        CoCreateInstance(&CLSID_CaptureGraphBuilder2, None, CLSCTX_INPROC_SERVER)?;
    builder.SetFiltergraph(&graph)?;
    graph.AddFilter(&source, windows::core::w!("src"))?;

    let grabber_filter: IBaseFilter =
        match CoCreateInstance(&CLSID_SAMPLE_GRABBER, None, CLSCTX_INPROC_SERVER) {
            Ok(filter) => filter,
            Err(err) => {
                let _ = ui_tx.try_send(CamEvent::Error(format!(
                    "无法创建 DirectShow Sample Grabber（需要 qedit.dll）：{err}"
                )));
                return Ok(());
            }
        };
    let grabber: ISampleGrabber = grabber_filter.cast()?;
    graph.AddFilter(&grabber_filter, windows::core::w!("grab"))?;

    let null_renderer: IBaseFilter =
        CoCreateInstance(&CLSID_NULL_RENDERER, None, CLSCTX_INPROC_SERVER)?;
    graph.AddFilter(&null_renderer, windows::core::w!("null"))?;

    grabber.set_one_shot(false)?;
    grabber.set_buffer_samples(true)?;
    // 不要强行要 RGB24：OBS 虚拟相机通常是 NV12，插入色彩转换滤镜会一直卡在「正在请求相机」。
    let mut mt = AM_MEDIA_TYPE::default();
    mt.majortype = MEDIATYPE_Video;
    grabber.set_media_type(&mt)?;

    let mut rendered = false;
    let mut last_err = None;
    for category in [PIN_CATEGORY_PREVIEW, PIN_CATEGORY_CAPTURE] {
        match builder.RenderStream(
            Some(&category as *const GUID),
            &MEDIATYPE_Video,
            &source,
            &grabber_filter,
            &null_renderer,
        ) {
            Ok(()) => {
                rendered = true;
                break;
            }
            Err(err) => last_err = Some(err),
        }
    }
    if !rendered {
        let err = last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "未知错误".into());
        let _ = ui_tx.try_send(CamEvent::Error(format!(
            "无法连接 DirectShow 相机（请确认 OBS 已点「开始虚拟摄像机」）：{err}"
        )));
        return Ok(());
    }

    let control: IMediaControl = graph.cast()?;
    control.Run()?;
    let _ = control.GetState(2000);

    let mut connected = AM_MEDIA_TYPE::default();
    grabber.get_connected_media_type(&mut connected)?;
    let info = video_info(&connected);
    free_media_type(&mut connected);
    let Some(info) = info else {
        let _ = ui_tx.try_send(CamEvent::Error("DirectShow 未给出画面尺寸".into()));
        let _ = control.Stop();
        return Ok(());
    };

    let need = buffer_len(&info);
    let mut buf = vec![0u8; need.max(1)];
    let started = Instant::now();
    let mut got_frame = false;
    loop {
        if stop.try_recv().is_ok() {
            break;
        }
        let mut size = buf.len() as i32;
        if grabber
            .get_current_buffer(&mut size, buf.as_mut_ptr())
            .is_err()
            || size <= 0
        {
            if !got_frame && started.elapsed() > Duration::from_secs(5) {
                let _ = ui_tx.try_send(CamEvent::Error(
                    "DirectShow 已连接但没有画面，请确认 OBS 虚拟摄像机正在输出".into(),
                ));
                break;
            }
            thread::sleep(Duration::from_millis(15));
            continue;
        }
        let used = (size as usize).min(buf.len());
        let rgba = frame_to_rgba(&buf[..used], &info);
        if rgba.is_empty() {
            thread::sleep(Duration::from_millis(15));
            continue;
        }
        got_frame = true;
        let luma = rgba_to_luma(info.width, info.height, &rgba);
        let (preview_w, preview_h, preview_rgba) =
            downscale_rgba(info.width, info.height, &rgba, 480);
        let _ = raw_tx.try_send(RawCam {
            width: info.width,
            height: info.height,
            luma,
            preview_w,
            preview_h,
            preview_rgba,
        });
    }
    let _ = control.Stop();
    Ok(())
}

unsafe fn bind_source(key: &str) -> windows::core::Result<IBaseFilter> {
    let enumerator: ICreateDevEnum =
        CoCreateInstance(&CLSID_SystemDeviceEnum, None, CLSCTX_INPROC_SERVER)?;
    let mut monikers: Option<IEnumMoniker> = None;
    enumerator.CreateClassEnumerator(&CLSID_VideoInputDeviceCategory, &mut monikers, 0)?;
    let Some(monikers) = monikers else {
        return Err(windows::core::Error::from(E_FAIL));
    };
    loop {
        let mut slot = [None];
        let mut fetched = 0u32;
        let hr = monikers.Next(&mut slot, Some(&mut fetched as *mut u32));
        if hr.is_err() || fetched == 0 {
            break;
        }
        let Some(moniker) = slot[0].take() else {
            continue;
        };
        let (name, path) = read_moniker(&moniker)?;
        let matched = if let Some(rest) = key.strip_prefix("name:") {
            rest == name
        } else {
            key == path || key == name
        };
        if matched {
            return moniker.BindToObject::<_, _, IBaseFilter>(None::<&IBindCtx>, None::<&IMoniker>);
        }
    }
    Err(windows::core::Error::from(E_FAIL))
}

#[derive(Clone, Copy)]
struct VideoInfo {
    width: u32,
    height: u32,
    kind: PixelKind,
}

#[derive(Clone, Copy)]
enum PixelKind {
    Bgr24,
    Bgr32,
    Yuy2,
    Nv12,
}

fn buffer_len(info: &VideoInfo) -> usize {
    match info.kind {
        PixelKind::Bgr24 => {
            let stride = ((info.width as usize * 3 + 3) / 4) * 4;
            stride * info.height as usize
        }
        PixelKind::Bgr32 => info.width as usize * info.height as usize * 4,
        PixelKind::Yuy2 => info.width as usize * info.height as usize * 2,
        PixelKind::Nv12 => info.width as usize * info.height as usize * 3 / 2,
    }
}

unsafe fn video_info(mt: &AM_MEDIA_TYPE) -> Option<VideoInfo> {
    let bmih_off = if mt.formattype == FORMAT_VideoInfo2 {
        72usize
    } else {
        48usize
    };
    if (mt.cbFormat as usize) < bmih_off + 16 || mt.pbFormat.is_null() {
        return None;
    }
    let header = std::slice::from_raw_parts(mt.pbFormat, mt.cbFormat as usize);
    let width = i32::from_le_bytes(header[bmih_off + 4..bmih_off + 8].try_into().ok()?);
    let height = i32::from_le_bytes(header[bmih_off + 8..bmih_off + 12].try_into().ok()?);
    let bit_count =
        u16::from_le_bytes(header[bmih_off + 14..bmih_off + 16].try_into().ok()?) as u32;
    let width = u32::try_from(width).ok()?;
    let height = height.unsigned_abs();
    if width == 0 || height == 0 {
        return None;
    }
    let kind = if mt.subtype == MEDIASUBTYPE_NV12 {
        PixelKind::Nv12
    } else if mt.subtype == MEDIASUBTYPE_YUY2 {
        PixelKind::Yuy2
    } else if mt.subtype == MEDIASUBTYPE_RGB32 || bit_count == 32 {
        PixelKind::Bgr32
    } else {
        PixelKind::Bgr24
    };
    Some(VideoInfo {
        width,
        height,
        kind,
    })
}

unsafe fn free_media_type(mt: &mut AM_MEDIA_TYPE) {
    if !mt.pbFormat.is_null() && mt.cbFormat > 0 {
        CoTaskMemFree(Some(mt.pbFormat as *const c_void));
        mt.pbFormat = std::ptr::null_mut();
    }
}

fn frame_to_rgba(buf: &[u8], info: &VideoInfo) -> Vec<u8> {
    match info.kind {
        PixelKind::Bgr24 => packed_bgr_to_rgba(buf, info.width, info.height, 3),
        PixelKind::Bgr32 => packed_bgr_to_rgba(buf, info.width, info.height, 4),
        PixelKind::Yuy2 => yuy2_to_rgba(buf, info.width, info.height),
        PixelKind::Nv12 => nv12_to_rgba(buf, info.width, info.height),
    }
}

fn packed_bgr_to_rgba(buf: &[u8], width: u32, height: u32, pixel: usize) -> Vec<u8> {
    let stride = ((width as usize * pixel + 3) / 4) * 4;
    let mut out = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        let src_y = height as usize - 1 - y;
        let row = src_y * stride;
        for x in 0..width as usize {
            let i = row + x * pixel;
            if i + 2 >= buf.len() {
                continue;
            }
            let o = (y * width as usize + x) * 4;
            out[o] = buf[i + 2];
            out[o + 1] = buf[i + 1];
            out[o + 2] = buf[i];
            out[o + 3] = 255;
        }
    }
    out
}

fn yuy2_to_rgba(buf: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in (0..w).step_by(2) {
            let i = (y * w + x) * 2;
            if i + 3 >= buf.len() {
                continue;
            }
            let y0 = buf[i] as i32;
            let u = buf[i + 1] as i32 - 128;
            let y1 = buf[i + 2] as i32;
            let v = buf[i + 3] as i32 - 128;
            put_yuv(&mut out, w, x, y, y0, u, v);
            if x + 1 < w {
                put_yuv(&mut out, w, x + 1, y, y1, u, v);
            }
        }
    }
    out
}

fn nv12_to_rgba(buf: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    if buf.len() < y_size + y_size / 2 {
        return Vec::new();
    }
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let yv = buf[y * w + x] as i32;
            let uv = y_size + (y / 2) * w + (x & !1);
            let u = buf[uv] as i32 - 128;
            let v = buf[uv + 1] as i32 - 128;
            put_yuv(&mut out, w, x, y, yv, u, v);
        }
    }
    out
}

fn put_yuv(out: &mut [u8], width: usize, x: usize, y: usize, luma: i32, u: i32, v: i32) {
    let r = (luma + (359 * v) / 256).clamp(0, 255) as u8;
    let g = (luma - (88 * u + 183 * v) / 256).clamp(0, 255) as u8;
    let b = (luma + (454 * u) / 256).clamp(0, 255) as u8;
    let o = (y * width + x) * 4;
    out[o] = r;
    out[o + 1] = g;
    out[o + 2] = b;
    out[o + 3] = 255;
}
