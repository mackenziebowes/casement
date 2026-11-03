//! An IntUI tree laid out as casement nodes. IntUI trees carry no sizes, so the layout is made
//! here, for a width: columns stack, rows share the width, text wraps inside its label.
//!
//! All eleven kinds are drawn natively. A kind this build has never heard of (`Drawn::Unknown`) is
//! named in a muted line, so a newer tree is never blank.

use crate::ui::{BarMark, Dot, Line, Shape, Node, Rect, Role, Tone};
use intui_layout::{build_model, format_value, nice_step, Category, Scale};
use intui_types::forms::{self, GroupDirection};
use intui_types::{Author, Drawn, Node as Kind};

const MARGIN: i32 = 16;
const GAP: i32 = 8;
const LINE: i32 = 20;
const BUTTON: i32 = 40;
/// How wide one character of 14 px text is, on average: only for guessing how many lines a text needs.
const CHAR_W: i32 = 7;

fn tone_of(t: forms::Tone) -> Tone {
    match t {
        forms::Tone::Neutral => Tone::Neutral,
        forms::Tone::Positive => Tone::Positive,
        forms::Tone::Warning => Tone::Warning,
        forms::Tone::Critical => Tone::Critical,
        forms::Tone::Muted => Tone::Muted,
    }
}

/// What people have done to the tree that the layout must show: Choice options pressed, and what each
/// field holds (by field node id, `form.field`).
#[derive(Clone, Copy)]
pub struct Memory<'a> {
    pub pressed: &'a [Pressed],
    pub values: &'a [(String, String)],
    /// Where the caret is in each field, in characters, by field node id.
    pub carets: &'a [(String, usize)],
}

impl Memory<'_> {
    pub fn value(&self, id: &str) -> &str {
        self.values.iter().find(|(i, _)| i == id).map_or("", |(_, v)| v)
    }
}

/// A Choice option a person pressed. A `once` Choice with one is used up.
#[derive(Clone, Debug, PartialEq)]
pub struct Pressed {
    pub choice: String,
    pub option: String,
}

/// Parse an IntUI tree from JSON text and lay it out for a surface `width` logical pixels wide.
pub fn scene_of(json: &str, width: i32) -> Result<Node, String> {
    let tree = intui_types::parse(json, Author::Unchecked).map_err(|refusal| refusal.to_string())?;
    Ok(layout(tree.root(), width, 0, Memory { pressed: &[], values: &[], carets: &[] }))
}

/// The laid-out tree for a parsed one, as a panel that fills the surface (`height`) or, when the
/// content is taller, is as tall as the content.
pub fn layout(root: &Drawn, width: i32, height: i32, used: Memory) -> Node {
    let mut children = vec![];
    let inner = (width - 2 * MARGIN).max(0);
    let content = lay(root, "root", MARGIN, MARGIN, inner, used, &mut children);
    Node::Panel { id: "intui".into(), bounds: Rect { x: 0, y: 0, w: width, h: height.max(content + 2 * MARGIN) }, tone: Tone::Neutral, children }
}

/// How many lines `text` takes in `width`, by a guess at the glyph width (the painter really wraps).
fn lines_of(text: &str, width: i32) -> i32 {
    let per_line = (width / CHAR_W).max(1) as usize;
    text.split('\n').map(|l| l.chars().count().div_ceil(per_line).max(1) as i32).sum()
}

fn label(id: String, x: i32, y: i32, w: i32, tone: Tone, text: String, out: &mut Vec<Node>) -> i32 {
    let h = lines_of(&text, w) * LINE;
    out.push(Node::Label { id, bounds: Rect { x, y, w, h }, tone, text });
    h
}

/// A backslash before punctuation only stops Markdown reading it as a mark: `usage\_history` is read as `usage_history`.
fn unescape(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some(next)) if next.is_ascii_punctuation() => out.push(chars.next().unwrap_or(c)),
            _ => out.push(c),
        }
    }
    out
}

/// Markdown as the plain words a person reads: no heading marks, emphasis marks or code ticks.
fn plain(markdown: &str) -> Vec<String> {
    markdown
        .split("\n\n")
        .map(|para| {
            para.lines()
                .map(|l| unescape(&l.trim_start_matches('#').trim().replace("**", "").replace(['*', '`'], "")))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|p| !p.is_empty())
        .collect()
}

/// Lay `node` out at (x, y) in `w`; push what it draws into `out`; return its height.
fn lay(node: &Drawn, path: &str, x: i32, y: i32, w: i32, used_by: Memory, out: &mut Vec<Node>) -> i32 {
    let Drawn::Known(known) = node else {
        let Drawn::Unknown { kind, path } = node else { unreachable!() };
        return label(path.clone(), x, y, w, Tone::Muted, format!("{} is a component this build cannot draw", kind.name()), out);
    };
    match known {
        Kind::Label(l) => label(path.to_string(), x, y, w, tone_of(l.tone), l.text.clone(), out),
        Kind::Text(t) => {
            let mut used = 0;
            for (i, para) in plain(&t.markdown).into_iter().enumerate() {
                if i > 0 {
                    used += GAP;
                }
                used += label(format!("{path}.p{i}"), x, y + used, w, Tone::Neutral, para, out);
            }
            used
        }
        Kind::Group(g) => {
            let kids: Vec<&Drawn> = g.children.iter().collect();
            match g.direction {
                GroupDirection::Column => {
                    let mut used = 0;
                    for (i, kid) in kids.iter().enumerate() {
                        if i > 0 {
                            used += GAP;
                        }
                        used += lay(kid, &format!("{path}.children[{i}]"), x, y + used, w, used_by, out);
                    }
                    used
                }
                GroupDirection::Row => {
                    let n = kids.len() as i32;
                    let each = ((w - GAP * (n - 1)) / n).max(0);
                    kids.iter()
                        .enumerate()
                        .map(|(i, kid)| lay(kid, &format!("{path}.children[{i}]"), x + i as i32 * (each + GAP), y, each, used_by, out))
                        .max()
                        .unwrap_or(0)
                }
            }
        }
        Kind::Choice(c) => {
            let mut used = 0;
            if let Some(prompt) = &c.prompt {
                used += label(format!("{path}.prompt"), x, y, w, Tone::Neutral, prompt.clone(), out) + GAP;
            }
            let n = c.options.len() as i32;
            let each = ((w - GAP * (n - 1)) / n).max(0);
            for (i, option) in c.options.iter().enumerate() {
                let was = used_by.pressed.iter().find(|p| p.choice == c.id);
                // A used-up Choice keeps the pressed option as it was and greys the others out.
                let tone = match was {
                    Some(p) if p.option != option.id && matches!(c.repeat, forms::ChoiceRepeat::Once) => Tone::Muted,
                    _ if matches!(option.emphasis, forms::Emphasis::Primary) => Tone::Positive,
                    _ => Tone::Neutral,
                };
                out.push(Node::Button {
                    id: format!("{}.{}", c.id, option.id),
                    bounds: Rect { x: x + i as i32 * (each + GAP), y: y + used, w: each, h: BUTTON },
                    tone,
                    label: option.label.clone(),
                });
            }
            used + BUTTON
        }
        Kind::Stat(stat) => {
            let mut figure = figure_of(stat.value, stat.unit.as_deref().filter(|u| !u.is_empty()));
            if let Some(change) = &stat.change {
                let unit = matches!(change.as_, forms::StatChangeAs::Percent).then_some("%");
                figure = format!("{figure} ({})", intui_text::reading_text(change.value, unit, true));
            }
            let mut used = label(format!("{path}.label"), x, y, w, Tone::Muted, stat.label.clone(), out);
            used += label(format!("{path}.value"), x, y + used, w, tone_of(stat.tone), figure, out);
            if let Some(note) = stat.note.as_ref().filter(|n| !n.is_empty()) {
                used += label(format!("{path}.note"), x, y + used, w, Tone::Muted, note.clone(), out);
            }
            used
        }
        Kind::Table(t) => {
            let mut used = 0;
            if let Some(caption) = t.caption.as_ref().filter(|c| !c.is_empty()) {
                used += label(format!("{path}.caption"), x, y, w, Tone::Muted, caption.clone(), out) + GAP;
            }
            let n = t.columns.len() as i32;
            let each = (w / n).max(0);
            let row = |cells: Vec<String>, tone: Tone, at: &str, used: &mut i32, out: &mut Vec<Node>| {
                let height = cells.iter().map(|c| lines_of(c, each) * LINE).max().unwrap_or(LINE);
                for (i, cell) in cells.into_iter().enumerate() {
                    out.push(Node::Label { id: format!("{path}.{at}.{i}"), bounds: Rect { x: x + i as i32 * each, y: y + *used, w: each, h: height }, tone, text: cell });
                }
                *used += height + GAP / 2;
            };
            row(t.columns.iter().map(|c| c.label.clone()).collect(), Tone::Muted, "head", &mut used, out);
            for (r, tr) in t.rows.iter().enumerate() {
                row(t.columns.iter().map(|c| intui_text::cell_text(tr.cell(&c.key))).collect(), Tone::Neutral, &format!("row{r}"), &mut used, out);
            }
            used - GAP / 2
        }
        Kind::Flow(f) => {
            // Each step is a box holding its label and, under it, its detail; links with words follow.
            let mut used = 0;
            for step in f.steps().iter() {
                let mut inner = vec![];
                let mut h = label(format!("{path}.{}.label", step.id), x + GAP, y + used + GAP, w - 2 * GAP, Tone::Neutral, step.label.clone(), &mut inner);
                if let Some(detail) = step.detail.as_ref().filter(|d| !d.is_empty()) {
                    h += label(format!("{path}.{}.detail", step.id), x + GAP, y + used + GAP + h, w - 2 * GAP, Tone::Muted, detail.clone(), &mut inner);
                }
                let h = h + 2 * GAP;
                out.push(Node::Panel { id: format!("{path}.{}", step.id), bounds: Rect { x, y: y + used, w, h }, tone: Tone::Muted, children: inner });
                used += h + GAP;
            }
            for (i, (from, to, link)) in f.joined().enumerate() {
                if let Some(words) = link.label.as_ref().filter(|l| !l.is_empty()) {
                    used += label(format!("{path}.link{i}"), x, y + used, w, Tone::Muted, format!("{} to {}: {words}", from.label, to.label), out) + GAP;
                }
            }
            used - GAP
        }
        Kind::Form(form) => {
            let mut used = 0;
            for field in form.fields.iter() {
                let fid = format!("{path}.{}", field.id);
                used += label(format!("{fid}.label"), x, y + used, w, Tone::Neutral, field.label.clone(), out);
                if field.required {
                    used += label(format!("{fid}.required"), x, y + used, w, Tone::Muted, "required".into(), out);
                }
                let id = format!("{}.{}", form.id, field.id);
                let held = used_by.value(&id);
                let text = match field.input {
                    forms::FieldInput::Checkbox(_) => if held == "true" { "[x]".to_string() } else { "[ ]".to_string() },
                    forms::FieldInput::Select(_) => if held.is_empty() { "None".to_string() } else { held.to_string() },
                    _ => held.to_string(),
                };
                let caret = used_by.carets.iter().find(|(i, _)| *i == id).map_or_else(|| text.chars().count(), |(_, c)| (*c).min(text.chars().count()));
                out.push(Node::Input { id, bounds: Rect { x, y: y + used, w, h: BUTTON - 8 }, text, caret });
                used += BUTTON - 8 + GAP;
            }
            out.push(Node::Button { id: format!("{}.submit", form.id), bounds: Rect { x, y: y + used, w: w.min(240), h: BUTTON }, tone: Tone::Positive, label: form.submit_label.clone() });
            used + BUTTON
        }
        Kind::Chart(c) => chart(c, path, x, y, w, out),
        Kind::Marks(m) => marks(m, path, x, y, w, out),
        Kind::Ladder(l) => ladder(l, path, x, y, w, out),
    }
}

/// The IntUI event for pressing the button the layout drew for a Choice option: the wire form of
/// a press (`{"v":1,"source","target","action":"press","value"}`), checked by IntUI's own parser.
/// None when `node_id` is not a Choice option.
pub fn press_event(root: &Drawn, node_id: &str, source: &str, used_by: &[Pressed]) -> Option<String> {
    let Drawn::Known(known) = root else { return None };
    if let Kind::Choice(c) = known {
        let option = c.options.iter().find(|o| format!("{}.{}", c.id, o.id) == node_id)?;
        if matches!(c.repeat, forms::ChoiceRepeat::Once) && used_by.iter().any(|p| p.choice == c.id) {
            return None;
        }
        let event = serde_json::json!({"v": 1, "source": source, "target": c.id, "action": "press", "value": option.id}).to_string();
        return intui_types::parse_event(&event).is_ok().then_some(event);
    }
    known.children().into_iter().find_map(|child| press_event(child, node_id, source, used_by))
}

const PLOT_H: i32 = 180;
const AXIS_W: i32 = 44;

/// Y tick values as the web chart picks them: round numbers inside the domain, at least 32 px apart.
fn y_ticks(lo: f64, hi: f64, plot_h: i32) -> Vec<f64> {
    let room = (plot_h / 32).max(1) as f64;
    let step = nice_step((hi - lo) / room);
    let first = (lo / step).ceil() * step;
    let mut ticks = vec![];
    let mut k = 0.0;
    loop {
        let value = ((first + k * step) * 1e9).round() / 1e9;
        if value > hi + step * 1e-9 {
            break ticks;
        }
        ticks.push(value);
        k += 1.0;
    }
}

/// A line chart: title, a plot (grid and one polyline per series), y ticks, x labels, legend, notes.
fn chart(c: &forms::Chart, path: &str, x: i32, y: i32, w: i32, out: &mut Vec<Node>) -> i32 {
    let model = build_model(c);
    let mut used = 0;
    if let Some(title) = c.title.as_ref().filter(|t| !t.is_empty()) {
        used += label(format!("{path}.title"), x, y, w, Tone::Neutral, title.clone(), out) + GAP;
    }
    let (left, right) = (x + AXIS_W, x + w - 8);
    used += LINE + 12;
    let top = y + used;
    let n = model.xs.len().max(1) as i32;
    let x_at = |i: usize| left + ((right - left) * (2 * i as i32 + 1)) / (2 * n);
    if let Scale::Readable(domain) = model.scale {
        let y_at = |v: f64| top + PLOT_H - intui_layout::project(v, domain, PLOT_H as f64).round() as i32;
        let ticks = y_ticks(domain.lo, domain.hi, PLOT_H);
        let mut lines: Vec<Line> = ticks.iter().map(|&t| Line { role: Role::Grid, points: vec![Some((left, y_at(t))), Some((right, y_at(t)))] }).collect();
        let mut bars = vec![];
        let bar_chart = matches!(c.kind, forms::ChartKind::Bar);
        let group = ((right - left) / n).max(1);
        let each = ((group * 7 / 10) / model.series.len().max(1) as i32).max(1);
        for (si, series) in model.series.iter().enumerate() {
            let role = match series.role {
                forms::ChartSeriesRole::Primary => Role::Primary,
                forms::ChartSeriesRole::Baseline => Role::Baseline,
                forms::ChartSeriesRole::Comparison => Role::Comparison,
            };
            if bar_chart {
                for i in 0..model.xs.len() {
                    if let Some(v) = series.value_at(i) {
                        let base = y_at(domain.lo.max(0.0).min(domain.hi));
                        let (a, b) = (y_at(v).min(base), y_at(v).max(base));
                        let left_edge = x_at(i) - (each * model.series.len() as i32) / 2 + si as i32 * each;
                        bars.push(BarMark { role, rect: Rect { x: left_edge, y: a, w: each, h: (b - a).max(1) } });
                    }
                }
                continue;
            }
            lines.push(Line { role, points: (0..model.xs.len()).map(|i| series.value_at(i).map(|v| (x_at(i), y_at(v)))).collect() });
        }
        out.push(Node::Plot { id: format!("{path}.plot"), bounds: Rect { x: left, y: top, w: right - left, h: PLOT_H }, lines, bars, dots: vec![] });
        for (i, &t) in ticks.iter().enumerate() {
            out.push(Node::Label { id: format!("{path}.tick{i}"), bounds: Rect { x, y: y_at(t) - LINE / 2, w: AXIS_W - 6, h: LINE }, tone: Tone::Muted, text: format_value(t) });
        }
        if let Some(unit) = c.unit.as_ref().filter(|u| !u.is_empty()) {
            out.push(Node::Label { id: format!("{path}.unit"), bounds: Rect { x, y: top - LINE - 12, w: AXIS_W + 120, h: LINE }, tone: Tone::Muted, text: unit.clone() });
        }
    }
    // Each annotation that matches an x is numbered on the plot; the notes below say what it is.
    for m in &model.markers {
        if let intui_layout::MarkerSite::OnCategory(i) = m.site {
            out.push(Node::Label { id: format!("{path}.mark{}", m.n), bounds: Rect { x: x_at(i) - 4, y: top - LINE - 10, w: 24, h: LINE }, tone: Tone::Warning, text: m.n.to_string() });
        }
    }
    used += PLOT_H + 4;
    // Which x labels fit, chosen by IntUI's own rule (first and last, then midpoints while there is room),
    // with the web's width guess of 7 px a character so both surfaces keep the same ones.
    let texts: Vec<String> = model
        .xs
        .iter()
        .map(|xv| match xv {
            Category::Text(t) => t.clone(),
            Category::Number(v) => intui_text::numbers::js_to_string(*v),
        })
        .collect();
    let widths: Vec<f64> = texts.iter().map(|t| (t.chars().count() * 7) as f64).collect();
    let centers: Vec<f64> = (0..model.xs.len()).map(|i| (x_at(i) - x) as f64).collect();
    for placed in intui_layout::place_labels(&centers, &widths, w as f64, 10.0) {
        let text = texts[placed.index].clone();
        let width = widths[placed.index] as i32 + 16;
        out.push(Node::Label { id: format!("{path}.x{}", placed.index), bounds: Rect { x: x + placed.start as i32, y: y + used, w: width, h: LINE }, tone: Tone::Muted, text });
    }
    used += LINE + GAP;
    for (i, s) in model.series.iter().enumerate() {
        let text = if matches!(s.role, forms::ChartSeriesRole::Primary) { s.label.clone() } else { format!("{} ({})", s.label, s.role) };
        used += label(format!("{path}.legend{i}"), x, y + used, w, Tone::Neutral, text, out);
    }
    for m in &model.markers {
        let at = match &m.at {
            Category::Text(t) => t.clone(),
            Category::Number(v) => format_value(*v),
        };
        used += label(format!("{path}.note{}", m.n), x, y + used, w, Tone::Muted, format!("{at} {}", m.label), out);
    }
    used
}

/// The form and field a node id belongs to, if it is a field the layout drew: `form.field`.
fn field_of<'a>(root: &'a Drawn, node_id: &str) -> Option<(&'a forms::Form, &'a forms::Field)> {
    let Drawn::Known(known) = root else { return None };
    if let Kind::Form(form) = known {
        return form.fields.iter().find(|f| format!("{}.{}", form.id, f.id) == node_id).map(|f| (form, f));
    }
    known.children().into_iter().find_map(|child| field_of(child, node_id))
}

/// The values a field steps through when pressed, if it is one a press changes: a checkbox flips, a
/// select moves to its next option (an optional one passes through "None", which holds nothing).
pub fn cycle_of(root: &Drawn, node_id: &str) -> Option<Vec<String>> {
    let (_, f) = field_of(root, node_id)?;
    match &f.input {
        forms::FieldInput::Checkbox(_) => Some(vec!["false".into(), "true".into()]),
        forms::FieldInput::Select(options) => {
            let mut all: Vec<String> = if f.required { vec![] } else { vec![String::new()] };
            all.extend(options.iter().cloned());
            Some(all)
        }
        _ => None,
    }
}

fn forms_in<'a>(root: &'a Drawn, out: &mut Vec<&'a forms::Form>) {
    let Drawn::Known(known) = root else { return };
    if let Kind::Form(form) = known {
        out.push(form);
    }
    known.children().into_iter().for_each(|child| forms_in(child, out));
}

/// The submit event for pressing a form's submit button: the wire form (`action: submit`, one value
/// per field, a string, a number or a flag), checked by IntUI's parser. `None` when `node_id` is not a
/// submit button; `Some(Err(words))` when a required field is empty or a number field is not a number.
pub fn submit_event(root: &Drawn, node_id: &str, source: &str, values: &[(String, String)]) -> Option<Result<String, String>> {
    let mut all = vec![];
    forms_in(root, &mut all);
    let form = all.into_iter().find(|f| format!("{}.submit", f.id) == node_id)?;
    let mut value = serde_json::Map::new();
    for f in form.fields.iter() {
        let held = values.iter().find(|(i, _)| *i == format!("{}.{}", form.id, f.id)).map_or("", |(_, v)| v.as_str());
        let json = match f.input {
            forms::FieldInput::Checkbox(_) => serde_json::json!(held == "true"),
            forms::FieldInput::Number(_) if held.is_empty() => serde_json::Value::Null,
            forms::FieldInput::Number(_) => match held.parse::<f64>() {
                Ok(n) if n.is_finite() => serde_json::json!(n),
                _ => return Some(Err(format!("{} must be a number; it holds '{held}'", f.label))),
            },
            _ => serde_json::json!(held),
        };
        if f.required && (json.is_null() || json == serde_json::json!("")) {
            return Some(Err(format!("{} is required; type something in it first", f.label)));
        }
        if !json.is_null() && json != serde_json::json!("") {
            value.insert(f.id.clone(), json);
        }
    }
    let event = serde_json::json!({"v": 1, "source": source, "target": form.id, "action": "submit", "value": value}).to_string();
    Some(intui_types::parse_event(&event).map(|_| event).map_err(|refusal| refusal.to_string()))
}

/// A reading as the eye gets it: a mark before the number (a floor and an estimate say so), not the
/// words the text form uses; an unknown is a dash and the word.
fn figure_of(reading: intui_types::Reading, unit: Option<&str>) -> String {
    use intui_types::ReadingKind;
    let words = intui_text::reading_text(reading, unit, false);
    match reading.kind() {
        ReadingKind::Exact => words,
        ReadingKind::Unknown => format!("\u{2014} {words}"),
        kind => {
            let number = words.strip_prefix("at least ").or_else(|| words.strip_prefix("about ")).unwrap_or(&words);
            format!("{}{number}", intui_layout::reading_mark(kind))
        }
    }
}

const DOT_R: i32 = 6;
const DOT_STEP: i32 = 18;

fn shape_of(kind: usize) -> Shape {
    [Shape::Filled, Shape::Hollow, Shape::Ring][kind.min(2)]
}

/// The key: one sample mark and its name for each kind (shape) and each state (colour).
fn legend(kinds: &[forms::MarkKind], states: &[forms::MarkState], path: &str, x: i32, y: i32, w: i32, dots: &mut Vec<Dot>, out: &mut Vec<Node>) -> i32 {
    let mut at = x;
    let mut line_y = y;
    let mut put = |dot: Dot, text: &str, n: usize, dots: &mut Vec<Dot>, out: &mut Vec<Node>| {
        let width = text.chars().count() as i32 * (CHAR_W + 1) + 12;
        if at + DOT_R * 2 + 6 + width > x + w && at > x {
            at = x;
            line_y += LINE + 4;
        }
        dots.push(Dot { x: at + DOT_R, y: line_y + LINE / 2, ..dot });
        out.push(Node::Label { id: format!("{path}.key{n}"), bounds: Rect { x: at + DOT_R * 2 + 6, y: line_y, w: width, h: LINE }, tone: Tone::Neutral, text: text.to_string() });
        at += DOT_R * 2 + 6 + width + 8;
    };
    let mut n = 0;
    for (i, k) in kinds.iter().enumerate() {
        put(Dot { shape: shape_of(i), tone: Tone::Neutral, x: 0, y: 0, r: DOT_R }, &k.label, n, dots, out);
        n += 1;
    }
    for st in states {
        put(Dot { shape: Shape::Filled, tone: tone_of(st.tone), x: 0, y: 0, r: DOT_R }, &st.label, n, dots, out);
        n += 1;
    }
    line_y + LINE - y
}

/// A row of marks wrapped to the width; returns its height.
fn mark_dots(list: &[forms::Mark], states: &[forms::MarkState], x: i32, y: i32, w: i32, dots: &mut Vec<Dot>) -> i32 {
    let per_line = ((w / DOT_STEP).max(1)) as usize;
    for (i, m) in list.iter().enumerate() {
        let tone = m.state.map_or(Tone::Neutral, |s| states.get(s.get()).map_or(Tone::Neutral, |st| tone_of(st.tone)));
        dots.push(Dot { shape: shape_of(m.kind.get()), tone, x: x + DOT_R + (i % per_line) as i32 * DOT_STEP, y: y + LINE / 2 + (i / per_line) as i32 * LINE, r: DOT_R });
    }
    (list.len().div_ceil(per_line).max(1)) as i32 * LINE
}

/// What a mark's detail says, with its state: "replied: asked about price".
fn detail_lines(list: &[forms::Mark], states: &[forms::MarkState]) -> Vec<String> {
    list.iter()
        .filter_map(|m| {
            let d = m.detail.as_ref().filter(|d| !d.is_empty())?;
            Some(match m.state.and_then(|s| states.get(s.get())) {
                Some(st) => format!("{}: {d}", st.label),
                None => d.clone(),
            })
        })
        .collect()
}

fn marks(m: &forms::Marks, path: &str, x: i32, y: i32, w: i32, out: &mut Vec<Node>) -> i32 {
    let mut dots = vec![];
    let mut used = legend(&m.kinds, &m.states, path, x, y, w, &mut dots, out) + GAP;
    for (r, row) in m.rows.iter().enumerate() {
        used += label(format!("{path}.row{r}.label"), x, y + used, w, Tone::Neutral, row.label.clone(), out);
        if let Some(note) = row.note.as_ref().filter(|n| !n.is_empty()) {
            used += label(format!("{path}.row{r}.note"), x, y + used, w, Tone::Muted, note.clone(), out);
        }
        match &row.body {
            forms::RowBody::Marks(list) => {
                used += mark_dots(list, &m.states, x, y + used, w, &mut dots);
                for (d, text) in detail_lines(list, &m.states).into_iter().enumerate() {
                    used += label(format!("{path}.row{r}.detail{d}"), x, y + used, w, Tone::Muted, text, out);
                }
            }
            forms::RowBody::Unknown(why) => used += label(format!("{path}.row{r}.unknown"), x, y + used, w, Tone::Muted, why.clone(), out),
        }
        if let Some(halo) = &row.halo {
            let text = match halo.detail.as_ref().filter(|d| !d.is_empty()) {
                Some(d) => format!("{} {} ({d})", halo.label, halo.n),
                None => format!("{} {}", halo.label, halo.n),
            };
            used += label(format!("{path}.row{r}.halo"), x, y + used, w, Tone::Muted, text, out);
        }
        used += GAP;
    }
    out.push(Node::Plot { id: format!("{path}.marks"), bounds: Rect { x, y, w, h: used }, lines: vec![], bars: vec![], dots });
    used - GAP
}

/// A ladder, read top to bottom (the data lists rungs bottom to top): each rung is a label and its nodes side by side.
fn ladder(l: &forms::Ladder, path: &str, x: i32, y: i32, w: i32, out: &mut Vec<Node>) -> i32 {
    let mut dots = vec![];
    let mut used = legend(&l.kinds, &l.states, path, x, y, w, &mut dots, out) + GAP;
    for rung in l.rungs.iter().rev() {
        used += label(format!("{path}.{}.label", rung.id), x, y + used, w, Tone::Neutral, rung.label.clone(), out);
        if rung.nodes.is_empty() {
            if let Some(empty) = rung.empty.as_ref().filter(|e| !e.is_empty()) {
                used += label(format!("{path}.{}.empty", rung.id), x, y + used, w, Tone::Muted, empty.clone(), out);
            }
            used += GAP;
            continue;
        }
        let n = rung.nodes.len() as i32;
        let each = ((w - GAP * (n - 1)) / n).max(0);
        let mut tallest = 0;
        for (ni, node) in rung.nodes.iter().enumerate() {
            let nx = x + ni as i32 * (each + GAP);
            let mut inner = vec![];
            let mut h = GAP;
            h += label(format!("{path}.{}.label", node.id), nx + GAP, y + used + h, each - 2 * GAP, Tone::Neutral, node.label.clone(), &mut inner);
            if let Some(group) = node.group.as_ref().filter(|g| !g.is_empty()) {
                h += label(format!("{path}.{}.group", node.id), nx + GAP, y + used + h, each - 2 * GAP, Tone::Muted, group.clone(), &mut inner);
            }
            match &node.body {
                forms::NodeBody::Marks(list) => h += mark_dots(list, &l.states, nx + GAP, y + used + h, each - 2 * GAP, &mut dots),
                forms::NodeBody::Said { count, marks } => {
                    h += label(format!("{path}.{}.count", node.id), nx + GAP, y + used + h, each - 2 * GAP, Tone::Neutral, count.clone(), &mut inner);
                    h += mark_dots(marks, &l.states, nx + GAP, y + used + h, each - 2 * GAP, &mut dots);
                }
                forms::NodeBody::Unknown(why) => h += label(format!("{path}.{}.unknown", node.id), nx + GAP, y + used + h, each - 2 * GAP, Tone::Muted, why.clone(), &mut inner),
            }
            h += GAP;
            out.push(Node::Panel { id: format!("{path}.{}", node.id), bounds: Rect { x: nx, y: y + used, w: each, h }, tone: Tone::Muted, children: inner });
            tallest = tallest.max(h);
        }
        used += tallest + GAP;
    }
    for (si, step) in l.steps.iter().enumerate() {
        if let (Some(from), Some(to)) = (l.node(step.from), l.node(step.to)) {
            used += label(format!("{path}.step{si}"), x, y + used, w, Tone::Muted, format!("{} → {}: {}", from.label, to.label, step.label), out);
        }
    }
    out.push(Node::Plot { id: format!("{path}.ladder"), bounds: Rect { x, y, w, h: used }, lines: vec![], bars: vec![], dots });
    used
}
