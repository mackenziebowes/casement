//! Splits the cold first draw: font load, shaping, first rasterise.
use cosmic_text::{fontdb, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use std::time::Instant;
fn main() {
    let t = Instant::now();
    let mut sys = { let mut db = fontdb::Database::new(); db.load_font_file("/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf").unwrap(); FontSystem::new_with_locale_and_db("en-US".into(), db) };
    println!("font system: {:.1} ms ({} faces)", t.elapsed().as_secs_f64() * 1e3, sys.db().len());
    let mut cache = SwashCache::new();
    let t = Instant::now();
    let mut b = Buffer::new(&mut sys, Metrics::new(14.0, 17.5));
    b.set_size(Some(400.0), Some(24.0));
    b.set_text("Recording in progress 00:12", &Attrs::new().family(Family::Name("Adwaita Sans")), Shaping::Advanced, None);
    b.shape_until_scroll(&mut sys, false);
    println!("shape: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    b.draw(&mut sys, &mut cache, Color::rgb(255, 255, 255), |_, _, _, _, _| {});
    println!("first rasterise: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
}
