#ifndef RAYSEND_H
#define RAYSEND_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * RaySend 稳定 C ABI，供 Android JNI、iOS Swift、鸿蒙 NAPI 绑定。
 * iOS 链接 staticlib，Android / 鸿蒙 / Windows 链接 cdylib。
 *
 * 相机、窗口、文件系统仍由平台实现。本库只做压缩、喷泉码、二维码光栅与扫码。
 * 句柄非线程安全，同一句柄上的调用必须串行。
 *
 * 由本库分配的缓冲一律用 raysend_free 释放。
 */

#define RAYSEND_OK 0
#define RAYSEND_ERR_NULL -1
#define RAYSEND_ERR_ARG -2
#define RAYSEND_ERR_BUF -3
#define RAYSEND_ERR_EMPTY -4
#define RAYSEND_ERR_TOO_LARGE -5
#define RAYSEND_ERR_INTERNAL -6

#define RAYSEND_INGEST_IGNORED 0
#define RAYSEND_INGEST_META 1 /* 首次锁定 session / OTI */
#define RAYSEND_INGEST_DUP 2
#define RAYSEND_INGEST_ACCEPTED 3
#define RAYSEND_INGEST_COMPLETE 4
#define RAYSEND_INGEST_FAILED 5
#define RAYSEND_INGEST_LEGACY 6 /* 旧 QT 协议 */

#define RAYSEND_DENSITY_STABLE 0  /* QR v20 L */
#define RAYSEND_DENSITY_DEFAULT 1 /* QR v27 L */
#define RAYSEND_DENSITY_FAST 2    /* QR v40 L */

typedef struct raysend_sender raysend_sender;
typedef struct raysend_receiver raysend_receiver;

/* 压缩 / 解压。out_ptr 由库分配。 */
int32_t raysend_compress(
    const uint8_t *input,
    size_t input_len,
    uint8_t **out_ptr,
    size_t *out_len
);

int32_t raysend_decompress(
    const uint8_t *input,
    size_t input_len,
    uint8_t **out_ptr,
    size_t *out_len
);

void raysend_free(uint8_t *ptr, size_t len);

/* 发送：打 R2 容器并建立喷泉编码器。density 见 RAYSEND_DENSITY_*。
 * 默认 raysend_sender_new 使用 RAYSEND_DENSITY_DEFAULT。ABI 与旧 QT 不兼容。 */
int32_t raysend_sender_new(
    const uint8_t *name_utf8,
    size_t name_len,
    const uint8_t *file_bytes,
    size_t file_len,
    raysend_sender **out
);

int32_t raysend_sender_new_ex(
    const uint8_t *name_utf8,
    size_t name_len,
    const uint8_t *file_bytes,
    size_t file_len,
    uint8_t density,
    raysend_sender **out
);

void raysend_sender_free(raysend_sender *sender);

/* 下一帧协议载荷。缓冲不够时返回 RAYSEND_ERR_BUF，out_len 为所需长度。 */
int32_t raysend_sender_next(
    raysend_sender *sender,
    uint8_t *out_buf,
    size_t out_cap,
    size_t *out_len
);

int32_t raysend_sender_info(
    const raysend_sender *sender,
    uint64_t *orig_size,
    uint64_t *compressed_size
);

/* RGBA 边长为 *out_px；释放时 len = (*out_px) * (*out_px) * 4。 */
int32_t raysend_qr_rgba(
    const uint8_t *payload,
    size_t payload_len,
    uint32_t out_size,
    uint8_t **out_ptr,
    uint32_t *out_px
);

raysend_receiver *raysend_receiver_new(void);
void raysend_receiver_free(raysend_receiver *receiver);

int32_t raysend_receiver_ingest(
    raysend_receiver *receiver,
    const uint8_t *frame,
    size_t frame_len
);

/* 灰度：width * height 字节，行优先。返回本帧新接受的符号数。 */
int32_t raysend_receiver_scan_luma(
    raysend_receiver *receiver,
    uint32_t width,
    uint32_t height,
    const uint8_t *luma,
    size_t luma_len
);

int32_t raysend_receiver_scan_rgba(
    raysend_receiver *receiver,
    uint32_t width,
    uint32_t height,
    const uint8_t *rgba,
    size_t rgba_len
);

int32_t raysend_receiver_progress(
    const raysend_receiver *receiver,
    uint32_t *unique,
    uint32_t *needed
);

/* 完成后取出 UTF-8 文件名与解压数据，并重置接收器。 */
int32_t raysend_receiver_take(
    raysend_receiver *receiver,
    uint8_t **name_utf8,
    size_t *name_len,
    uint8_t **file_bytes,
    size_t *file_len
);

#ifdef __cplusplus
}
#endif

#endif
