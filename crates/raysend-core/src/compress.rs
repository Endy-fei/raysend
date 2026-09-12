//! Brotli 压缩与解压。质量 5、窗口 22，兼顾体积与 WASM/原生启动耗时。

use std::io::Write;

/// 压缩原始文件字节。已压缩格式（zip、jpg 等）通常几乎不再缩小。
pub fn compress(input: Vec<u8>) -> Vec<u8> {
    let mut output = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut output, 4096, 5, 22);
        writer.write_all(&input).expect("Failed compressing.");
    }
    output
}

/// 仅当压缩后更小时采用 Brotli；否则返回原文。第二个值表示是否压缩。
pub fn compress_if_smaller(input: &[u8]) -> (Vec<u8>, bool) {
    let compressed = compress(input.to_vec());
    if compressed.len() < input.len() {
        (compressed, true)
    } else {
        (input.to_vec(), false)
    }
}

/// 解压喷泉码还原出的压缩载荷。
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

    #[test]
    fn skips_when_larger() {
        let data: Vec<u8> = (0..64u8).map(|i| i.wrapping_mul(17)).collect();
        let (out, used) = compress_if_smaller(&data);
        if used {
            assert!(out.len() < data.len());
        } else {
            assert_eq!(out, data);
        }
    }
}
