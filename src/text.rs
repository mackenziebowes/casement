//! Text into a BGRA frame with cosmic-text. A line is shaped once per distinct text and size and
//! kept, so a redraw with the same words only rasterises and blends.

use std::collections::HashMap;

use cosmic_text::fontdb;
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};

use crate::ui::Rect;

/// The font we prefer, so startup reads a file instead of scanning every installed font.
/// `CASEMENT_FONT` names another file and is tried first.
const FONT: &str = "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf";

/// Shaped lines kept between frames. Past this the cache starts over, so changing text cannot grow it for ever.
const MAX_LINES: usize = 512;

/// What the fonts have loaded. `Cold` costs a system font scan, so it is built on the first
/// text, never at startup.
pub enum Fonts {
    Cold,
    Ready { system: FontSystem, glyphs: SwashCache, lines: HashMap<(String, u32), Buffer> },
}

/// A font system holding only the preferred font file, with no scan of the installed fonts.
/// When no named file loads, it scans the installed fonts once so text still draws.
fn font_system() -> FontSystem {
    let mut db = fontdb::Database::new();
    let named = std::env::var("CASEMENT_FONT").ok();
    let loaded = named.iter().map(String::as_str).chain([FONT]).any(|path| db.load_font_file(path).is_ok());
    if !loaded {
        eprintln!("casement: font file {FONT} not readable; using the installed fonts");
        db.load_system_fonts();
    }
    FontSystem::new_with_locale_and_db("en-US".into(), db)
}

impl Fonts {
    fn ready(&mut self) -> (&mut FontSystem, &mut SwashCache, &mut HashMap<(String, u32), Buffer>) {
        if let Fonts::Cold = self {
            *self = Fonts::Ready { system: font_system(), glyphs: SwashCache::new(), lines: HashMap::new() };
        }
        match self {
            Fonts::Ready { system, glyphs, lines } => (system, glyphs, lines),
            Fonts::Cold => unreachable!("just made ready"),
        }
    }
}

/// Source-over blend of `c` (straight alpha) onto one BGRA pixel.
fn blend(px: &mut [u8], c: Color) {
    let a = c.a() as u32;
    for (dst, src) in px[..3].iter_mut().zip([c.b(), c.g(), c.r()]) {
        *dst = ((src as u32 * a + *dst as u32 * (255 - a)) / 255) as u8;
    }
    px[3] = 255;
}

/// Draw `text` at `px` pixels high, clipped to `bounds`, in `argb` (0xAARRGGBB).
pub fn draw_text(fonts: &mut Fonts, frame: &mut [u8], size: (u32, u32), bounds: Rect, text: &str, px: u32, argb: u32) {
    let (system, glyphs, lines) = fonts.ready();
    if lines.len() >= MAX_LINES {
        lines.clear();
    }
    let line = lines.entry((text.to_string(), px)).or_insert_with(|| {
        let mut b = Buffer::new(system, Metrics::new(px as f32, px as f32 * 1.25));
        b.set_size(Some(bounds.w.max(0) as f32), Some(bounds.h.max(0) as f32));
        b.set_text(text, &Attrs::new().family(Family::Name("Adwaita Sans")), Shaping::Advanced, None);
        b
    });
    line.set_size(Some(bounds.w.max(0) as f32), Some(bounds.h.max(0) as f32));
    let (fw, fh) = (size.0 as i32, size.1 as i32);
    let ink = Color::rgba((argb >> 16) as u8, (argb >> 8) as u8, argb as u8, (argb >> 24) as u8);
    line.draw(system, glyphs, ink, |x, y, w, h, c| {
        for dy in 0..h as i32 {
            for dx in 0..w as i32 {
                let (fx, fy) = (bounds.x + x + dx, bounds.y + y + dy);
                let inside = fx >= bounds.x && fx < bounds.x + bounds.w && fy >= bounds.y && fy < bounds.y + bounds.h;
                if inside && fx >= 0 && fx < fw && fy >= 0 && fy < fh {
                    let i = (fy * fw + fx) as usize * 4;
                    blend(&mut frame[i..i + 4], c);
                }
            }
        }
    });
}

/// How wide `text` is at `px` pixels, as it will be drawn (for placing a caret after it).
pub fn text_width(fonts: &mut Fonts, text: &str, px: u32) -> i32 {
    let (system, _, lines) = fonts.ready();
    if lines.len() >= MAX_LINES {
        lines.clear();
    }
    let line = lines.entry((text.to_string(), px)).or_insert_with(|| {
        let mut b = Buffer::new(system, Metrics::new(px as f32, px as f32 * 1.25));
        b.set_size(Some(10_000.0), Some(px as f32 * 2.0));
        b.set_text(text, &Attrs::new().family(Family::Name("Adwaita Sans")), Shaping::Advanced, None);
        b
    });
    line.layout_runs().map(|run| run.line_w).fold(0.0, f32::max).ceil() as i32
}
