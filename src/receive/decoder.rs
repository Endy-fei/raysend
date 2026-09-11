use crate::compress::decompress;
use crate::fountain::{FountainReceiver, IngestResult};
use crate::protocol::Meta;
use crate::utils::log;
use quircs::Quirc;

pub struct Finished {
    pub meta: Meta,
    pub data: Vec<u8>,
}

impl Finished {
    pub fn get_name(&self) -> String {
        if self.meta.name.is_empty() {
            "download.bin".into()
        } else {
            self.meta.name.clone()
        }
    }

    pub fn decompressed(&self) -> Result<Vec<u8>, String> {
        log("Decompressing...");
        let data = decompress(&self.data);
        if data.len() as u32 != self.meta.orig_len {
            return Err("size_mismatch".into());
        }
        Ok(data)
    }
}

pub struct Decoder {
    scanner: Quirc,
    fountain: FountainReceiver,
    finished: Option<Finished>,
    failed: Option<String>,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            scanner: Quirc::default(),
            fountain: FountainReceiver::new(),
            finished: None,
            failed: None,
        }
    }

    pub fn get_progress(&self) -> String {
        let lang = crate::i18n::current();
        if let Some(err) = &self.failed {
            return lang.error(err);
        }
        if self.finished.is_some() {
            return lang.t("progress_finished").to_string();
        }
        if !self.fountain.has_meta() {
            return lang.t("progress_handshake").to_string();
        }
        lang.symbols(self.fountain.unique_count(), self.fountain.needed().max(1))
    }

    pub fn unique_count(&self) -> usize {
        self.fountain.unique_count()
    }

    pub fn needed(&self) -> usize {
        self.fountain.needed()
    }

    pub fn is_finished(&self) -> bool {
        self.finished.is_some()
    }

    pub fn error(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    pub fn process_frame(&mut self, frame: &[u8]) -> usize {
        if self.finished.is_some() || self.failed.is_some() {
            return 0;
        }
        match self.fountain.ingest(frame) {
            IngestResult::Accepted { .. } | IngestResult::Meta => 1,
            IngestResult::Complete { meta, data } => {
                self.finished = Some(Finished { meta, data });
                1
            }
            IngestResult::Failed(err) => {
                log(&err);
                self.failed = Some(err);
                0
            }
            IngestResult::Duplicate | IngestResult::Ignored => 0,
        }
    }

    pub fn scan(&mut self, width: u32, height: u32, rgba: Vec<u8>) -> usize {
        if width == 0 || height == 0 {
            return 0;
        }
        let expected = width as usize * height as usize * 4;
        if rgba.len() < expected {
            return 0;
        }

        let mut luma = vec![0u8; width as usize * height as usize];
        for i in 0..luma.len() {
            let r = rgba[i * 4] as u32;
            let g = rgba[i * 4 + 1] as u32;
            let b = rgba[i * 4 + 2] as u32;
            luma[i] = ((r * 77 + g * 150 + b * 29) >> 8) as u8;
        }

        let codes: Vec<_> = self
            .scanner
            .identify(width as usize, height as usize, &luma)
            .flatten()
            .collect();

        let mut counter = 0;
        for code in codes {
            if let Ok(decoded) = code.decode() {
                counter += self.process_frame(&decoded.payload);
            }
        }
        counter
    }

    pub fn take_finished(self) -> Option<Finished> {
        self.finished
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fountain::FountainSender;
    use crate::utils::hash_bytes;

    #[test]
    fn decoder_rebuilds_file() {
        let payload = b"hello from raysend fountain".to_vec();
        let hash = hash_bytes(&payload);
        let mut sender = FountainSender::new("note.txt", payload.len() as u32, hash, &payload);
        let mut decoder = Decoder::new();

        for _ in 0..400 {
            decoder.process_frame(&sender.next_payload());
            if decoder.is_finished() {
                break;
            }
        }

        let finished = decoder.take_finished().expect("finished");
        assert_eq!(finished.meta.name, "note.txt");
        assert_eq!(finished.data, payload);
    }
}
