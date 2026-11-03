//! The inspection socket: what an outside process may ask for, and the PNG it gets back.

/// What an outside process asked the inspection socket for.
#[derive(Debug, PartialEq)]
pub enum Route {
    Png,
    State,
    /// GET /events: the IntUI events people have caused since the last read, one JSON object a line.
    Events,
    Tree,
    /// POST /click/<id>: press the node with this id.
    Click(String),
    /// POST /tree with a JSON body: replace what is on the surface.
    SetTree(String),
    /// POST /intui with an IntUI tree as JSON: casement lays it out and shows it.
    SetIntui(String),
    Unknown,
}

/// Why a request is turned away before it is routed, or None. The socket is for tools on this machine, never for a page in a browser:
/// a page can send a cross-site POST to 127.0.0.1 (its Host is still ours, but a browser adds Origin and Sec-Fetch-Site), and a rebinding page
/// sends its own name as Host. curl and the rig scripts send neither header.
pub fn refused(request: &str) -> Option<&'static str> {
    let head = request.split_once("\r\n\r\n").map(|(h, _)| h).unwrap_or(request).to_lowercase();
    let header = |name: &str| head.lines().skip(1).find_map(|l| l.strip_prefix(name).and_then(|v| v.strip_prefix(':')).map(|v| v.trim().to_string()));
    if header("origin").is_some() || header("sec-fetch-site").is_some() {
        return Some("a browser page may not use this socket");
    }
    match header("host") {
        Some(h) if h.rsplit_once(':').map(|(n, _)| n).unwrap_or(&h) == "127.0.0.1" || h.rsplit_once(':').map(|(n, _)| n).unwrap_or(&h) == "localhost" => None,
        None => None,
        Some(_) => Some("the Host must be 127.0.0.1 or localhost"),
    }
}

/// The first line of an HTTP request decides the route; nothing else in it matters.
pub fn route(request: &str) -> Route {
    let body = request.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    match request.lines().next().unwrap_or("").split_whitespace().collect::<Vec<_>>().as_slice() {
        ["GET", "/frame.png", ..] => Route::Png,
        ["GET", "/state", ..] => Route::State,
        ["GET", "/tree", ..] => Route::Tree,
        ["GET", "/events", ..] => Route::Events,
        ["POST", "/tree", ..] => Route::SetTree(body.to_string()),
        ["POST", "/intui", ..] => Route::SetIntui(body.to_string()),
        ["POST", path, ..] if path.starts_with("/click/") => Route::Click(path["/click/".len()..].to_string()),
        _ => Route::Unknown,
    }
}

/// The frame as the app painted it: BGRA, 4 bytes a pixel, rows top to bottom.
pub fn encode_png(w: u32, h: u32, bgra: &[u8]) -> Result<Vec<u8>, String> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header().map_err(|e| e.to_string())?.write_image_data(&rgba).map_err(|e| e.to_string())?;
    Ok(out)
}


/// Read one HTTP request: the head, then as many body bytes as Content-Length says (at most 1 MiB: a real hub page is 100 KB).
pub fn read_request(stream: &mut impl std::io::Read) -> String {
    const MAX: usize = 1024 * 1024;
    let mut data: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let head_end = data.windows(4).position(|w| w == b"\r\n\r\n");
        if let Some(end) = head_end {
            let head = String::from_utf8_lossy(&data[..end]).to_lowercase();
            let want = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0).min(MAX);
            if data.len() >= end + 4 + want {
                break;
            }
        }
        if data.len() >= MAX {
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
        }
    }
    String::from_utf8_lossy(&data).to_string()
}
