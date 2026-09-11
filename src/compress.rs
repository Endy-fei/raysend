use std::io::Write;

pub fn compress(input: Vec<u8>) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut output, 4096, 5, 22);
        writer.write_all(&input).expect("Failed compressing.");
    }
    output
}

pub fn decompress(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut writer = brotli::DecompressorWriter::new(&mut output, 4096);
        writer.write_all(input).expect("Failed decompressing.");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let data: Vec<u8> = (0..4_000).map(|i| (i % 97) as u8).collect();
        let compressed = compress(data.clone());
        let decompressed = decompress(&compressed);
        assert_eq!(data, decompressed);
    }
}
