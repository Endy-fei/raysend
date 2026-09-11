use std::collections::HashSet;

use raptorq::{
    Decoder, Encoder, EncoderBuilder, EncodingPacket, ObjectTransmissionInformation,
};

use crate::protocol::{encode_data, encode_meta, parse_frame, Frame, Meta};
use crate::utils::hash_bytes;

pub const SYMBOL_MTU: u16 = 640;
pub const META_INTERVAL: u64 = 8;

pub fn build_encoder(data: &[u8]) -> Encoder {
    let mut builder = EncoderBuilder::new();
    builder.set_max_packet_size(SYMBOL_MTU);
    builder.set_decoder_memory_requirement(48 * 1024 * 1024);
    builder.build(data)
}

pub fn needed_symbols(oti: &ObjectTransmissionInformation) -> usize {
    let symbol_size = oti.symbol_size().max(1) as u64;
    let kt = oti.transfer_length().div_ceil(symbol_size);
    kt as usize + oti.source_blocks() as usize * 4
}

pub struct FountainSender {
    encoder: Encoder,
    meta_bytes: Vec<u8>,
    seq: u64,
    data_seq: u64,
}

impl FountainSender {
    pub fn new(name: &str, orig_len: u32, hash: [u8; 20], compressed: &[u8]) -> Self {
        let encoder = build_encoder(compressed);
        let oti = encoder.get_config().serialize();
        Self {
            meta_bytes: encode_meta(name, orig_len, &hash, &oti),
            encoder,
            seq: 0,
            data_seq: 0,
        }
    }

    #[allow(dead_code)]
    pub fn config(&self) -> ObjectTransmissionInformation {
        self.encoder.get_config()
    }

    pub fn next_payload(&mut self) -> Vec<u8> {
        self.seq += 1;
        if self.seq <= 2 || self.seq % META_INTERVAL == 0 {
            self.meta_bytes.clone()
        } else {
            encode_data(&self.next_data_packet())
        }
    }

    fn next_data_packet(&mut self) -> Vec<u8> {
        let blocks = self.encoder.get_block_encoders();
        let n_blocks = blocks.len().max(1);
        let block_idx = (self.data_seq as usize) % n_blocks;
        let repair_id = (self.data_seq / n_blocks as u64) as u32;
        self.data_seq += 1;
        let packet = blocks[block_idx].repair_packets(repair_id, 1);
        packet[0].serialize()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestResult {
    Ignored,
    Meta,
    Duplicate,
    Accepted { unique: usize, needed: usize },
    Complete { meta: Meta, data: Vec<u8> },
    Failed(String),
}

pub struct FountainReceiver {
    decoder: Option<Decoder>,
    meta: Option<Meta>,
    unique: HashSet<(u8, u32)>,
    pending: Vec<Vec<u8>>,
    needed: usize,
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
            meta: None,
            unique: HashSet::new(),
            pending: Vec::new(),
            needed: 0,
        }
    }

    pub fn unique_count(&self) -> usize {
        self.unique.len()
    }

    pub fn needed(&self) -> usize {
        self.needed
    }

    pub fn has_meta(&self) -> bool {
        self.meta.is_some()
    }

    pub fn ingest(&mut self, frame: &[u8]) -> IngestResult {
        let parsed = match parse_frame(frame) {
            Some(frame) => frame,
            None => return IngestResult::Ignored,
        };

        match parsed {
            Frame::Meta(meta) => self.ingest_meta(meta),
            Frame::Data(packet) => self.ingest_data(packet),
        }
    }

    fn ingest_meta(&mut self, meta: Meta) -> IngestResult {
        if self.meta.is_some() {
            return IngestResult::Duplicate;
        }

        let oti_bytes: [u8; 12] = meta.oti;
        let oti = ObjectTransmissionInformation::deserialize(&oti_bytes);
        self.needed = needed_symbols(&oti);
        self.decoder = Some(Decoder::new(oti));
        self.meta = Some(meta);

        let pending = std::mem::take(&mut self.pending);
        let mut outcome = IngestResult::Meta;
        for packet in pending {
            outcome = self.ingest_data(packet);
            if matches!(outcome, IngestResult::Complete { .. } | IngestResult::Failed(_)) {
                return outcome;
            }
        }
        outcome
    }

    fn ingest_data(&mut self, packet: Vec<u8>) -> IngestResult {
        if packet.len() < 4 {
            return IngestResult::Ignored;
        }

        let Some(decoder) = self.decoder.as_mut() else {
            if self.pending.len() < 512 {
                self.pending.push(packet);
            }
            return IngestResult::Ignored;
        };

        let block = packet[0];
        let esi = ((packet[1] as u32) << 16) | ((packet[2] as u32) << 8) | packet[3] as u32;
        if !self.unique.insert((block, esi)) {
            return IngestResult::Duplicate;
        }

        let decoded = decoder.decode(EncodingPacket::deserialize(&packet));
        if let Some(data) = decoded {
            return self.finish(data);
        }

        IngestResult::Accepted {
            unique: self.unique.len(),
            needed: self.needed,
        }
    }

    fn finish(&mut self, data: Vec<u8>) -> IngestResult {
        let Some(meta) = self.meta.clone() else {
            return IngestResult::Failed("missing_metadata".into());
        };
        let actual = hash_bytes(&data);
        if actual != meta.hash {
            return IngestResult::Failed("integrity".into());
        }
        IngestResult::Complete { meta, data }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_with_packet_loss() {
        let data: Vec<u8> = (0..80_000).map(|i| (i % 251) as u8).collect();
        let hash = hash_bytes(&data);
        let mut sender = FountainSender::new("loss.bin", data.len() as u32, hash, &data);
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
                _ => {}
            }
        }

        assert_eq!(complete.expect("should recover"), data);
    }

    #[test]
    fn metadata_can_arrive_late() {
        let data: Vec<u8> = (0..2_000).map(|i| i as u8).collect();
        let hash = hash_bytes(&data);
        let mut sender = FountainSender::new("late.bin", data.len() as u32, hash, &data);
        let mut receiver = FountainReceiver::new();

        let mut buffered = Vec::new();
        for _ in 0..80 {
            let payload = sender.next_payload();
            if parse_frame(&payload).map(|f| matches!(f, Frame::Meta(_))) == Some(true) {
                buffered.push(payload);
                continue;
            }
            receiver.ingest(&payload);
        }
        let mut complete = None;
        for payload in buffered {
            if let IngestResult::Complete { data: recovered, .. } = receiver.ingest(&payload) {
                complete = Some(recovered);
            }
        }
        if complete.is_none() {
            for _ in 0..400 {
                if let IngestResult::Complete { data: recovered, .. } =
                    receiver.ingest(&sender.next_payload())
                {
                    complete = Some(recovered);
                    break;
                }
            }
        }
        assert_eq!(complete.expect("should recover"), data);
    }
}
