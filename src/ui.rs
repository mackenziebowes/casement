//! What is on the surface, as nodes an outside process can read, replace and press by id.
//!
//! Colour means something: a node carries a `Tone` (what it says), never a colour (how it
//! looks). The painter maps tone to colour in one place.

use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// What a node means. The words are IntUI's tones (packages/core/src/components/label.ts).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Neutral,
    /// Fine, done, safe to go on.
    Positive,
    /// Happening now, or needs a look.
    Warning,
    /// Failed or wrong.
    Critical,
    /// Not started, or not part of the story yet.
    Muted,
}

impl Tone {
    pub fn word(self) -> &'static str {
        match self {
            Tone::Neutral => "neutral",
            Tone::Positive => "positive",
            Tone::Warning => "warning",
            Tone::Critical => "critical",
            Tone::Muted => "muted",
        }
    }
    fn parse(s: &str) -> Result<Tone, String> {
        Ok(match s {
            "neutral" => Tone::Neutral,
            "positive" => Tone::Positive,
            "warning" => Tone::Warning,
            "critical" => Tone::Critical,
            "muted" => Tone::Muted,
            other => return Err(format!("unknown tone '{other}' (neutral, positive, warning, critical, muted)")),
        })
    }
}

/// A number from 0 to 1 and how sure it is (IntUI's Reading). Each kind holds only its own
/// fields: an `Unknown` holds no number, so it can never pass for 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reading {
    Exact(f32),
    AtLeast(f32),
    Estimate(f32),
    Unknown,
}

impl Reading {
    /// The number to draw, if there is one.
    pub fn level(self) -> Option<f32> {
        match self {
            Reading::Exact(v) | Reading::AtLeast(v) | Reading::Estimate(v) => Some(v),
            Reading::Unknown => None,
        }
    }

    /// IntUI's wire form: an exact reading is a bare number, the others say their kind.
    pub fn to_json(self) -> Value {
        match self {
            Reading::Exact(v) => json!(v),
            Reading::AtLeast(v) => json!({"kind": "atLeast", "value": v}),
            Reading::Estimate(v) => json!({"kind": "estimate", "value": v}),
            Reading::Unknown => json!({"kind": "unknown"}),
        }
    }

    fn from_json(id: &str, v: &Value) -> Result<Reading, String> {
        let in_range = |n: f64| if (0.0..=1.0).contains(&n) { Ok(n as f32) } else { Err(format!("bar {id}: reading value {n} is outside 0 to 1")) };
        if let Some(n) = v.as_f64() {
            return Ok(Reading::Exact(in_range(n)?));
        }
        let kind = v["kind"].as_str().ok_or_else(|| format!("bar {id}: 'reading' must be a number or {{\"kind\": atLeast|estimate|unknown, ...}}"))?;
        let number = || -> Result<f32, String> {
            in_range(v["value"].as_f64().ok_or_else(|| format!("bar {id}: a '{kind}' reading needs a 'value' number from 0 to 1"))?)
        };
        match kind {
            "atLeast" => Ok(Reading::AtLeast(number()?)),
            "estimate" => Ok(Reading::Estimate(number()?)),
            "unknown" if v.get("value").is_some() => Err(format!("bar {id}: an 'unknown' reading holds no value; send the bare number, or drop the 'value'")),
            "unknown" => Ok(Reading::Unknown),
            other => Err(format!("bar {id}: unknown reading kind '{other}' (atLeast, estimate, unknown; a bare number is exact)")),
        }
    }
}

/// What a line in a plot is for. The painter decides how each looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    /// A guide the eye reads values against.
    Grid,
    /// What the chart is about.
    Primary,
    /// What the primary is compared with.
    Baseline,
    Comparison,
}

impl Role {
    pub fn word(self) -> &'static str {
        match self {
            Role::Grid => "grid",
            Role::Primary => "primary",
            Role::Baseline => "baseline",
            Role::Comparison => "comparison",
        }
    }
}

/// One polyline in a plot, in surface pixels. A gap (no point at that x) breaks the line.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub role: Role,
    pub points: Vec<Option<(i32, i32)>>,
}

/// How a kind of mark is drawn, so kinds stay apart without colour (colour is the state's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Filled,
    Hollow,
    Ring,
}

impl Shape {
    pub fn word(self) -> &'static str {
        match self {
            Shape::Filled => "filled",
            Shape::Hollow => "hollow",
            Shape::Ring => "ring",
        }
    }
}

/// One mark in a plot: a circle of radius `r` centred at (x, y).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    pub shape: Shape,
    pub tone: Tone,
    pub x: i32,
    pub y: i32,
    pub r: i32,
}

/// One bar in a plot, in surface pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarMark {
    pub role: Role,
    pub rect: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Panel { id: String, bounds: Rect, tone: Tone, children: Vec<Node> },
    Label { id: String, bounds: Rect, tone: Tone, text: String },
    /// A drawn form with its own colour, for a character or a sticker: not a tone, so it carries no meaning to a reader of
    /// the tree beyond its id. Anti-aliased.
    Blob { id: String, bounds: Rect, form: Form, fill: u32 },
    /// A press target with no look of its own: it sits over a `Blob` and `Words` an app drew, so a rounded button is still a button.
    Hotspot { id: String, bounds: Rect },
    /// Words in a colour and size the app chose, for text on a `Blob`.
    Words { id: String, bounds: Rect, px: u32, ink: u32, text: String },
    /// Words to read from across a room: a label drawn large, wrapped to its bounds.
    Headline { id: String, bounds: Rect, tone: Tone, text: String },
    Button { id: String, bounds: Rect, tone: Tone, label: String },
    /// A level or progress bar; its reading is 0.0 to 1.0, or unknown.
    Bar { id: String, bounds: Rect, tone: Tone, reading: Reading },
    /// Lines and dots inside `bounds`; its words (ticks, legend) are separate labels.
    Plot { id: String, bounds: Rect, lines: Vec<Line>, bars: Vec<BarMark>, dots: Vec<Dot> },
    /// A field a person types in (or, for a checkbox, presses); `text` is what it holds now.
    Input { id: String, bounds: Rect, text: String, caret: usize },
}

impl Node {
    pub fn id(&self) -> &str {
        match self {
            Node::Panel { id, .. } | Node::Label { id, .. } | Node::Headline { id, .. } | Node::Blob { id, .. } | Node::Words { id, .. } | Node::Hotspot { id, .. } | Node::Button { id, .. } | Node::Bar { id, .. } | Node::Plot { id, .. } | Node::Input { id, .. } => id,
        }
    }

    /// The same tree moved `dy` pixels down (negative: up). A scrolled view is the tree moved.
    pub fn shifted(&self, dy: i32) -> Node {
        let down = |r: &Rect| Rect { y: r.y + dy, ..*r };
        match self {
            Node::Panel { id, bounds, tone, children } => Node::Panel { id: id.clone(), bounds: down(bounds), tone: *tone, children: children.iter().map(|c| c.shifted(dy)).collect() },
            Node::Label { id, bounds, tone, text } => Node::Label { id: id.clone(), bounds: down(bounds), tone: *tone, text: text.clone() },
            Node::Headline { id, bounds, tone, text } => Node::Headline { id: id.clone(), bounds: down(bounds), tone: *tone, text: text.clone() },
            Node::Hotspot { id, bounds } => Node::Hotspot { id: id.clone(), bounds: down(bounds) },
            Node::Blob { id, bounds, form, fill } => Node::Blob { id: id.clone(), bounds: down(bounds), form: *form, fill: *fill },
            Node::Words { id, bounds, px, ink, text } => Node::Words { id: id.clone(), bounds: down(bounds), px: *px, ink: *ink, text: text.clone() },
            Node::Button { id, bounds, tone, label } => Node::Button { id: id.clone(), bounds: down(bounds), tone: *tone, label: label.clone() },
            Node::Bar { id, bounds, tone, reading } => Node::Bar { id: id.clone(), bounds: down(bounds), tone: *tone, reading: *reading },
            Node::Input { id, bounds, text, caret } => Node::Input { id: id.clone(), bounds: down(bounds), text: text.clone(), caret: *caret },
            Node::Plot { id, bounds, lines, bars, dots } => Node::Plot {
                id: id.clone(),
                bounds: down(bounds),
                lines: lines.iter().map(|l| Line { role: l.role, points: l.points.iter().map(|p| p.map(|(x, y)| (x, y + dy))).collect() }).collect(),
                bars: bars.iter().map(|m| BarMark { role: m.role, rect: down(&m.rect) }).collect(),
                dots: dots.iter().map(|d| Dot { y: d.y + dy, ..*d }).collect(),
            },
        }
    }

    pub fn to_json(&self) -> Value {
        let b = |r: &Rect| json!({"x": r.x, "y": r.y, "w": r.w, "h": r.h});
        match self {
            Node::Panel { id, bounds, tone, children } => json!({"id": id, "kind": "panel", "tone": tone.word(), "bounds": b(bounds), "children": children.iter().map(Node::to_json).collect::<Vec<_>>()}),
            Node::Label { id, bounds, tone, text } => json!({"id": id, "kind": "label", "tone": tone.word(), "bounds": b(bounds), "text": text}),
            Node::Headline { id, bounds, tone, text } => json!({"id": id, "kind": "headline", "tone": tone.word(), "bounds": b(bounds), "text": text}),
            Node::Hotspot { id, bounds } => json!({"id": id, "kind": "hotspot", "tone": "neutral", "bounds": b(bounds)}),
            Node::Blob { id, bounds, form, fill } => json!({"id": id, "kind": "blob", "tone": "neutral", "bounds": b(bounds), "form": form.to_json(), "fill": format!("#{fill:08X}")}),
            Node::Words { id, bounds, px, ink, text } => json!({"id": id, "kind": "words", "tone": "neutral", "bounds": b(bounds), "px": px, "ink": format!("#{ink:08X}"), "text": text}),
            Node::Button { id, bounds, tone, label } => json!({"id": id, "kind": "button", "tone": tone.word(), "bounds": b(bounds), "text": label}),
            Node::Input { id, bounds, text, caret } => json!({"id": id, "kind": "input", "bounds": b(bounds), "text": text, "caret": caret}),
            Node::Plot { id, bounds, lines, bars, dots } => json!({"id": id, "kind": "plot", "bounds": b(bounds), "dots": dots.iter().map(|d| json!({"shape": d.shape.word(), "tone": d.tone.word(), "x": d.x, "y": d.y, "r": d.r})).collect::<Vec<_>>(), "bars": bars.iter().map(|m| json!({"role": m.role.word(), "rect": b(&m.rect)})).collect::<Vec<_>>(), "lines": lines.iter().map(|l| json!({"role": l.role.word(), "points": l.points.iter().map(|p| p.map_or(Value::Null, |(x, y)| json!([x, y]))).collect::<Vec<_>>()})).collect::<Vec<_>>()}),
            Node::Bar { id, bounds, tone, reading } => json!({"id": id, "kind": "bar", "tone": tone.word(), "bounds": b(bounds), "reading": reading.to_json()}),
        }
    }

    /// Read a node tree an app posted. Every field the kind needs must be there; nothing is guessed.
    pub fn from_json(v: &Value) -> Result<Node, String> {
        let text = |k: &str| v[k].as_str().map(str::to_string).ok_or_else(|| format!("node {}: '{k}' must be a string", v["id"]));
        let id = text("id")?;
        let tone = Tone::parse(v["tone"].as_str().ok_or_else(|| format!("node {id}: 'tone' must be a string"))?)?;
        let num = |k: &str| v["bounds"][k].as_i64().map(|n| n as i32).ok_or_else(|| format!("node {id}: bounds.{k} must be a number"));
        let bounds = Rect { x: num("x")?, y: num("y")?, w: num("w")?, h: num("h")? };
        match v["kind"].as_str() {
            Some("panel") => {
                let children = v["children"].as_array().ok_or_else(|| format!("panel {id}: 'children' must be a list"))?;
                Ok(Node::Panel { id, bounds, tone, children: children.iter().map(Node::from_json).collect::<Result<_, _>>()? })
            }
            Some("label") => Ok(Node::Label { id, bounds, tone, text: text("text")? }),
            Some("hotspot") => Ok(Node::Hotspot { id, bounds }),
            Some("blob") => Ok(Node::Blob { id, bounds, form: Form::from_json(&v["form"], &v["id"])?, fill: colour_from(&v["fill"], &v["id"])? }),
            Some("words") => Ok(Node::Words { id, bounds, px: v["px"].as_u64().filter(|n| (6..=200).contains(n)).ok_or_else(|| format!("words {}: 'px' must be a number from 6 to 200", v["id"]))? as u32, ink: colour_from(&v["ink"], &v["id"])?, text: text("text")? }),
            Some("headline") => Ok(Node::Headline { id, bounds, tone, text: text("text")? }),
            Some("button") => Ok(Node::Button { id, bounds, tone, label: text("text")? }),
            Some("bar") => Ok(Node::Bar { id: id.clone(), bounds, tone, reading: Reading::from_json(&id, &v["reading"])? }),
            Some("input") => Ok(Node::Input { id, bounds, caret: v["caret"].as_u64().unwrap_or(0) as usize, text: text("text")? }),
            Some("plot") => Err(format!("plot {id}: a plot is made by the IntUI layout (POST /intui), not posted as a node")),
            other => Err(format!("node {id}: unknown kind {other:?} (panel, label, button, bar)")),
        }
    }

    fn find(&self, want: &str) -> Option<&Node> {
        if self.id() == want {
            return Some(self);
        }
        match self {
            Node::Panel { children, .. } => children.iter().find_map(|c| c.find(want)),
            _ => None,
        }
    }
}

/// The nodes shown before any app has posted its own: a button and a status line.
pub fn demo_tree(w: u32, h: u32, rev: usize) -> Node {
    let (w, h) = (w as i32, h as i32);
    Node::Panel {
        id: "root".into(),
        bounds: Rect { x: 0, y: 0, w, h },
        tone: if rev % 2 == 0 { Tone::Warning } else { Tone::Neutral },
        children: vec![
            Node::Button { id: "flip".into(), bounds: Rect { x: 16, y: 16, w: (w - 32).max(0), h: 48 }, tone: Tone::Neutral, label: "Flip colour".into() },
            Node::Label { id: "status".into(), bounds: Rect { x: 16, y: 80, w: (w - 32).max(0), h: 24 }, tone: Tone::Neutral, text: format!("revision {rev}") },
        ],
    }
}

#[derive(Debug, PartialEq)]
pub enum Click {
    /// The node is a button: its press happened.
    Pressed,
    NotClickable,
    Unknown,
}

pub fn click(root: &Node, id: &str) -> Click {
    match root.find(id) {
        Some(Node::Button { .. } | Node::Hotspot { .. }) => Click::Pressed,
        Some(_) => Click::NotClickable,
        None => Click::Unknown,
    }
}

/// How a `Blob` is shaped inside its bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Form {
    /// A rectangle with rounded corners; a radius of half the short side makes a pill.
    Rounded { radius: i32 },
    /// The ellipse that fills the bounds (a circle when they are square).
    Disc,
    /// A stroke along the ellipse inside the bounds from `from` to `to` degrees (0 = right, 90 = down), `width` thick, round ends.
    Arc { width: i32, from: i32, to: i32 },
}

impl Form {
    fn to_json(self) -> Value {
        match self {
            Form::Rounded { radius } => json!({"kind": "rounded", "radius": radius}),
            Form::Disc => json!({"kind": "disc"}),
            Form::Arc { width, from, to } => json!({"kind": "arc", "width": width, "from": from, "to": to}),
        }
    }

    fn from_json(v: &Value, id: &Value) -> Result<Form, String> {
        let n = |k: &str| v[k].as_i64().map(|n| n as i32).ok_or_else(|| format!("blob {id}: form.{k} must be a number"));
        match v["kind"].as_str() {
            Some("rounded") => Ok(Form::Rounded { radius: n("radius")? }),
            Some("disc") => Ok(Form::Disc),
            Some("arc") => Ok(Form::Arc { width: n("width")?, from: n("from")?, to: n("to")? }),
            _ => Err(format!("blob {id}: form.kind must be rounded, disc or arc")),
        }
    }
}

/// "#RRGGBB" or "#RRGGBBAA" as 0xAARRGGBB.
fn colour_from(v: &Value, id: &Value) -> Result<u32, String> {
    let bad = || format!("node {id}: a colour must be \"#RRGGBB\" or \"#RRGGBBAA\"");
    let hex = v.as_str().and_then(|s| s.strip_prefix('#')).ok_or_else(bad)?;
    let n = u32::from_str_radix(hex, 16).map_err(|_| bad())?;
    match hex.len() {
        6 => Ok(0xFF00_0000 | n),
        8 => Ok(n << 24 | n >> 8),
        _ => Err(bad()),
    }
}
