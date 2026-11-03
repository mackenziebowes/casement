//! Times text painting: the cold first draw and the warm median, and checks ink landed.
use casement::text::{draw_text, Fonts};
use casement::ui::Rect;
use std::time::Instant;

fn main() {
    let size = (640u32, 360u32);
    let mut frame = vec![0x20u8; (size.0 * size.1 * 4) as usize];
    let bounds = Rect { x: 16, y: 80, w: 400, h: 24 };
    let mut fonts = Fonts::Cold;
    let t = Instant::now();
    draw_text(&mut fonts, &mut frame, size, bounds, "Recording in progress 00:12", 14, 0xFFE8_ECF2);
    println!("cold first draw: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    let mut runs: Vec<f64> = (0..100)
        .map(|_| {
            let t = Instant::now();
            draw_text(&mut fonts, &mut frame, size, bounds, "Recording in progress 00:12", 14, 0xFFE8_ECF2);
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    runs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("warm redraw median: {:.3} ms (min {:.3}, max {:.3})", runs[50], runs[0], runs[99]);
    let ink = frame.chunks_exact(4).filter(|p| p[0] > 0x80).count();
    println!("bright pixels: {ink}");
}
