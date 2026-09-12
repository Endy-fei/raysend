//! `raysend-core` 的 C ABI，见 `include/raysend.h`。
//!
//! - iOS：打 `staticlib`，目标 `aarch64-apple-ios` / `aarch64-apple-ios-sim`
//! - Android：打 `cdylib`，目标 `aarch64-linux-android`（JNI）
//! - 鸿蒙：打 `cdylib`，目标 `aarch64-unknown-linux-ohos`（NAPI）
//!
//! 句柄非线程安全，同一句柄上的调用必须串行。

#![allow(non_camel_case_types)] // 不透明类型名必须与 raysend.h 一致

use std::os::raw::c_int;
use std::slice;

use raysend_core::Decoder;
use raysend_core::session::{Outgoing, PrepareError};
use raysend_core::{compress, decompress, render_qr_rgba, Density, IngestResult};

/// 成功。
pub const RAYSEND_OK: c_int = 0;
/// 空指针。
pub const RAYSEND_ERR_NULL: c_int = -1;
/// 参数不合法（预留）。
#[allow(dead_code)]
pub const RAYSEND_ERR_ARG: c_int = -2;
/// 输出缓冲不够；`out_len` 会写成所需长度。
pub const RAYSEND_ERR_BUF: c_int = -3;
/// 空文件。
pub const RAYSEND_ERR_EMPTY: c_int = -4;
/// 超过 64 MB 上限。
pub const RAYSEND_ERR_TOO_LARGE: c_int = -5;
/// 内部错误（编码失败、尚未完成等）。
pub const RAYSEND_ERR_INTERNAL: c_int = -6;

/// 忽略（垃圾帧）。
pub const RAYSEND_INGEST_IGNORED: c_int = 0;
/// 首次锁定 session / OTI。
pub const RAYSEND_INGEST_META: c_int = 1;
/// 重复符号。
pub const RAYSEND_INGEST_DUP: c_int = 2;
/// 新符号。
pub const RAYSEND_INGEST_ACCEPTED: c_int = 3;
/// 还原完成。
pub const RAYSEND_INGEST_COMPLETE: c_int = 4;
/// 校验失败。
pub const RAYSEND_INGEST_FAILED: c_int = 5;
/// 旧 QT 协议。
pub const RAYSEND_INGEST_LEGACY: c_int = 6;

/// 发送句柄，对应 [`Outgoing`]。
pub struct raysend_sender {
    inner: Outgoing,
}

/// 接收句柄，对应 [`Decoder`]。
pub struct raysend_receiver {
    inner: Decoder,
}

/// 把 `Vec<u8>` 交给 C 侧，调用方必须用 [`raysend_free`] 释放。
fn copy_vec(bytes: Vec<u8>, out_ptr: *mut *mut u8, out_len: *mut usize) -> c_int {
    if out_ptr.is_null() || out_len.is_null() {
        return RAYSEND_ERR_NULL;
    }
    let len = bytes.len();
    let mut boxed = bytes.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    unsafe {
        *out_ptr = ptr;
        *out_len = len;
    }
    RAYSEND_OK
}

/// 空指针仅在 `len == 0` 时视为空切片。
fn slice_from<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() {
        if len == 0 {
            Some(&[])
        } else {
            None
        }
    } else {
        Some(unsafe { slice::from_raw_parts(ptr, len) })
    }
}

/// 释放 [`copy_vec`] / 压缩 / 二维码 分配的缓冲。
#[no_mangle]
pub unsafe extern "C" fn raysend_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    drop(Vec::from_raw_parts(ptr, len, len));
}

/// Brotli 压缩。`out_ptr` 由本库分配。
#[no_mangle]
pub unsafe extern "C" fn raysend_compress(
    input: *const u8,
    input_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> c_int {
    let Some(bytes) = slice_from(input, input_len) else {
        return RAYSEND_ERR_NULL;
    };
    copy_vec(compress(bytes.to_vec()), out_ptr, out_len)
}

/// Brotli 解压。
#[no_mangle]
pub unsafe extern "C" fn raysend_decompress(
    input: *const u8,
    input_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> c_int {
    let Some(bytes) = slice_from(input, input_len) else {
        return RAYSEND_ERR_NULL;
    };
    copy_vec(decompress(bytes), out_ptr, out_len)
}

/// 从原始文件字节创建发送会话（内部会压缩）。
#[no_mangle]
pub unsafe extern "C" fn raysend_sender_new(
    name_utf8: *const u8,
    name_len: usize,
    file_bytes: *const u8,
    file_len: usize,
    out: *mut *mut raysend_sender,
) -> c_int {
    raysend_sender_new_ex(name_utf8, name_len, file_bytes, file_len, 1, out)
}

/// 按密度档创建发送会话。
#[no_mangle]
pub unsafe extern "C" fn raysend_sender_new_ex(
    name_utf8: *const u8,
    name_len: usize,
    file_bytes: *const u8,
    file_len: usize,
    density: u8,
    out: *mut *mut raysend_sender,
) -> c_int {
    if out.is_null() {
        return RAYSEND_ERR_NULL;
    }
    let Some(name_bytes) = slice_from(name_utf8, name_len) else {
        return RAYSEND_ERR_NULL;
    };
    let Some(file) = slice_from(file_bytes, file_len) else {
        return RAYSEND_ERR_NULL;
    };
    let name = String::from_utf8_lossy(name_bytes).into_owned();
    match Outgoing::prepare_with(name, file.to_vec(), Density::from_id(density)) {
        Ok(inner) => {
            *out = Box::into_raw(Box::new(raysend_sender { inner }));
            RAYSEND_OK
        }
        Err(PrepareError::Empty) => RAYSEND_ERR_EMPTY,
        Err(PrepareError::TooLarge(_)) => RAYSEND_ERR_TOO_LARGE,
    }
}

/// 释放发送句柄。
#[no_mangle]
pub unsafe extern "C" fn raysend_sender_free(sender: *mut raysend_sender) {
    if !sender.is_null() {
        drop(Box::from_raw(sender));
    }
}

/// 写出下一帧协议载荷。缓冲不够时返回 [`RAYSEND_ERR_BUF`]，并在 `out_len` 给出所需长度。
#[no_mangle]
pub unsafe extern "C" fn raysend_sender_next(
    sender: *mut raysend_sender,
    out_buf: *mut u8,
    out_cap: usize,
    out_len: *mut usize,
) -> c_int {
    if sender.is_null() || out_len.is_null() {
        return RAYSEND_ERR_NULL;
    }
    if out_buf.is_null() && out_cap > 0 {
        return RAYSEND_ERR_NULL;
    }
    let payloads = (*sender).inner.next_payloads(1);
    let payload = payloads.into_iter().next().unwrap_or_default();
    if payload.len() > out_cap {
        *out_len = payload.len();
        return RAYSEND_ERR_BUF;
    }
    if !payload.is_empty() {
        std::ptr::copy_nonoverlapping(payload.as_ptr(), out_buf, payload.len());
    }
    *out_len = payload.len();
    RAYSEND_OK
}

/// 读取原始/压缩体积。指针可为 `NULL` 表示不关心该项。
#[no_mangle]
pub unsafe extern "C" fn raysend_sender_info(
    sender: *const raysend_sender,
    orig_size: *mut u64,
    compressed_size: *mut u64,
) -> c_int {
    if sender.is_null() {
        return RAYSEND_ERR_NULL;
    }
    if !orig_size.is_null() {
        *orig_size = (*sender).inner.orig_size;
    }
    if !compressed_size.is_null() {
        *compressed_size = (*sender).inner.compressed_size;
    }
    RAYSEND_OK
}

/// 将一帧协议载荷画成 RGBA。释放时 `len = (*out_px) * (*out_px) * 4`。
#[no_mangle]
pub unsafe extern "C" fn raysend_qr_rgba(
    payload: *const u8,
    payload_len: usize,
    out_size: u32,
    out_ptr: *mut *mut u8,
    out_px: *mut u32,
) -> c_int {
    let Some(bytes) = slice_from(payload, payload_len) else {
        return RAYSEND_ERR_NULL;
    };
    if out_ptr.is_null() || out_px.is_null() {
        return RAYSEND_ERR_NULL;
    }
    let Some((px, rgba)) = render_qr_rgba(bytes, out_size) else {
        return RAYSEND_ERR_INTERNAL;
    };
    *out_px = px;
    let mut dummy = 0usize;
    copy_vec(rgba, out_ptr, &mut dummy)
}

/// 新建接收会话。
#[no_mangle]
pub unsafe extern "C" fn raysend_receiver_new() -> *mut raysend_receiver {
    Box::into_raw(Box::new(raysend_receiver {
        inner: Decoder::new(),
    }))
}

/// 释放接收句柄。
#[no_mangle]
pub unsafe extern "C" fn raysend_receiver_free(receiver: *mut raysend_receiver) {
    if !receiver.is_null() {
        drop(Box::from_raw(receiver));
    }
}

/// 把 [`Decoder::ingest`] 映射为 C 侧摄入码。
fn ingest_code(dec: &mut Decoder, frame: &[u8]) -> c_int {
    match dec.ingest(frame) {
        IngestResult::Ignored => RAYSEND_INGEST_IGNORED,
        IngestResult::Legacy => RAYSEND_INGEST_LEGACY,
        IngestResult::Meta => RAYSEND_INGEST_META,
        IngestResult::Duplicate => RAYSEND_INGEST_DUP,
        IngestResult::Accepted { .. } => RAYSEND_INGEST_ACCEPTED,
        IngestResult::Complete { .. } => RAYSEND_INGEST_COMPLETE,
        IngestResult::Failed(_) => RAYSEND_INGEST_FAILED,
    }
}

/// 喂一帧已识别的二维码载荷。
#[no_mangle]
pub unsafe extern "C" fn raysend_receiver_ingest(
    receiver: *mut raysend_receiver,
    frame: *const u8,
    frame_len: usize,
) -> c_int {
    if receiver.is_null() {
        return RAYSEND_ERR_NULL;
    }
    let Some(bytes) = slice_from(frame, frame_len) else {
        return RAYSEND_ERR_NULL;
    };
    ingest_code(&mut (*receiver).inner, bytes)
}

/// 扫描 8 位灰度图，返回本帧新接受的符号数。
#[no_mangle]
pub unsafe extern "C" fn raysend_receiver_scan_luma(
    receiver: *mut raysend_receiver,
    width: u32,
    height: u32,
    luma: *const u8,
    luma_len: usize,
) -> c_int {
    if receiver.is_null() {
        return RAYSEND_ERR_NULL;
    }
    let Some(bytes) = slice_from(luma, luma_len) else {
        return RAYSEND_ERR_NULL;
    };
    (*receiver).inner.scan_luma(width, height, bytes) as c_int
}

/// 扫描 RGBA（每像素 4 字节）。
#[no_mangle]
pub unsafe extern "C" fn raysend_receiver_scan_rgba(
    receiver: *mut raysend_receiver,
    width: u32,
    height: u32,
    rgba: *const u8,
    rgba_len: usize,
) -> c_int {
    if receiver.is_null() {
        return RAYSEND_ERR_NULL;
    }
    let Some(bytes) = slice_from(rgba, rgba_len) else {
        return RAYSEND_ERR_NULL;
    };
    (*receiver).inner.scan_rgba(width, height, bytes) as c_int
}

/// 读取互异符号进度。
#[no_mangle]
pub unsafe extern "C" fn raysend_receiver_progress(
    receiver: *const raysend_receiver,
    unique: *mut u32,
    needed: *mut u32,
) -> c_int {
    if receiver.is_null() {
        return RAYSEND_ERR_NULL;
    }
    if !unique.is_null() {
        *unique = (*receiver).inner.unique_count() as u32;
    }
    if !needed.is_null() {
        *needed = (*receiver).inner.needed() as u32;
    }
    RAYSEND_OK
}

/// 完成后取出文件名与解压数据，并重置接收器。缓冲由调用方 `raysend_free`。
#[no_mangle]
pub unsafe extern "C" fn raysend_receiver_take(
    receiver: *mut raysend_receiver,
    name_utf8: *mut *mut u8,
    name_len: *mut usize,
    file_bytes: *mut *mut u8,
    file_len: *mut usize,
) -> c_int {
    if receiver.is_null()
        || name_utf8.is_null()
        || name_len.is_null()
        || file_bytes.is_null()
        || file_len.is_null()
    {
        return RAYSEND_ERR_NULL;
    }
    if !(*receiver).inner.is_finished() {
        return RAYSEND_ERR_INTERNAL;
    }
    let decoder = std::ptr::replace(&mut (*receiver).inner, Decoder::new());
    let Some(finished) = decoder.take_finished() else {
        return RAYSEND_ERR_INTERNAL;
    };
    let name = finished.get_name();
    let data = match finished.decompressed() {
        Ok(bytes) => bytes,
        Err(_) => return RAYSEND_ERR_INTERNAL,
    };
    if copy_vec(name.into_bytes(), name_utf8, name_len) != RAYSEND_OK {
        return RAYSEND_ERR_INTERNAL;
    }
    copy_vec(data, file_bytes, file_len)
}
