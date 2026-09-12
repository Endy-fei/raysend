//! 本机测 QR 生成 / 识别耗时，用来对照纸面吞吐。
//!
//! ```bash
//! cargo run -p raysend-core --example scan_cost --release
//! ```

use std::time::Instant;

use raysend_core::qr::{render_qr_rgba_density, Density};
use raysend_core::session::Outgoing;
use raysend_core::{decode_from_quad, decode_qr_luma, rgba_to_luma, QrScan};

fn main() {
    println!("RaySend scan_cost (release)\n");
    println!(
        "{:<10} {:>8} {:>10} {:>12} {:>12} {:>12}",
        "density", "bytes", "render", "decode", "paper@24", "paper@60"
    );

    for density in [Density::Stable, Density::Default, Density::Fast] {
        let mut outgoing =
            Outgoing::prepare_with("perf.bin".into(), vec![0x5a; 64 * 1024], density).unwrap();
        let payload = outgoing.next_payloads(1).remove(0);
        let side = match density {
            Density::Stable => 400,
            Density::Default => 520,
            Density::Fast => 720,
        };

        let warmup = render_qr_rgba_density(&payload, side, density).unwrap();
        let luma = rgba_to_luma(warmup.0, warmup.0, &warmup.1);
        let _ = decode_qr_luma(warmup.0, warmup.0, &luma);

        let n = 20;
        let t0 = Instant::now();
        let mut last = warmup;
        for _ in 0..n {
            last = render_qr_rgba_density(&payload, side, density).unwrap();
        }
        let render_us = t0.elapsed().as_secs_f64() * 1_000_000.0 / n as f64;

        let luma = rgba_to_luma(last.0, last.0, &last.1);
        let t1 = Instant::now();
        let mut hits = 0usize;
        for _ in 0..n {
            let (found, _) = decode_qr_luma(last.0, last.0, &luma);
            if found.iter().any(|p| p == &payload) {
                hits += 1;
            }
        }
        let decode_us = t1.elapsed().as_secs_f64() * 1_000_000.0 / n as f64;

        let mtu = density.symbol_mtu() as u64;
        let mut tracker = QrScan::new();
        let _ = tracker.scan_full(last.0, last.0, &luma);
        let tracked_us = tracker
            .planned_crops(last.0, last.0)
            .and_then(|crops| crops.into_iter().next())
            .and_then(|crop| {
                let hint = crop.hint?;
                let local = hint.offset(-(crop.region.x as i32), -(crop.region.y as i32));
                let (cw, ch, buf) = {
                    let x = crop.region.x.min(last.0.saturating_sub(1));
                    let y = crop.region.y.min(last.0.saturating_sub(1));
                    let w = crop.region.w.min(last.0.saturating_sub(x)).max(1);
                    let h = crop.region.h.min(last.0.saturating_sub(y)).max(1);
                    let mut out = vec![0u8; w as usize * h as usize];
                    for row in 0..h {
                        let src = ((y + row) * last.0 + x) as usize;
                        let dst = (row * w) as usize;
                        out[dst..dst + w as usize]
                            .copy_from_slice(&luma[src..src + w as usize]);
                    }
                    (w, h, out)
                };
                let t2 = Instant::now();
                let mut ok = 0usize;
                for _ in 0..n {
                    if decode_from_quad(cw, ch, &buf, local)
                        .is_some_and(|(bytes, _, _)| bytes == payload)
                    {
                        ok += 1;
                    }
                }
                let us = t2.elapsed().as_secs_f64() * 1_000_000.0 / n as f64;
                Some((us, ok))
            });

        print!(
            "{:<10} {:>8} {:>8.1}µs {:>10.1}µs {:>8.1} KB/s {:>8.1} KB/s  detect={}/{}",
            density,
            payload.len(),
            render_us,
            decode_us,
            mtu as f64 * 24.0 / 1024.0,
            mtu as f64 * 60.0 / 1024.0,
            hits,
            n
        );
        if let Some((us, ok)) = tracked_us {
            println!("  tracked={us:.1}µs {ok}/{n}");
        } else {
            println!("  tracked=n/a");
        }
    }

    println!(
        "\n纸面 = symbol_mtu × fps（未计捕获率）。UI 估算再乘 0.7。\
         \n默认档 v27 @ 24：约 {:.1} KB/s 毛吞吐，×0.7 ≈ {:.1} KB/s。\
         \n2×2 再 ×4。Decimen 默认接近 v40 @ 60 单码。",
        Density::Default.symbol_mtu() as f64 * 24.0 / 1024.0,
        Density::Default.symbol_mtu() as f64 * 24.0 * 0.7 / 1024.0
    );
}
