//! RaptorQ 喷泉码。先发源符号再发修包；每帧自描述，漏扫、重复、乱序均可。

use std::collections::HashSet;

use raptorq::{
    Decoder, Encoder, EncoderBuilder, EncodingPacket, ObjectTransmissionInformation,
};

use crate::protocol::{
    decode_container, encode_data, parse_frame, Frame, FrameHeader, Meta, HEADER_LEN,
};
use crate::qr::Density;

/// 默认密度（v27 L）下的 RaptorQ 包上限，等于 `Density::Default.symbol_mtu()`。
pub const SYMBOL_MTU: u16 = Density::Default.symbol_mtu();

/// 限制每源块约 1000 符号（RaptorQ `kl()` ≈ `memory / 64`）。
/// 网页收发都在 WASM 主线程上建计划；大单块会卡在「正在生成喷泉码」或还原阶段。
const DECODER_MEMORY: u64 = 64 * 1024;

/// 按通道 MTU 与解码内存限制构建编码器。
pub fn build_encoder(data: &[u8], symbol_mtu: u16) -> Encoder {
    let mut builder = EncoderBuilder::new();
    builder.set_max_packet_size(symbol_mtu.max(8));
    builder.set_decoder_memory_requirement(DECODER_MEMORY);
    builder.build(data)
}

/// 进度条用的预计所需符号数：源符号数 + 每源块 4 个余量。
pub fn needed_symbols(oti: &ObjectTransmissionInformation) -> usize {
    let symbol_size = oti.symbol_size().max(1) as u64;
    let kt = oti.transfer_length().div_ceil(symbol_size);
    kt as usize + oti.source_blocks() as usize * 4
}

/// 循环产出自描述修复/源包。
pub struct FountainSender {
    encoder: Encoder,
    session_id: u16,
    container_len: u32,
    oti: [u8; 12],
    symbol_mtu: u16,
    seq: u32,
    source_block: usize,
    source_esi: usize,
    cached_source: Vec<Vec<u8>>,
    repair_seq: u64,
}

impl FountainSender {
    /// 用已编码容器建立发送端。
    pub fn new(session_id: u16, container: &[u8], symbol_mtu: u16) -> Self {
        let encoder = build_encoder(container, symbol_mtu);
        let oti = encoder.get_config().serialize();
        Self {
            encoder,
            session_id,
            container_len: container.len() as u32,
            oti,
            symbol_mtu,
            seq: 0,
            source_block: 0,
            source_esi: 0,
            cached_source: Vec::new(),
            repair_seq: 0,
        }
    }

    pub fn session_id(&self) -> u16 {
        self.session_id
    }

    pub fn symbol_mtu(&self) -> u16 {
        self.symbol_mtu
    }

    /// 当前对象的 RaptorQ 传输参数。
    pub fn config(&self) -> ObjectTransmissionInformation {
        self.encoder.get_config()
    }

    /// 下一帧协议字节。
    pub fn next_payload(&mut self) -> Vec<u8> {
        let packet = self.next_data_packet();
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        encode_data(self.session_id, seq, &self.oti, self.container_len, &packet)
    }

    fn next_data_packet(&mut self) -> Vec<u8> {
        let n_blocks = self.encoder.get_block_encoders().len();
        if n_blocks == 0 {
            return Vec::new();
        }
        if self.source_block < n_blocks {
            if self.cached_source.is_empty() {
                self.cached_source = self.encoder.get_block_encoders()[self.source_block]
                    .source_packets()
                    .into_iter()
                    .map(|p| p.serialize())
                    .collect();
                self.source_esi = 0;
            }
            if self.source_esi < self.cached_source.len() {
                let packet = self.cached_source[self.source_esi].clone();
                self.source_esi += 1;
                return packet;
            }
            self.source_block += 1;
            self.cached_source.clear();
            return self.next_data_packet();
        }

        let blocks = self.encoder.get_block_encoders();
        let block_idx = (self.repair_seq as usize) % n_blocks;
        let repair_id = (self.repair_seq / n_blocks as u64) as u32;
        self.repair_seq += 1;
        blocks[block_idx]
            .repair_packets(repair_id, 1)
            .into_iter()
            .next()
            .map(|p| p.serialize())
            .unwrap_or_default()
    }
}

/// 把一帧喂给接收器后的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestResult {
    /// 非 `R2` 帧。
    Ignored,
    /// 扫到旧 `QT` 协议。
    Legacy,
    /// 首次锁定 session / OTI。
    Meta,
    /// 重复的 `(块号, ESI)`。
    Duplicate,
    /// 新的互异符号。
    Accepted { unique: usize, needed: usize },
    /// 已还原并校验通过。
    Complete { meta: Meta, data: Vec<u8> },
    /// 哈希失败等不可恢复错误。
    Failed(String),
}

/// 喷泉接收器。任意帧可锁定会话。
pub struct FountainReceiver {
    decoder: Option<Decoder>,
    header: Option<FrameHeader>,
    unique: HashSet<(u8, u32)>,
    needed: usize,
    symbol_mtu: u16,
}

impl Default for FountainReceiver {
    fn default() -> Self {
        Self::new()
    }
}

impl FountainReceiver {
    pub fn new() -> Self {
        Self {
            decoder: None,
            header: None,
            unique: HashSet::new(),
            needed: 0,
            symbol_mtu: SYMBOL_MTU,
        }
    }

    pub fn unique_count(&self) -> usize {
        self.unique.len()
    }

    pub fn needed(&self) -> usize {
        self.needed
    }

    pub fn symbol_mtu(&self) -> u16 {
        self.symbol_mtu
    }

    pub fn has_meta(&self) -> bool {
        self.header.is_some()
    }

    /// 解析并消化一帧。重复与垃圾输入不会中断会话。
    pub fn ingest(&mut self, frame: &[u8]) -> IngestResult {
        let parsed = match parse_frame(frame) {
            Some(frame) => frame,
            None => return IngestResult::Ignored,
        };

        match parsed {
            Frame::Legacy => IngestResult::Legacy,
            Frame::Data(header, packet) => self.ingest_data(header, packet, frame.len()),
        }
    }

    fn ingest_data(&mut self, header: FrameHeader, packet: Vec<u8>, frame_len: usize) -> IngestResult {
        if packet.len() < 4 {
            return IngestResult::Ignored;
        }

        let first = self.header.is_none();
        if let Some(current) = &self.header {
            if current.session_id != header.session_id || current.oti != header.oti {
                *self = Self::new();
                return self.ingest_data(header, packet, frame_len);
            }
        }

        if first {
            let oti = ObjectTransmissionInformation::deserialize(&header.oti);
            self.needed = needed_symbols(&oti);
            self.decoder = Some(Decoder::new(oti));
            self.symbol_mtu = (frame_len.saturating_sub(HEADER_LEN) as u16).max(1);
            self.header = Some(header);
        }

        let Some(decoder) = self.decoder.as_mut() else {
            return IngestResult::Ignored;
        };

        let block = packet[0];
        let esi = ((packet[1] as u32) << 16) | ((packet[2] as u32) << 8) | packet[3] as u32;
        if !self.unique.insert((block, esi)) {
            return if first {
                IngestResult::Meta
            } else {
                IngestResult::Duplicate
            };
        }

        let decoded = decoder.decode(EncodingPacket::deserialize(&packet));
        if let Some(data) = decoded {
            return self.finish(data);
        }

        if first {
            IngestResult::Meta
        } else {
            IngestResult::Accepted {
                unique: self.unique.len(),
                needed: self.needed,
            }
        }
    }

    fn finish(&mut self, data: Vec<u8>) -> IngestResult {
        let Some(header) = self.header.clone() else {
            return IngestResult::Failed("missing_metadata".into());
        };
        match decode_container(&data) {
            Ok((mut meta, original)) => {
                meta.oti = header.oti;
                meta.session_id = header.session_id;
                IngestResult::Complete {
                    meta,
                    data: original,
                }
            }
            Err(err) => IngestResult::Failed(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::encode_container;

    fn sender_for(name: &str, data: &[u8]) -> FountainSender {
        let container = encode_container(name, "", data);
        FountainSender::new(1, &container, Density::Stable.symbol_mtu())
    }

    #[test]
    fn recovers_with_packet_loss() {
        let data: Vec<u8> = (0..80_000).map(|i| (i % 251) as u8).collect();
        let mut sender = sender_for("loss.bin", &data);
        let mut receiver = FountainReceiver::new();

        let mut complete = None;
        for i in 0..4_000 {
            let payload = sender.next_payload();
            if i % 4 == 0 {
                continue;
            }
            match receiver.ingest(&payload) {
                IngestResult::Complete { data: recovered, .. } => {
                    complete = Some(recovered);
                    break;
                }
                IngestResult::Failed(err) => panic!("{err}"),
                IngestResult::Legacy => panic!("legacy"),
                _ => {}
            }
        }

        assert_eq!(complete.expect("should recover"), data);
    }

    #[test]
    fn metadata_can_arrive_late() {
        let data: Vec<u8> = (0..2_000).map(|i| i as u8).collect();
        let mut sender = sender_for("late.bin", &data);
        let mut receiver = FountainReceiver::new();
        let mut complete = None;
        for _ in 0..800 {
            if let IngestResult::Complete { data: recovered, .. } = receiver.ingest(&sender.next_payload())
            {
                complete = Some(recovered);
                break;
            }
        }
        assert_eq!(complete.expect("should recover"), data);
    }

    #[test]
    fn large_payload_is_split_into_small_blocks() {
        let data = vec![0xA5u8; 2_000_000];
        let encoder = build_encoder(&data, Density::Stable.symbol_mtu());
        let cfg = encoder.get_config();
        assert!(
            cfg.source_blocks() >= 2,
            "WASM 内存限制应拆成多个源块，实际 {}",
            cfg.source_blocks()
        );
    }

    #[test]
    fn golden_first_frame_is_stable() {
        let container = encode_container("gold.txt", "text/plain", b"abc");
        let mut sender = FountainSender::new(0x1234, &container, Density::Stable.symbol_mtu());
        let a = sender.next_payload();
        let mut sender2 = FountainSender::new(0x1234, &container, Density::Stable.symbol_mtu());
        let b = sender2.next_payload();
        assert_eq!(a, b);
        assert_eq!(&a[..2], &crate::protocol::MAGIC);
        assert_eq!(a[2], crate::protocol::WIRE_VERSION);
        assert_eq!(&a[4..6], &0x1234u16.to_be_bytes());
        assert_eq!(&a[6..10], &0u32.to_be_bytes());
        assert!(matches!(parse_frame(&a), Some(Frame::Data(_, _))));
    }

    #[test]
    fn rejects_legacy_qt() {
        let mut receiver = FountainReceiver::new();
        assert_eq!(receiver.ingest(b"QTMxxxx"), IngestResult::Legacy);
    }
}
