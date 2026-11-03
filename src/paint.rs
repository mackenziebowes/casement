//! Painting a node tree into a BGRA frame. Tone becomes colour here and nowhere else.
//! Labels draw their text with `text.rs`, in a colour chosen to read against what they sit on.

use crate::input::{Focus, Input, Keys, Pointer};
use crate::text::{draw_text, text_width, Fonts};
use crate::ui::{Form, BarMark, Dot, Line, Shape, Node, Reading, Rect, Role, Tone};

/// 0xAARRGGBB.
pub fn colour_of(t: Tone) -> u32 {
    match t {
        Tone::Neutral => 0xFF20_242C,
        Tone::Positive => 0xFF2E_9E5B,
        Tone::Warning => 0xFFE0_A030,
        Tone::Critical => 0xFFD0_3A3A,
        Tone::Muted => 0xFF3A_404C,
    }
}

fn fill(frame: &mut [u8], size: (u32, u32), r: Rect, argb: u32) {
    let bytes = argb.to_le_bytes();
    let (fw, fh) = (size.0 as i32, size.1 as i32);
    let (x0, x1) = (r.x.clamp(0, fw), (r.x + r.w).clamp(0, fw));
    let (y0, y1) = (r.y.clamp(0, fh), (r.y + r.h).clamp(0, fh));
    for y in y0..y1 {
        let row = (y * fw) as usize * 4;
        for px in frame[row + x0 as usize * 4..row + x1 as usize * 4].chunks_exact_mut(4) {
            px.copy_from_slice(&bytes);
        }
    }
}

/// Text colour on a tone: dark on the light tones, light on the dark ones.
fn ink_on(background: Tone) -> u32 {
    match background {
        Tone::Warning => 0xFF14_1820,
        _ => 0xFFF2_F4F8,
    }
}

/// Move each colour channel `pct` percent of the way to `to` (0x00 or 0xFF).
fn shade(argb: u32, to: u32, pct: u32) -> u32 {
    let mix = |c: u32| (c * (100 - pct) + to * pct) / 100;
    0xFF00_0000 | mix((argb >> 16) & 0xFF) << 16 | mix((argb >> 8) & 0xFF) << 8 | mix(argb & 0xFF)
}

/// A rectangle's outline, `t` pixels thick, drawn inside its bounds.
fn outline(frame: &mut [u8], size: (u32, u32), r: Rect, t: i32, argb: u32) {
    fill(frame, size, Rect { h: t, ..r }, argb);
    fill(frame, size, Rect { y: r.y + r.h - t, h: t, ..r }, argb);
    fill(frame, size, Rect { w: t, ..r }, argb);
    fill(frame, size, Rect { x: r.x + r.w - t, w: t, ..r }, argb);
}

/// A straight line of `thick` pixels from a to b, by stepping along the longer axis.
fn stroke(frame: &mut [u8], size: (u32, u32), a: (i32, i32), b: (i32, i32), thick: i32, argb: u32, dashed: bool) {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
    for i in 0..=steps {
        // Dashes: a baseline is read as "the usual", so it is drawn in broken strokes.
        if dashed && (i / (3 * thick).max(1)) % 2 == 1 {
            continue;
        }
        let (x, y) = (a.0 + (b.0 - a.0) * i / steps, a.1 + (b.1 - a.1) * i / steps);
        fill(frame, size, Rect { x: x - thick / 2, y: y - thick / 2, w: thick, h: thick }, argb);
    }
}

/// A circle by testing each pixel of its box: filled, an outline of 1 px, or a thick ring.
fn draw_dot(frame: &mut [u8], size: (u32, u32), d: &Dot, scale: u32) {
    let (cx, cy, r) = (px(d.x, scale), px(d.y, scale), px(d.r, scale).max(2));
    let argb = match d.tone {
        Tone::Neutral => 0xFFD8_DCE4,
        t => ink_for(t, Tone::Neutral),
    };
    let inner = match d.shape {
        Shape::Filled => -1,
        Shape::Hollow => (r - px(2, scale).max(1)).max(0),
        Shape::Ring => (r / 2).max(1),
    };
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dy * dy;
            let ring_hole = inner >= 0 && d2 < inner * inner;
            if d2 <= r * r && !ring_hole {
                fill(frame, size, Rect { x: cx + dx, y: cy + dy, w: 1, h: 1 }, argb);
            }
        }
    }
}

fn role_colour(role: Role) -> u32 {
    match role {
        Role::Grid => 0xFF3A_404C,
        Role::Primary => 0xFF6E_A8FE,
        Role::Baseline => 0xFF9A_A1B0,
        Role::Comparison => 0xFFF0_C060,
    }
}

fn draw_line(frame: &mut [u8], size: (u32, u32), line: &Line, scale: u32) {
    let (argb, thick, dashed, dots) = match line.role {
        Role::Grid => (0xFF3A_404C, 1, false, false),
        Role::Primary => (0xFF6E_A8FE, px(2, scale).max(1), false, true),
        Role::Baseline => (0xFF9A_A1B0, px(2, scale).max(1), true, false),
        Role::Comparison => (0xFFF0_C060, px(2, scale).max(1), false, true),
    };
    let at = |p: &(i32, i32)| (px(p.0, scale), px(p.1, scale));
    let mut previous: Option<(i32, i32)> = None;
    for point in &line.points {
        let here = point.as_ref().map(at);
        if let (Some(a), Some(b)) = (previous, here) {
            stroke(frame, size, a, b, thick, argb, dashed);
        }
        if let (true, Some((x, y))) = (dots, here) {
            let r = px(4, scale).max(2);
            fill(frame, size, Rect { x: x - r / 2, y: y - r / 2, w: r, h: r }, argb);
        }
        previous = here;
    }
}

/// What the surface is scaled by, in 120ths (the unit wp_fractional_scale speaks): 120 is 1.0, 150 is 1.25.
pub const WHOLE: u32 = 120;

/// A logical length in device pixels.
/// Source-over blend of `argb` at `coverage` (0 to 1) onto one BGRA pixel.
fn blend_px(px: &mut [u8], argb: u32, coverage: f32) {
    let a = ((argb >> 24) as f32 / 255.0) * coverage;
    if a <= 0.0 {
        return;
    }
    for (dst, src) in px[..3].iter_mut().zip([argb & 0xFF, (argb >> 8) & 0xFF, (argb >> 16) & 0xFF]) {
        *dst = (src as f32 * a + *dst as f32 * (1.0 - a)).round() as u8;
    }
    px[3] = 255;
}

/// A form with soft edges: each pixel's coverage comes from its distance to the form's edge (a signed distance, one pixel wide).
fn blob(frame: &mut [u8], size: (u32, u32), r: Rect, form: Form, argb: u32) {
    let (fw, fh) = (size.0 as i32, size.1 as i32);
    let (hx, hy) = (r.w as f32 / 2.0, r.h as f32 / 2.0);
    let (cx, cy) = (r.x as f32 + hx, r.y as f32 + hy);
    let pad = if let Form::Arc { width, .. } = form { width + 2 } else { 2 };
    for y in (r.y - pad).max(0)..(r.y + r.h + pad).min(fh) {
        for x in (r.x - pad).max(0)..(r.x + r.w + pad).min(fw) {
            let (px_, py_) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let d: f32 = match form {
                Form::Rounded { radius } => {
                    let rad = (radius as f32).clamp(0.0, hx.min(hy));
                    let (qx, qy) = (px_.abs() - (hx - rad), py_.abs() - (hy - rad));
                    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - rad
                }
                Form::Disc => (((px_ / hx).powi(2) + (py_ / hy).powi(2)).sqrt() - 1.0) * hx.min(hy),
                Form::Arc { width, from, to } => {
                    let half = width as f32 / 2.0;
                    let (rx, ry) = ((hx - half).max(1.0), (hy - half).max(1.0));
                    let ang = py_.atan2(px_).to_degrees().rem_euclid(360.0);
                    let (a0, a1) = ((from as f32).rem_euclid(360.0), (to as f32).rem_euclid(360.0));
                    let inside = if a0 <= a1 { ang >= a0 && ang <= a1 } else { ang >= a0 || ang <= a1 };
                    let ring = (((px_ / rx).powi(2) + (py_ / ry).powi(2)).sqrt() - 1.0) * rx.min(ry);
                    let at = |deg: f32| (cx + rx * deg.to_radians().cos(), cy + ry * deg.to_radians().sin());
                    let cap = |deg: f32| {
                        let (ex, ey) = at(deg);
                        ((x as f32 + 0.5 - ex).powi(2) + (y as f32 + 0.5 - ey).powi(2)).sqrt()
                    };
                    if inside { ring.abs() - half } else { cap(a0).min(cap(a1)) - half }
                }
            };
            let coverage = (0.5 - d).clamp(0.0, 1.0);
            let i = (y * fw + x) as usize * 4;
            blend_px(&mut frame[i..i + 4], argb, coverage);
        }
    }
}

fn px(v: i32, scale: u32) -> i32 {
    ((v as i64 * scale as i64 + 60) / 120) as i32
}

/// A logical rectangle in device pixels. Each edge is rounded on its own, so neighbours that share
/// an edge in logical pixels still share it in device pixels: no gap, no overlap, no blur.
fn device(r: Rect, scale: u32) -> Rect {
    let (x0, y0, x1, y1) = (px(r.x, scale), px(r.y, scale), px(r.x + r.w, scale), px(r.y + r.h, scale));
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// What a label's own tone says in its text: neutral takes the ink that reads on what it sits on,
/// the others a colour that carries their meaning on a dark ground.
fn ink_for(tone: Tone, behind: Tone) -> u32 {
    match tone {
        Tone::Neutral => ink_on(behind),
        Tone::Positive => 0xFF5B_D08A,
        Tone::Warning => 0xFFF0_C060,
        Tone::Critical => 0xFFFF_7A7A,
        Tone::Muted => 0xFF9A_A1B0,
    }
}

/// `behind` is the tone of the node this one sits on, so text can be read against it.
/// `size` is the frame in device pixels; the node's rectangles are logical and are scaled here.
pub fn paint(fonts: &mut Fonts, frame: &mut [u8], size: (u32, u32), node: &Node, input: &Input, scale: u32) {
    paint_on(fonts, frame, size, node, Tone::Neutral, input, scale)
}

fn paint_on(fonts: &mut Fonts, frame: &mut [u8], size: (u32, u32), node: &Node, behind: Tone, input: &Input, scale: u32) {
    let text_px = px(14, scale).max(1) as u32;
    match node {
        Node::Panel { bounds, tone, children, .. } => {
            fill(frame, size, device(*bounds, scale), colour_of(*tone));
            children.iter().for_each(|c| paint_on(fonts, frame, size, c, *tone, input, scale));
        }
        Node::Button { id, bounds, tone, label } => {
            // Hover lightens, a press that is still on the button darkens; focus is a ring, bright
            // while the window has the keyboard and dim while it does not.
            // A neutral button would vanish into a neutral panel, so it sits one step lighter.
            let base = if *tone == Tone::Neutral { shade(colour_of(Tone::Neutral), 0xFF, 16) } else { colour_of(*tone) };
            let face = match &input.pointer {
                Pointer::Pressed { id: p, over: true } if p == id => shade(base, 0x00, 25),
                Pointer::Over { id: o } if o == id => shade(base, 0xFF, 14),
                _ => base,
            };
            let face_at = device(*bounds, scale);
            fill(frame, size, face_at, face);
            let inset = px(12, scale);
            let line = px(20, scale);
            draw_text(fonts, frame, size, Rect { x: face_at.x + inset, y: face_at.y + (face_at.h - line).max(0) / 2, w: (face_at.w - 2 * inset).max(0), h: line }, label, text_px, ink_on(*tone));
            if matches!(&input.focus, Focus::On { id: f } if f == id) {
                outline(frame, size, face_at, px(2, scale).max(1), if input.keys == Keys::Here { 0xFFF2_F4F8 } else { 0xFF80_8694 });
            }
        }
        Node::Hotspot { .. } => {}
        Node::Blob { bounds, form, fill, .. } => {
            let at = device(*bounds, scale);
            let form = match form {
                Form::Rounded { radius } => Form::Rounded { radius: px(*radius, scale) },
                Form::Arc { width, from, to } => Form::Arc { width: px(*width, scale).max(1), from: *from, to: *to },
                Form::Disc => Form::Disc,
            };
            blob(frame, size, at, form, *fill)
        }
        Node::Words { bounds, px: size_px, ink, text, .. } => draw_text(fonts, frame, size, device(*bounds, scale), text, px(*size_px as i32, scale).max(1) as u32, *ink),
        Node::Headline { bounds, text, tone, .. } => draw_text(fonts, frame, size, device(*bounds, scale), text, px(28, scale).max(1) as u32, ink_for(*tone, behind)),
        Node::Label { bounds, text, tone, .. } => draw_text(fonts, frame, size, device(*bounds, scale), text, text_px, ink_for(*tone, behind)),
        Node::Input { id, bounds, text, caret } => {
            let at = device(*bounds, scale);
            fill(frame, size, at, shade(colour_of(Tone::Muted), 0x00, 20));
            let inset = px(8, scale);
            let line = px(20, scale);
            draw_text(fonts, frame, size, Rect { x: at.x + inset, y: at.y + (at.h - line).max(0) / 2, w: (at.w - 2 * inset).max(0), h: line }, text, text_px, 0xFFF2_F4F8);
            if matches!(&input.focus, Focus::On { id: f } if f == id) {
                outline(frame, size, at, px(2, scale).max(1), if input.keys == Keys::Here { 0xFFF2_F4F8 } else { 0xFF80_8694 });
                // The caret: where the next typed character goes, after the text. Only while the keyboard is ours.
                if input.keys == Keys::Here && !text.starts_with('[') {
                    let before: String = text.chars().take(*caret).collect();
                    let end = at.x + inset + if before.is_empty() { 0 } else { text_width(fonts, &before, text_px) };
                    fill(frame, size, Rect { x: end + 1, y: at.y + (at.h - line) / 2 + 1, w: px(2, scale).max(1), h: line - 2 }, 0xFFF2_F4F8);
                }
            }
        }
        Node::Plot { lines, bars, dots, .. } => {
            dots.iter().for_each(|d| draw_dot(frame, size, d, scale));
            lines.iter().filter(|l| l.role == Role::Grid).for_each(|l| draw_line(frame, size, l, scale));
            for BarMark { role, rect } in bars {
                fill(frame, size, device(*rect, scale), role_colour(*role));
            }
            lines.iter().filter(|l| l.role != Role::Grid).for_each(|l| draw_line(frame, size, l, scale));
        }
        Node::Bar { bounds, tone, reading, .. } => {
            let bounds = &device(*bounds, scale);
            let stripe = px(6, scale).max(1);
            fill(frame, size, *bounds, colour_of(Tone::Muted));
            match reading {
                Reading::Exact(v) | Reading::AtLeast(v) | Reading::Estimate(v) => {
                    fill(frame, size, Rect { w: (bounds.w as f32 * v.clamp(0.0, 1.0)) as i32, ..*bounds }, colour_of(*tone))
                }
                // No number: a striped track that cannot be mistaken for an empty (silent) one.
                Reading::Unknown => {
                    let mut x = bounds.x;
                    while x < bounds.x + bounds.w {
                        fill(frame, size, Rect { x, y: bounds.y, w: stripe.min(bounds.x + bounds.w - x), h: bounds.h }, colour_of(Tone::Neutral));
                        x += 2 * stripe;
                    }
                }
            }
        }
    }
}
