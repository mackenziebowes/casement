//! Pointer, keyboard and focus as one state machine. The Wayland handlers turn what the seat
//! said into one `Event` each and call `step`; `step` decides, the handlers do not.
//!
//! Hover, press and focus are held as node ids, apart from the tree. A repost of the tree
//! therefore cannot lose them while the id still exists, and keyboard focus leaving the window
//! (`Keys::Away`) does not forget which node was focused.

use crate::ui::{Node, Rect};

#[derive(Clone, Debug, PartialEq)]
pub enum Pointer {
    Outside,
    Over { id: String },
    /// Button held down on `id`; `over` says whether the pointer is still on it.
    Pressed { id: String, over: bool },
}

/// Whether the compositor has given this window the keyboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Keys {
    Away,
    Here,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Focus {
    Nowhere,
    On { id: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub pointer: Pointer,
    pub keys: Keys,
    pub focus: Focus,
    /// How far the content is scrolled up, in logical pixels; 0 is the top.
    pub scroll: i32,
}

pub const START: Input = Input { pointer: Pointer::Outside, keys: Keys::Away, focus: Focus::Nowhere, scroll: 0 };

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Tab,
    BackTab,
    /// Enter or space.
    Activate,
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    PointerMoved { x: i32, y: i32 },
    PointerLeft,
    ButtonDown,
    ButtonUp,
    KeyboardEntered,
    KeyboardLeft,
    Key(Key),
    /// Characters a key press made, for the field in focus.
    Typed(String),
    /// The view moved by `dy` pixels (the wheel or a page key) on a surface `view_h` tall.
    Scrolled { dy: i32, view_h: i32 },
    /// The tree was replaced: drop any id that is no longer in it.
    TreeChanged,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Nothing,
    /// The state changed in a way that shows; paint again.
    Redraw,
    /// A button was pressed (pointer released on it, or Enter while it had focus).
    Press(String),
    /// The field `id` took this change to what it holds.
    Edit { id: String, edit: Edit },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Insert(String),
    Backspace,
    Delete,
    Left,
    Right,
    Start,
    End,
}

pub fn describe(s: &Input) -> String {
    let pointer = match &s.pointer {
        Pointer::Outside => "outside".to_string(),
        Pointer::Over { id } => format!("over {id}"),
        Pointer::Pressed { id, over } => format!("pressed {id}{}", if *over { "" } else { " (left)" }),
    };
    let focus = match &s.focus {
        Focus::Nowhere => "none".to_string(),
        Focus::On { id } => id.clone(),
    };
    format!("pointer={pointer} keys={} focus={focus} scroll={}", if s.keys == Keys::Here { "here" } else { "away" }, s.scroll)
}

fn contains(r: &Rect, x: i32, y: i32) -> bool {
    x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h
}

fn bounds(n: &Node) -> &Rect {
    match n {
        Node::Panel { bounds, .. } | Node::Label { bounds, .. } | Node::Prose { bounds, .. } | Node::Headline { bounds, .. } | Node::Blob { bounds, .. } | Node::Hotspot { bounds, .. } | Node::Words { bounds, .. } | Node::Button { bounds, .. } | Node::Bar { bounds, .. } | Node::Plot { bounds, .. } | Node::Input { bounds, .. } => bounds,
    }
}

/// The id of the deepest node under the point; later children are drawn on top, so they win.
pub fn hit(root: &Node, x: i32, y: i32) -> Option<&str> {
    if !contains(bounds(root), x, y) {
        return None;
    }
    if let Node::Panel { children, .. } = root {
        if let Some(id) = children.iter().rev().find_map(|c| hit(c, x, y)) {
            return Some(id);
        }
    }
    Some(root.id())
}

/// Buttons and fields in tree order: the order Tab visits them.
fn buttons<'a>(n: &'a Node, out: &mut Vec<&'a str>) {
    match n {
        Node::Button { id, .. } | Node::Hotspot { id, .. } | Node::Input { id, .. } => out.push(id),
        Node::Panel { children, .. } => children.iter().for_each(|c| buttons(c, out)),
        _ => {}
    }
}

fn is_focusable(root: &Node, id: &str) -> bool {
    let mut all = vec![];
    buttons(root, &mut all);
    all.contains(&id)
}

fn is_input(root: &Node, id: &str) -> bool {
    fn walk(n: &Node, id: &str) -> bool {
        match n {
            Node::Input { id: i, .. } => i == id,
            Node::Panel { children, .. } => children.iter().any(|c| walk(c, id)),
            _ => false,
        }
    }
    walk(root, id)
}

fn exists(root: &Node, id: &str) -> bool {
    fn walk(n: &Node, id: &str) -> bool {
        n.id() == id || matches!(n, Node::Panel { children, .. } if children.iter().any(|c| walk(c, id)))
    }
    walk(root, id)
}

fn changed(before: &Input, after: &Input) -> Effect {
    if before == after { Effect::Nothing } else { Effect::Redraw }
}

pub fn step(state: &Input, event: Event, root: &Node) -> (Input, Effect) {
    let mut next = state.clone();
    let mut effect = None;
    match event {
        Event::PointerMoved { x, y } => {
            let under = hit(root, x, y).map(str::to_string);
            next.pointer = match (&state.pointer, under) {
                (Pointer::Pressed { id, .. }, under) => Pointer::Pressed { id: id.clone(), over: under.as_deref() == Some(id.as_str()) },
                (_, Some(id)) => Pointer::Over { id },
                (_, None) => Pointer::Outside,
            };
        }
        Event::PointerLeft => {
            next.pointer = match &state.pointer {
                Pointer::Pressed { id, .. } => Pointer::Pressed { id: id.clone(), over: false },
                _ => Pointer::Outside,
            };
        }
        Event::ButtonDown => {
            if let Pointer::Over { id } = &state.pointer {
                if is_focusable(root, id) {
                    next.pointer = Pointer::Pressed { id: id.clone(), over: true };
                    next.focus = Focus::On { id: id.clone() };
                }
            }
        }
        Event::ButtonUp => {
            if let Pointer::Pressed { id, over } = &state.pointer {
                next.pointer = if *over { Pointer::Over { id: id.clone() } } else { Pointer::Outside };
                if *over {
                    effect = Some(Effect::Press(id.clone()));
                }
            }
        }
        Event::KeyboardEntered => next.keys = Keys::Here,
        Event::KeyboardLeft => next.keys = Keys::Away,
        Event::Key(key) if state.keys == Keys::Here => match key {
            Key::Tab | Key::BackTab => {
                let mut all = vec![];
                buttons(root, &mut all);
                let at = match &state.focus {
                    Focus::On { id } => all.iter().position(|b| b == id),
                    Focus::Nowhere => None,
                };
                let n = all.len();
                let to = match (at, key) {
                    (_, _) if n == 0 => None,
                    (None, Key::Tab) => Some(0),
                    (None, _) => Some(n - 1),
                    (Some(i), Key::Tab) => Some((i + 1) % n),
                    (Some(i), _) => Some((i + n - 1) % n),
                };
                next.focus = to.map_or(Focus::Nowhere, |i| Focus::On { id: all[i].to_string() });
            }
            Key::Activate => {
                if let Focus::On { id } = &state.focus {
                    effect = Some(Effect::Press(id.clone()));
                }
            }
            Key::Backspace => {
                if let Focus::On { id } = &state.focus {
                    if is_input(root, id) {
                        effect = Some(Effect::Edit { id: id.clone(), edit: Edit::Backspace });
                    }
                }
            }
            Key::Delete | Key::Left | Key::Right | Key::Home | Key::End => {
                if let Focus::On { id } = &state.focus {
                    if is_input(root, id) {
                        let edit = match key {
                            Key::Delete => Edit::Delete,
                            Key::Left => Edit::Left,
                            Key::Right => Edit::Right,
                            Key::Home => Edit::Start,
                            _ => Edit::End,
                        };
                        effect = Some(Effect::Edit { id: id.clone(), edit });
                    }
                }
            }
            Key::Other => {}
        },
        Event::Typed(text) if state.keys == Keys::Here => {
            if let Focus::On { id } = &state.focus {
                if is_input(root, id) {
                    effect = Some(Effect::Edit { id: id.clone(), edit: Edit::Insert(text) });
                }
            }
        }
        // Space presses the focused button, as Enter does; in a field it is a character (above).
        Event::Typed(text) if state.keys == Keys::Here && text == " " => {
            if let Focus::On { id } = &state.focus {
                effect = Some(Effect::Press(id.clone()));
            }
        }
        Event::Typed(_) => {}
        Event::Scrolled { dy, view_h } => {
            // `root` is already moved by the current scroll; its height is the content's.
            let most = (bounds(root).h - view_h).max(0);
            next.scroll = (state.scroll + dy).clamp(0, most);
        }
        Event::Key(_) => {}
        Event::TreeChanged => {
            let keep = |id: &str| exists(root, id);
            if let Pointer::Over { id } | Pointer::Pressed { id, .. } = &state.pointer {
                if !keep(id) {
                    next.pointer = Pointer::Outside;
                }
            }
            if let Focus::On { id } = &state.focus {
                if !keep(id) || !is_focusable(root, id) {
                    next.focus = Focus::Nowhere;
                }
            }
        }
    }
    let effect = effect.unwrap_or_else(|| changed(state, &next));
    (next, effect)
}

/// Whether the focus is in a text field (the arrows, Home and End then move the caret, not the view).
pub fn focus_in_field(state: &Input, root: &Node) -> bool {
    matches!(&state.focus, Focus::On { id } if is_input(root, id))
}

/// A field's box and what it holds, if `id` is a field.
pub fn field_box<'a>(root: &'a Node, id: &str) -> Option<(&'a Rect, &'a str)> {
    match root {
        Node::Input { id: i, bounds, text, .. } if i == id => Some((bounds, text)),
        Node::Panel { children, .. } => children.iter().find_map(|c| field_box(c, id)),
        _ => None,
    }
}
