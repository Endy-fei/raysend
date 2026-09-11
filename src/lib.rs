use dioxus::signals::{GlobalSignal, Signal};

pub mod compress;
pub mod fountain;
pub mod i18n;
pub mod protocol;
pub mod receive;
pub mod send;
pub mod utils;

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

#[derive(Clone, Default, PartialEq)]
pub struct ReceiveUi {
    pub status: ReceiveStatus,
    pub percent: f32,
    pub unique: usize,
    pub needed: usize,
    pub scanning: bool,
}

#[derive(Clone)]
pub struct ReceiveResult {
    pub name: String,
    pub size: u64,
    pub data: Vec<u8>,
}

pub static SEND_SESSION: GlobalSignal<Option<send::SendSession>> = Signal::global(|| None);
pub static SEND_STATUS: GlobalSignal<SendStatus> = Signal::global(SendStatus::default);
pub static CAMERA_FACING: GlobalSignal<String> = Signal::global(|| "environment".to_string());
pub static RECEIVE_UI: GlobalSignal<ReceiveUi> = Signal::global(ReceiveUi::default);
pub static RECEIVE_RESULT: GlobalSignal<Option<ReceiveResult>> = Signal::global(|| None);

#[cfg(test)]
mod tests {
    use crate::compress::{compress, decompress};
    use crate::fountain::{FountainReceiver, FountainSender, IngestResult};
    use crate::utils::hash_bytes;

    #[test]
    fn end_to_end_with_loss() {
        let file_name = "test_raysend.txt";
        let file_content = "Transfer your file from an air gapped computer to iOS/iPhone/iPad using only qrcode, no wifi/usb/bluetooth needed. This is a proof-of-concept project, implemented in Rust WebAssembly.";
        let original = Vec::from(file_content.as_bytes());
        let compressed = compress(original.clone());
        let hash = hash_bytes(&compressed);
        let mut sender = FountainSender::new(file_name, original.len() as u32, hash, &compressed);
        let mut receiver = FountainReceiver::new();

        let mut recovered = None;
        for _ in 0..2_000 {
            match receiver.ingest(&sender.next_payload()) {
                IngestResult::Complete { data, .. } => {
                    recovered = Some(data);
                    break;
                }
                IngestResult::Failed(err) => panic!("{err}"),
                _ => {}
            }
        }

        let decoded = decompress(&recovered.expect("should recover"));
        assert_eq!(decoded, original);
    }
}
