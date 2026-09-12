//! 网页版全局状态。协议与编解码在 `raysend-core`，此处只保留 Dioxus 信号。

use dioxus::signals::{GlobalSignal, Signal};

pub mod i18n;
pub mod receive;
pub mod send;
pub mod utils;

/// 发送页在选文件、压缩过程中的状态。
#[derive(Clone, Default, PartialEq, Eq)]
pub enum SendStatus {
    #[default]
    Idle,
    Reading,
    NoFile,
    EmptyFile,
    TooLarge(String),
    Compressing(String),
    Building,
}

/// 接收页相机与扫描状态。
#[derive(Clone, Default, PartialEq, Eq)]
pub enum ReceiveStatus {
    #[default]
    Idle,
    Requesting,
    Unavailable,
    OpenFailed,
    Denied,
    ViewNotReady,
    CanvasUnavailable,
    Aim,
    Scanning,
    Received,
    Failed(String),
}

/// 接收页进度条绑定的数据。
#[derive(Clone, Default, PartialEq)]
pub struct ReceiveUi {
    pub status: ReceiveStatus,
    pub percent: f32,
    pub unique: usize,
    pub needed: usize,
    pub scanning: bool,
    pub symbol_mtu: u32,
    pub camera_info: String,
    pub live_line: String,
}

/// 接收完成后的本地下载缓存。
#[derive(Clone)]
pub struct ReceiveResult {
    pub name: String,
    pub size: u64,
    pub data: Vec<u8>,
    pub receipt: Option<raysend_core::TransferReceipt>,
}

pub static SEND_SESSION: GlobalSignal<Option<send::SendSession>> = Signal::global(|| None);
pub static SEND_STATUS: GlobalSignal<SendStatus> = Signal::global(SendStatus::default);
pub static CAMERA_FACING: GlobalSignal<String> = Signal::global(|| "environment".to_string());
pub static RECEIVE_UI: GlobalSignal<ReceiveUi> = Signal::global(ReceiveUi::default);
pub static RECEIVE_RESULT: GlobalSignal<Option<ReceiveResult>> = Signal::global(|| None);
