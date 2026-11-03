//! casement: a native Wayland surface painted by us, that an outside process can inspect.
//!
//! The surface (an xdg window, or with `--layer` a layer-shell overlay) is driven by the state
//! machine in `pace.rs`. The Wayland handlers below each turn what the compositor said into one
//! `Event`, call `step`, and do the `Effect` it returns. They decide nothing.

use casement::counting::ALLOCS;
use casement::input::{self, Effect as InputEffect, Event as InputEvent, Input, Key};
use casement::intui_scene;
use casement::inspect::{encode_png, read_request, refused, route, Route};
use casement::paint::{paint, WHOLE};
use casement::text::Fonts;
use casement::ui::{click, demo_tree, Click, Node};
use serde_json::{json, Value};
use casement::pace::{describe, step, Effect, Event, Pacing, State, DEFAULT_SIZE, OVERDUE};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_data_device, delegate_keyboard, delegate_layer, delegate_pointer, delegate_seat, delegate_output, delegate_registry, delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::calloop::{generic::Generic, LoopHandle, timer::{TimeoutAction, Timer}, EventLoop, Interest, Mode, PostAction},
    reexports::calloop_wayland_source::WaylandSource,
    data_device_manager::{
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, DragOffer},
        data_source::{CopyPasteSource, DataSourceHandler},
        DataDeviceManagerState, WritePipe,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{slot::{Buffer, SlotPool}, Shm, ShmHandler},
};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};
use std::io::Write;
use std::net::TcpListener;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use wayland_client::{
    globals::{registry_queue_init, GlobalList},
    Dispatch,
    protocol::{wl_data_device, wl_data_device_manager::DndAction, wl_data_source, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};

/// What we are showing on: the compositor's idea of a window, or an overlay we anchor ourselves.
enum Surface {
    Window(Window),
    Layer(LayerSurface),
    /// A layer overlay waiting for an output to exist.
    Absent,
}

impl Surface {
    fn wl_surface(&self) -> Option<&wl_surface::WlSurface> {
        match self {
            Surface::Window(w) => Some(w.wl_surface()),
            Surface::Layer(l) => Some(l.wl_surface()),
            Surface::Absent => None,
        }
    }
    fn commit(&self) {
        match self {
            Surface::Window(w) => w.commit(),
            Surface::Layer(l) => l.commit(),
            Surface::Absent => {}
        }
    }
}

/// What the compositor offers for scaling. With both, we paint at device pixels and tell it the
/// logical size; without, we paint 1:1 and the compositor stretches.
enum Scaler {
    Absent,
    Present { fractional: WpFractionalScaleManagerV1, viewporter: WpViewporter },
}

/// The scaling objects of one surface; made again with every new surface.
enum Scaling {
    None,
    On { _fractional: WpFractionalScaleV1, viewport: WpViewport },
}

fn scaling_for(scaler: &Scaler, surface: &wl_surface::WlSurface, qh: &QueueHandle<App>) -> Scaling {
    match scaler {
        Scaler::Absent => Scaling::None,
        Scaler::Present { fractional, viewporter } => Scaling::On { _fractional: fractional.get_fractional_scale(surface, qh, ()), viewport: viewporter.get_viewport(surface, qh, ()) },
    }
}

fn bind_scaler(globals: &GlobalList, qh: &QueueHandle<App>) -> Scaler {
    match (globals.bind(qh, 1..=1, ()), globals.bind(qh, 1..=1, ())) {
        (Ok(fractional), Ok(viewporter)) => Scaler::Present { fractional, viewporter },
        _ => Scaler::Absent,
    }
}

/// What is on the surface.
enum Scene {
    /// Until an app posts something: a button and a status line.
    Demo,
    /// A node tree with sizes already set (POST /tree).
    Nodes(Node),
    /// An IntUI tree (POST /intui); its layout is made for the surface's width each time it is drawn.
    Intui(intui_types::Tree),
}

/// While the loop sleeps with nothing owed, what was counted when the quiet began.
#[derive(Clone, Copy)]
enum QuietWatch {
    NotQuiet,
    Since { at: Instant, allocs: u64 },
}

/// What was asked for on the command line that the surface needs to be made again.
#[derive(Clone, Copy)]
enum Kind {
    Window,
    Layer { size: (u32, u32), keys: bool },
}

struct App {
    registry: RegistryState,
    output: OutputState,
    seat: SeatState,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    /// The event loop, so a held key can repeat on its timer.
    loop_handle: LoopHandle<'static, App>,
    /// Hover, press and focus (input.rs); the handlers below only translate into its events.
    input: Input,
    shm: Shm,
    pool: SlotPool,
    qh: QueueHandle<App>,
    compositor: CompositorState,
    layer_shell: Option<LayerShell>,
    kind: Kind,
    /// Output names to put the overlay on, best first; empty means any.
    prefer: Vec<String>,
    surface: Surface,
    scaler: Scaler,
    scaling: Scaling,
    /// The scale the compositor prefers, in 120ths (120 is 1.0).
    scale: u32,
    /// Buffers of the current size; one is reused unless the compositor still holds it.
    buffers: Vec<Buffer>,
    /// Our own copy of the last painted frame, the exact pixels (the compositor blends its own).
    frame: Vec<u8>,
    frame_size: (u32, u32),
    /// Cold until the first label is drawn; costs nothing at start.
    fonts: Fonts,
    /// The tree an app posted; until one is, the demo tree shows.
    scene: Scene,
    /// IntUI events caused by presses, waiting for an outside process to read them (GET /events).
    events: Vec<String>,
    /// Options already pressed, so a `once` Choice shows used up and refuses a second press.
    pressed_options: Vec<intui_scene::Pressed>,
    /// What each form field holds, by `form.field`.
    field_values: Vec<(String, String)>,
    /// Where the caret is in each field (characters from the start), by field node id.
    carets: Vec<(String, usize)>,
    /// The clipboard: the manager, our device on the seat, and the text we are offering (if we copied).
    data_manager: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    copy_source: Option<CopyPasteSource>,
    clipboard: Option<String>,
    /// Whether Control is held, from the modifiers the compositor reports.
    ctrl: bool,
    /// Where the pointer last was on the surface, in logical pixels.
    pointer_at: (i32, i32),
    state: State,
    /// When the last callback was asked for.
    requested_at: Instant,
    /// When the surface was taken away, so the recovery time can be reported.
    gone_at: Instant,
    quiet: QuietWatch,
    log: bool,
    started: Instant,
    first_frame_reported: bool,
    /// Counters for the soak: buffers committed, frame callbacks received.
    paints: u64,
    callbacks: u64,
}

impl App {
    /// A key press, or one repeat of a held key (`serial` is 0 for a repeat).
    fn handle_key(&mut self, serial: u32, event: KeyEvent) {
        // Control+C, X and V work on the field in focus: the whole field is copied or cut; paste goes at the caret.
        if self.ctrl && matches!(event.keysym, Keysym::c | Keysym::x | Keysym::v) {
            if let Some((id, text)) = self.focused_field() {
                match event.keysym {
                    Keysym::c => self.copy(text, serial),
                    Keysym::x => {
                        self.copy(text, serial);
                        self.field_values.retain(|(i, _)| *i != id);
                        self.carets.retain(|(i, _)| *i != id);
                        self.feed(Event::Changed);
                    }
                    _ => {
                        if let Some(pasted) = self.paste_text() {
                            self.edit_field(id, input::Edit::Insert(pasted.replace(['\n', '\r'], " ")));
                        }
                    }
                }
            }
            return;
        }
        // A raw node scene is told about Space (a caller can step its opener with it).
        if matches!(self.scene, Scene::Nodes(_)) && event.keysym == Keysym::space {
            self.events.push(json!({"event": "key", "key": "space"}).to_string());
        }
        if self.log {
            eprintln!("{:>7} ms  key {:?} ({:?})", self.started.elapsed().as_millis(), event.keysym, key_of(event.keysym));
        }
        // Keys that move the view: a line, a page, the ends.
        let view = if let State::Showing { h, .. } = self.state { h as i32 } else { 0 };
        // In a text field, Home and End move the caret; elsewhere they move the view.
        let in_field = match &self.state {
            State::Showing { w, h, colour, .. } => input::focus_in_field(&self.input, &self.scene_for(*w, *h, *colour)),
            _ => false,
        };
        let dy = match event.keysym {
            Keysym::End | Keysym::Home if in_field => None,
            Keysym::Down => Some(40),
            Keysym::Up => Some(-40),
            Keysym::Page_Down => Some(view - 40),
            Keysym::Page_Up => Some(-(view - 40)),
            Keysym::End => Some(i32::MAX / 2),
            Keysym::Home => Some(-(i32::MAX / 2)),
            _ => None,
        };
        if let Some(dy) = dy {
            self.feed_input(InputEvent::Scrolled { dy, view_h: view });
        }
        self.feed_input(InputEvent::Key(key_of(event.keysym)));
        // What the key typed, when it typed something a person would read (not Tab, Enter, Backspace, Escape).
        if let Some(text) = event.utf8.filter(|t| !t.is_empty() && t.chars().all(|c| !c.is_control())) {
            self.feed_input(InputEvent::Typed(text));
        }
    }
    fn feed(&mut self, event: Event) {
        let allocs_at_event = ALLOCS.load(Ordering::Relaxed);
        let before = self.state;
        let (next, effect) = step(self.state, event);
        self.state = next;
        if self.log && before != next {
            eprintln!("{:>7} ms  {} -> {}", self.started.elapsed().as_millis(), describe(before), describe(next));
        }
        self.watch_quiet(before, next, allocs_at_event);
        match effect {
            Effect::Paint { w, h, colour } => self.paint(w, h, colour),
            Effect::TryRecreate => self.try_recreate(),
            Effect::Exit | Effect::Nothing => {}
        }
    }

    /// One seat event into the input machine; a press is the same act as POST /click.
    fn feed_input(&mut self, event: InputEvent) {
        let State::Showing { w, h, colour, .. } = self.state else { return };
        let root = self.scene_for(w, h, colour);
        let before = self.input.clone();
        let (next, effect) = input::step(&self.input, event, &root);
        self.input = next;
        if self.log && before != self.input {
            eprintln!("{:>7} ms  input {} -> {}", self.started.elapsed().as_millis(), input::describe(&before), input::describe(&self.input));
        }
        match effect {
            InputEffect::Nothing => {}
            InputEffect::Redraw => self.feed(Event::Changed),
            InputEffect::Edit { id, edit } => self.edit_field(id, edit),
            InputEffect::Press(id) => {
                println!("pressed {id}");
                self.place_caret(&root, &id);
                self.pressed(&id);
                if click(&root, &id) == Click::Pressed {
                    self.feed(Event::Changed);
                }
            }
        }
    }

    fn edit_field(&mut self, id: String, edit: input::Edit) {
        let held: Vec<char> = self.field_values.iter().find(|(i, _)| *i == id).map_or(vec![], |(_, v)| v.chars().collect());
        let at = self.carets.iter().find(|(i, _)| *i == id).map_or(held.len(), |(_, c)| (*c).min(held.len()));
        let (text, caret) = edit_text(held, at, edit);
        self.field_values.retain(|(i, _)| *i != id);
        self.field_values.push((id.clone(), text));
        self.carets.retain(|(i, _)| *i != id);
        self.carets.push((id, caret));
        self.feed(Event::Changed);
    }

    /// The field in focus and what it holds, if the focus is in a text field.
    fn focused_field(&self) -> Option<(String, String)> {
        let State::Showing { w, h, colour, .. } = self.state else { return None };
        let root = self.scene_for(w, h, colour);
        let input::Focus::On { id } = &self.input.focus else { return None };
        input::field_box(&root, id).map(|(_, text)| (id.clone(), text.to_string()))
    }

    /// Offer `text` as the clipboard.
    fn copy(&mut self, text: String, serial: u32) {
        let (Some(manager), Some(device)) = (&self.data_manager, &self.data_device) else { return };
        let source = manager.create_copy_paste_source(&self.qh, ["text/plain;charset=utf-8", "text/plain"]);
        source.set_selection(device, serial);
        self.copy_source = Some(source);
        self.clipboard = Some(text);
    }

    /// The clipboard's text: ours if we hold it, else what the owner sends (a moment's wait, then give up).
    fn paste_text(&mut self) -> Option<String> {
        if self.copy_source.is_some() {
            return self.clipboard.clone();
        }
        let offer = self.data_device.as_ref().and_then(|d| d.data().selection_offer());
        if self.log {
            eprintln!("paste: device {} offer {}", self.data_device.is_some(), offer.is_some());
        }
        let offer = offer?;
        let mut pipe = offer.receive("text/plain;charset=utf-8".to_string()).ok()?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut text = String::new();
            let _ = std::io::Read::read_to_string(&mut pipe, &mut text);
            let _ = tx.send(text);
        });
        rx.recv_timeout(Duration::from_millis(300)).ok()
    }

    /// A press inside a text field puts the caret at the character edge nearest the pointer.
    fn place_caret(&mut self, root: &Node, id: &str) {
        let Some((bounds, text)) = input::field_box(root, id) else { return };
        if text.starts_with('[') || self.pointer_at.1 < bounds.y || self.pointer_at.1 >= bounds.y + bounds.h {
            return;
        }
        let scale = self.scale;
        let text_px = ((14 * scale + 60) / WHOLE).max(1);
        let inset = ((8 * scale + 60) / WHOLE) as i32;
        let want = (self.pointer_at.0 * scale as i32 + 60) / WHOLE as i32 - ((bounds.x * scale as i32 + 60) / WHOLE as i32 + inset);
        let chars: Vec<char> = text.chars().collect();
        let mut best = (0usize, i32::MAX);
        for n in 0..=chars.len() {
            let prefix: String = chars[..n].iter().collect();
            let w = if n == 0 { 0 } else { casement::text::text_width(&mut self.fonts, &prefix, text_px) };
            let d = (w - want).abs();
            if d < best.1 {
                best = (n, d);
            }
        }
        self.carets.retain(|(i, _)| i != id);
        self.carets.push((id.to_string(), best.0));
    }

    /// A button was pressed (by pointer, key or POST /click): an IntUI Choice option becomes an event.
    fn pressed(&mut self, id: &str) {
        // A raw node scene has no Choice to turn into an IntUI event: the app asked to hear which node was pressed.
        if matches!(self.scene, Scene::Nodes(_)) {
            self.events.push(json!({"event": "pressed", "target": id}).to_string());
        }
        if let Scene::Intui(tree) = &self.scene {
            if let Some(steps) = intui_scene::cycle_of(tree.root(), id) {
                let now = self.field_values.iter().find(|(i, _)| i == id).map_or("", |(_, v)| v.as_str());
                let at = steps.iter().position(|v| v == now).unwrap_or(0);
                let next = steps[(at + 1) % steps.len()].clone();
                self.field_values.retain(|(i, _)| i != id);
                self.field_values.push((id.to_string(), next));
            }
            match intui_scene::submit_event(tree.root(), id, "casement", &self.field_values) {
                Some(Ok(event)) => self.events.push(event),
                Some(Err(words)) => eprintln!("casement: not submitted: {words}"),
                None => {}
            }
            if let Some(event) = intui_scene::press_event(tree.root(), id, "casement", &self.pressed_options) {
                if let Ok(v) = serde_json::from_str::<Value>(&event) {
                    let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
                    self.pressed_options.push(intui_scene::Pressed { choice: text("target"), option: text("value") });
                }
                self.events.push(event);
            }
        }
    }

    fn watch_quiet(&mut self, before: State, after: State, allocs_now: u64) {
        let quiet = |s: State| matches!(s, State::Showing { pacing: Pacing::Quiet, .. });
        match (quiet(before), quiet(after)) {
            (false, true) => self.quiet = QuietWatch::Since { at: Instant::now(), allocs: allocs_now },
            (true, false) => self.report_quiet(allocs_now),
            _ => {}
        }
    }

    fn report_quiet(&mut self, allocs_now: u64) {
        if let QuietWatch::Since { at, allocs } = self.quiet {
            let secs = at.elapsed().as_secs_f64();
            if secs > 1.0 {
                println!("quiet {:.1} s: {} allocations", secs, allocs_now - allocs);
            }
        }
        self.quiet = QuietWatch::NotQuiet;
    }

    fn paint(&mut self, w: u32, h: u32, colour: usize) {
        // The buffer is in device pixels; the surface is told it covers w x h logical ones.
        let (dw, dh) = ((w * self.scale + 60) / WHOLE, (h * self.scale + 60) / WHOLE);
        let stride = dw as i32 * 4;
        let size_changed = self.buffers.first().is_some_and(|b| b.height() != dh as i32 || b.stride() != stride);
        if size_changed {
            self.buffers.clear();
        }
        let free = (0..self.buffers.len()).find(|&i| self.buffers[i].canvas(&mut self.pool).is_some());
        let index = match free {
            Some(i) => i,
            None => {
                let (buffer, _) = self.pool.create_buffer(dw as i32, dh as i32, stride, wl_shm::Format::Argb8888).expect("create buffer");
                self.buffers.push(buffer);
                self.buffers.len() - 1
            }
        };
        self.render(w, h, colour);
        let canvas = self.buffers[index].canvas(&mut self.pool).expect("free buffer has a canvas");
        canvas.copy_from_slice(&self.frame);
        let Some(surface) = self.surface.wl_surface() else { return };
        surface.damage_buffer(0, 0, dw as i32, dh as i32);
        if let Scaling::On { viewport, .. } = &self.scaling {
            viewport.set_destination(w as i32, h as i32);
        }
        surface.frame(&self.qh, surface.clone());
        self.buffers[index].attach_to(surface).expect("attach");
        self.surface.commit();
        self.paints += 1;
        self.requested_at = Instant::now();
        if !self.first_frame_reported {
            self.first_frame_reported = true;
            println!("first frame {} ms ({}x{}, device {}x{})", self.started.elapsed().as_millis(), w, h, dw, dh);
        }
    }

    /// What is on the surface at this size: the posted tree, or the demo.
    fn scene_for(&self, w: u32, h: u32, rev: usize) -> Node {
        match &self.scene {
            Scene::Demo => demo_tree(w, h, rev),
            Scene::Nodes(node) => node.clone(),
            Scene::Intui(tree) => intui_scene::layout(tree.root(), w as i32, h as i32, intui_scene::Memory { pressed: &self.pressed_options, values: &self.field_values, carets: &self.carets }).shifted(-self.input.scroll),
        }
    }

    /// Draw the frame into our own copy. Needs no compositor: an agent can ask for it while hidden.
    fn render(&mut self, w: u32, h: u32, colour: usize) {
        let device = ((w * self.scale + 60) / WHOLE, (h * self.scale + 60) / WHOLE);
        if self.frame_size != device {
            self.frame = vec![0; (device.0 * device.1 * 4) as usize];
            self.frame_size = device;
        }
        let node = self.scene_for(w, h, colour);
        paint(&mut self.fonts, &mut self.frame, device, &node, &self.input, self.scale);
    }

    /// Make a new layer surface on the first output there is. Windows are never taken away from us.
    fn try_recreate(&mut self) {
        let Kind::Layer { size, keys } = self.kind else { return };
        let Some(layer_shell) = &self.layer_shell else { return };
        let Some(output) = self.pick_output() else { return };
        self.buffers.clear();
        let layer = make_layer(layer_shell, &self.compositor, &self.qh, Some(&output), size, keys);
        self.scaling = scaling_for(&self.scaler, layer.wl_surface(), &self.qh);
        self.surface = Surface::Layer(layer);
        println!("recreated {} ms after it was taken away", self.gone_at.elapsed().as_millis());
        self.feed(Event::Recreated);
    }

    /// The best output we may use that exists now.
    fn pick_output(&self) -> Option<wl_output::WlOutput> {
        let named = |o: &wl_output::WlOutput| self.output.info(o).and_then(|i| i.name);
        if self.prefer.is_empty() {
            return self.output.outputs().next();
        }
        self.prefer.iter().find_map(|want| self.output.outputs().find(|o| named(o).as_deref() == Some(want.as_str())))
    }

    fn serve(&mut self, mut stream: std::net::TcpStream) {
        let request = read_request(&mut stream);
        let reply: Vec<u8> = if let Some(why) = refused(&request) {
            format!("HTTP/1.0 403 Forbidden\r\n\r\n{why}\n").into_bytes()
        } else {
            self.answer(&request)
        };
        // The reply goes out on its own thread: a client that never reads it must not stop the window.
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        std::thread::spawn(move || {
            let _ = stream.write_all(&reply);
        });
    }

    /// What the socket says to an accepted request.
    fn answer(&mut self, request: &str) -> Vec<u8> {
        match route(request) {
            Route::Png => match {
                if let State::Showing { w, h, colour, .. } = self.state {
                    self.render(w, h, colour);
                }
                encode_png(self.frame_size.0, self.frame_size.1, &self.frame)
            } {
                Ok(png) if self.frame_size.0 > 0 => [format!("HTTP/1.0 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\n\r\n", png.len()).into_bytes(), png].concat(),
                Ok(_) => b"HTTP/1.0 503 Service Unavailable\r\n\r\nno frame painted yet\n".to_vec(),
                Err(e) => format!("HTTP/1.0 500 Internal Server Error\r\n\r\n{e}\n").into_bytes(),
            },
            Route::State => format!("HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\n{} paints={} callbacks={} allocs={} input: {}\n", describe(self.state), self.paints, self.callbacks, ALLOCS.load(Ordering::Relaxed), input::describe(&self.input)).into_bytes(),
            Route::Events => {
                let body: String = self.events.drain(..).map(|e| e + "\n").collect();
                format!("HTTP/1.0 200 OK\r\nContent-Type: application/x-ndjson\r\n\r\n{body}").into_bytes()
            }
            Route::Tree => match self.state {
                State::Showing { w, h, colour, .. } => format!("HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{}\n", self.scene_for(w, h, colour).to_json()).into_bytes(),
                _ => b"HTTP/1.0 503 Service Unavailable\r\n\r\nnothing is showing\n".to_vec(),
            },
            Route::Click(id) => match self.state {
                State::Showing { w, h, colour, .. } => match click(&self.scene_for(w, h, colour), &id) {
                    Click::Pressed => {
                        self.pressed(&id);
                        self.feed(Event::Changed);
                        format!("HTTP/1.0 200 OK\r\n\r\npressed {id}\n").into_bytes()
                    }
                    Click::NotClickable => format!("HTTP/1.0 409 Conflict\r\n\r\n{id} is not a button\n").into_bytes(),
                    Click::Unknown => format!("HTTP/1.0 404 Not Found\r\n\r\nno node {id}\n").into_bytes(),
                },
                _ => b"HTTP/1.0 503 Service Unavailable\r\n\r\nnothing is showing\n".to_vec(),
            },
            Route::SetTree(body) => match serde_json::from_str::<Value>(&body).map_err(|e| e.to_string()).and_then(|v| Node::from_json(&v)) {
                Ok(node) => {
                    self.scene = Scene::Nodes(node);
                    self.feed_input(InputEvent::TreeChanged);
                    self.feed(Event::Changed);
                    b"HTTP/1.0 200 OK\r\n\r\ntree set\n".to_vec()
                }
                Err(e) => format!("HTTP/1.0 400 Bad Request\r\n\r\n{e}\n").into_bytes(),
            },
            Route::SetIntui(body) => match intui_types::parse(&body, intui_types::Author::Unchecked) {
                Ok(tree) => {
                    self.scene = Scene::Intui(tree);
                    self.input.scroll = 0;
                    self.pressed_options.clear();
                    self.field_values.clear();
                    self.carets.clear();
                    self.feed_input(InputEvent::TreeChanged);
                    self.feed(Event::Changed);
                    b"HTTP/1.0 200 OK\r\n\r\nintui tree set\n".to_vec()
                }
                Err(refusal) => format!("HTTP/1.0 400 Bad Request\r\n\r\n{refusal}\n").into_bytes(),
            },
            Route::Unknown => b"HTTP/1.0 404 Not Found\r\n\r\nGET /frame.png, /state, /tree, /events; POST /tree (json), POST /intui (IntUI json), POST /click/<id>\n".to_vec(),
        }
    }

    /// How long the loop may sleep: forever when no callback is owed, else until it is overdue.
    fn sleep_for(&self) -> Option<Duration> {
        match self.state {
            State::Showing { pacing: Pacing::Pending | Pacing::PendingChanged, .. } => Some(OVERDUE.saturating_sub(self.requested_at.elapsed())),
            _ => None,
        }
    }
}

fn make_layer(shell: &LayerShell, compositor: &CompositorState, qh: &QueueHandle<App>, output: Option<&wl_output::WlOutput>, size: (u32, u32), keys: bool) -> LayerSurface {
    let layer = shell.create_layer_surface(qh, compositor.create_surface(qh), Layer::Overlay, Some("casement"), output);
    layer.set_anchor(Anchor::TOP | Anchor::RIGHT);
    layer.set_margin(16, 16, 0, 0);
    layer.set_size(size.0, size.1);
    // `--keys`: the compositor may give this window the keyboard when it is clicked, never on its own.
    layer.set_keyboard_interactivity(if keys { KeyboardInteractivity::OnDemand } else { KeyboardInteractivity::None });
    layer.commit();
    layer
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.callbacks += 1;
        self.feed(Event::FrameCallback);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for App {
    fn event(_: &mut Self, _: &WpFractionalScaleManagerV1, _: <WpFractionalScaleManagerV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WpViewporter, ()> for App {
    fn event(_: &mut Self, _: &WpViewporter, _: <WpViewporter as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WpViewport, ()> for App {
    fn event(_: &mut Self, _: &WpViewport, _: <WpViewport as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WpFractionalScaleV1, ()> for App {
    fn event(app: &mut Self, _: &WpFractionalScaleV1, event: wp_fractional_scale_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        let wp_fractional_scale_v1::Event::PreferredScale { scale } = event else { return };
        if scale != app.scale {
            app.scale = scale;
            app.feed(Event::Changed);
        }
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.feed(Event::OutputAppeared);
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.feed(Event::CloseRequested);
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window, configure: WindowConfigure, _: u32) {
        let (w, h) = configure.new_size;
        self.feed(Event::Configured { w: w.map(|v| v.get()), h: h.map(|v| v.get()) });
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.gone_at = Instant::now();
        self.feed(Event::SurfaceClosed);
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let (w, h) = configure.new_size;
        self.feed(Event::Configured { w: (w > 0).then_some(w), h: (h > 0).then_some(h) });
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        match capability {
            Capability::Pointer if self.pointer.is_none() => self.pointer = self.seat.get_pointer(qh, &seat).ok(),
            Capability::Keyboard if self.keyboard.is_none() => {
                // A held key repeats at the compositor's rate: the repeat timer feeds the same path as a press.
                let repeat = Box::new(|app: &mut App, _: &wl_keyboard::WlKeyboard, event: KeyEvent| {
                    if !app.ctrl {
                        app.handle_key(0, event);
                    }
                });
                self.keyboard = self.seat.get_keyboard_with_repeat(qh, &seat, None, self.loop_handle.clone(), repeat).ok();
                if self.data_device.is_none() {
                    self.data_device = self.data_manager.as_ref().map(|m| m.get_data_device(qh, &seat));
                }
            }
            _ => {}
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        match capability {
            Capability::Pointer => {
                // Release the object we hold, or the next device's events arrive on it as well as on the new one.
                if let Some(pointer) = self.pointer.take() {
                    pointer.release();
                }
            }
            Capability::Keyboard => {
                if let Some(keyboard) = self.keyboard.take() {
                    keyboard.release();
                }
                self.feed_input(InputEvent::KeyboardLeft);
            }
            _ => {}
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

/// BTN_LEFT in linux/input-event-codes.h.
const BTN_LEFT: u32 = 0x110;

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if self.surface.wl_surface() != Some(&e.surface) {
                continue;
            }
            let (x, y) = (e.position.0 as i32, e.position.1 as i32);
            self.pointer_at = (x, y);
            match e.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => self.feed_input(InputEvent::PointerMoved { x, y }),
                PointerEventKind::Axis { vertical, .. } => {
                    if let State::Showing { h, .. } = self.state {
                        self.feed_input(InputEvent::Scrolled { dy: vertical.absolute as i32, view_h: h as i32 });
                    }
                }
                PointerEventKind::Leave { .. } => self.feed_input(InputEvent::PointerLeft),
                PointerEventKind::Press { button: BTN_LEFT, .. } => self.feed_input(InputEvent::ButtonDown),
                PointerEventKind::Release { button: BTN_LEFT, .. } => self.feed_input(InputEvent::ButtonUp),
                _ => {}
            }
        }
    }
}

/// What a field holds and where its caret is after one edit.
fn edit_text(mut text: Vec<char>, caret: usize, edit: input::Edit) -> (String, usize) {
    let mut caret = caret.min(text.len());
    match edit {
        input::Edit::Insert(typed) => {
            let typed: Vec<char> = typed.chars().collect();
            caret += typed.len();
            text.splice(caret - typed.len()..caret - typed.len(), typed);
        }
        input::Edit::Backspace if caret > 0 => {
            text.remove(caret - 1);
            caret -= 1;
        }
        input::Edit::Delete if caret < text.len() => {
            text.remove(caret);
        }
        input::Edit::Left => caret = caret.saturating_sub(1),
        input::Edit::Right => caret = (caret + 1).min(text.len()),
        input::Edit::Start => caret = 0,
        input::Edit::End => caret = text.len(),
        input::Edit::Backspace | input::Edit::Delete => {}
    }
    (text.into_iter().collect(), caret)
}

fn key_of(keysym: Keysym) -> Key {
    match keysym {
        Keysym::Tab => Key::Tab,
        Keysym::ISO_Left_Tab => Key::BackTab,
        Keysym::Return | Keysym::KP_Enter => Key::Activate,
        Keysym::BackSpace => Key::Backspace,
        Keysym::Delete => Key::Delete,
        Keysym::Left => Key::Left,
        Keysym::Right => Key::Right,
        Keysym::Home => Key::Home,
        Keysym::End => Key::End,
        _ => Key::Other,
    }
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {
        if self.surface.wl_surface() == Some(surface) {
            self.feed_input(InputEvent::KeyboardEntered);
        }
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        if self.surface.wl_surface() == Some(surface) {
            self.feed_input(InputEvent::KeyboardLeft);
        }
    }
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, serial: u32, event: KeyEvent) {
        self.handle_key(serial, event);
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: RawModifiers, _: u32) {
        self.ctrl = modifiers.ctrl;
    }
}

impl DataDeviceHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice, _: f64, _: f64, _: &wl_surface::WlSurface) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice, _: f64, _: f64) {}
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
}

impl DataOfferHandler for App {
    fn source_actions(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
    fn selected_action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: DndAction) {}
}

impl DataSourceHandler for App {
    fn accept_mime(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource, _: Option<String>) {}
    fn send_request(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource, _: String, mut fd: WritePipe) {
        if let Some(text) = &self.clipboard {
            let _ = fd.write_all(text.as_bytes());
        }
    }
    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource) {
        self.copy_source = None;
        self.clipboard = None;
    }
    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource) {}
    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource) {}
    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_source::WlDataSource, _: DndAction) {}
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_seat!(App);
delegate_data_device!(App);
delegate_pointer!(App);
delegate_keyboard!(App);
delegate_xdg_shell!(App);
delegate_xdg_window!(App);
delegate_layer!(App);
delegate_registry!(App);

/// The value after `--name`, parsed.
fn flag<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok())
}

fn parse_size(args: &[String]) -> Option<(u32, u32)> {
    let v: String = flag(args, "--size")?;
    let (w, h) = v.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

fn main() {
    let started = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let conn = Connection::connect_to_env().unwrap_or_else(|e| die(&format!("no Wayland compositor: {e}")));
    let (globals, queue) = registry_queue_init(&conn).unwrap_or_else(|e| die(&format!("registry: {e}")));
    let qh = queue.handle();
    let mut event_loop: EventLoop<App> = EventLoop::try_new().unwrap_or_else(|e| die(&format!("event loop: {e}")));
    WaylandSource::new(conn.clone(), queue).insert(event_loop.handle()).unwrap_or_else(|e| die(&format!("wayland source: {e}")));

    let compositor = CompositorState::bind(&globals, &qh).unwrap_or_else(|e| die(&format!("wl_compositor: {e}")));
    let shm = Shm::bind(&globals, &qh).unwrap_or_else(|e| die(&format!("wl_shm: {e}")));
    let output = OutputState::new(&globals, &qh);

    let (kind, surface, layer_shell) = if args.iter().any(|a| a == "--layer") {
        let size = parse_size(&args).unwrap_or((280, 120));
        let shell = LayerShell::bind(&globals, &qh).unwrap_or_else(|e| die(&format!("wlr_layer_shell: {e}")));
        // No surface yet: the first one is made on the first matching output, by the path every later one takes.
        (Kind::Layer { size, keys: args.iter().any(|a| a == "--keys") }, Surface::Absent, Some(shell))
    } else {
        let xdg_shell = XdgShell::bind(&globals, &qh).unwrap_or_else(|e| die(&format!("xdg_shell: {e}")));
        let window = xdg_shell.create_window(compositor.create_surface(&qh), WindowDecorations::ServerDefault, &qh);
        window.set_title("casement");
        window.set_app_id("casement");
        let size = parse_size(&args);
        if let Some((w, h)) = size {
            window.set_min_size(Some((w, h)));
            window.set_max_size(Some((w, h)));
        }
        window.commit();
        (Kind::Window, Surface::Window(window), None)
    };

    let scaler = bind_scaler(&globals, &qh);
    let scaling = surface.wl_surface().map_or(Scaling::None, |s| scaling_for(&scaler, s, &qh));
    let pool = SlotPool::new((DEFAULT_SIZE.0 * DEFAULT_SIZE.1 * 4) as usize, &shm).unwrap_or_else(|e| die(&format!("shm pool: {e}")));
    let mut app = App {
        registry: RegistryState::new(&globals),
        output,
        seat: SeatState::new(&globals, &qh),
        pointer: None,
        keyboard: None,
        loop_handle: event_loop.handle(),
        input: input::START,
        shm,
        pool,
        qh: qh.clone(),
        compositor,
        layer_shell,
        kind,
        prefer: flag::<String>(&args, "--output").map(|v| v.split(',').map(str::to_string).collect()).unwrap_or_default(),
        surface,
        scaler,
        scaling,
        scale: WHOLE,
        buffers: vec![],
        frame: vec![],
        frame_size: (0, 0),
        fonts: Fonts::Cold,
        scene: Scene::Demo,
        events: vec![],
        pressed_options: vec![],
        field_values: vec![],
        carets: vec![],
        data_manager: DataDeviceManagerState::bind(&globals, &qh).ok(),
        data_device: None,
        copy_source: None,
        clipboard: None,
        ctrl: false,
        pointer_at: (0, 0),
        state: if matches!(kind, Kind::Layer { .. }) { State::Gone { colour: 0 } } else { State::Waiting { colour: 0 } },
        requested_at: Instant::now(),
        gone_at: Instant::now(),
        quiet: QuietWatch::NotQuiet,
        log: args.iter().any(|a| a == "--log"),
        started,
        first_frame_reported: false,
        paints: 0,
        callbacks: 0,
    };
    if let Some(port) = flag::<u16>(&args, "--port") {
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|e| die(&format!("127.0.0.1:{port}: {e}")));
        listener.set_nonblocking(true).unwrap_or_else(|e| die(&format!("listener: {e}")));
        println!("listening on 127.0.0.1:{}", listener.local_addr().map(|a| a.port()).unwrap_or(0));
        event_loop
            .handle()
            .insert_source(Generic::new(listener, Interest::READ, Mode::Level), |_, listener, app: &mut App| {
                while let Ok((stream, _)) = listener.accept() {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                    app.serve(stream);
                }
                Ok(PostAction::Continue)
            })
            .unwrap_or_else(|e| die(&format!("socket source: {e}")));
    }
    if let Some(ms) = flag::<u64>(&args, "--tick") {
        let every = Duration::from_millis(ms);
        event_loop
            .handle()
            .insert_source(Timer::from_duration(every), move |_, _, app: &mut App| {
                app.feed(Event::Changed);
                TimeoutAction::ToDuration(every)
            })
            .unwrap_or_else(|e| die(&format!("tick timer: {e}")));
    }
    if let Some(secs) = flag::<u64>(&args, "--exit-after") {
        event_loop
            .handle()
            .insert_source(Timer::from_duration(Duration::from_secs(secs)), |_, _, app: &mut App| {
                app.feed(Event::CloseRequested);
                TimeoutAction::Drop
            })
            .unwrap_or_else(|e| die(&format!("exit timer: {e}")));
    }
    while app.state != State::Closed {
        event_loop.dispatch(app.sleep_for(), &mut app).unwrap_or_else(|e| die(&format!("dispatch: {e}")));
        if app.sleep_for().is_some_and(|left| left.is_zero()) {
            app.feed(Event::CallbackOverdue);
        }
    }
}

fn die(msg: &str) -> ! {
    eprintln!("casement: {msg}");
    std::process::exit(1)
}
