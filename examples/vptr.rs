//! A pointer for tests: moves to (x, y) on the output and clicks, through wlr-virtual-pointer.
//! Usage: vptr <x> <y> <width> <height> [click | scroll <pixels>]   (width and height are the output's size)
//! Aims at $WAYLAND_DISPLAY, so point it at the headless rig, never the live desktop.
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry, wl_seat::WlSeat},
    Connection, Dispatch, QueueHandle,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1};

struct Quiet;
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Quiet {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for Quiet {
    fn event(_: &mut Self, _: &ZwlrVirtualPointerManagerV1, _: <ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<ZwlrVirtualPointerV1, ()> for Quiet {
    fn event(_: &mut Self, _: &ZwlrVirtualPointerV1, _: <ZwlrVirtualPointerV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WlSeat, ()> for Quiet {
    fn event(_: &mut Self, _: &WlSeat, _: <WlSeat as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n = |i: usize| a.get(i).and_then(|s| s.parse::<u32>().ok()).unwrap_or_else(|| panic!("usage: vptr <x> <y> <width> <height> [click]"));
    let (x, y, w, h) = (n(1), n(2), n(3), n(4));
    let click = a.get(5).map(String::as_str) == Some("click");
    let scroll: Option<f64> = (a.get(5).map(String::as_str) == Some("scroll")).then(|| a.get(6).and_then(|v| v.parse().ok()).expect("usage: vptr <x> <y> <w> <h> scroll <pixels>"));
    let conn = Connection::connect_to_env().expect("no WAYLAND_DISPLAY");
    let (globals, mut queue) = registry_queue_init::<Quiet>(&conn).unwrap();
    let qh = queue.handle();
    let manager: ZwlrVirtualPointerManagerV1 = globals.bind(&qh, 1..=2, ()).expect("compositor has no zwlr_virtual_pointer_manager_v1");
    let seat: Option<WlSeat> = globals.bind(&qh, 1..=1, ()).ok();
    let ptr = manager.create_virtual_pointer(seat.as_ref(), &qh, ());
    // The seat gains a pointer only while a virtual pointer exists, and the client needs a moment
    // to bind it; so wait before the first move and again before the click, and stay a moment after.
    let pause = |queue: &mut wayland_client::EventQueue<Quiet>| {
        queue.roundtrip(&mut Quiet).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
    };
    pause(&mut queue);
    ptr.motion_absolute(1, x, y, w, h);
    ptr.frame();
    pause(&mut queue);
    ptr.motion_absolute(2, x, y, w, h);
    ptr.frame();
    pause(&mut queue);
    if click {
        use wayland_client::protocol::wl_pointer::ButtonState;
        ptr.button(2, 0x110, ButtonState::Pressed); // BTN_LEFT
        ptr.frame();
        pause(&mut queue);
        ptr.button(3, 0x110, ButtonState::Released);
        ptr.frame();
    }
    if let Some(pixels) = scroll {
        use wayland_client::protocol::wl_pointer::{Axis, AxisSource};
        ptr.axis_source(AxisSource::Wheel);
        ptr.axis(4, Axis::VerticalScroll, pixels);
        ptr.axis_discrete(4, Axis::VerticalScroll, pixels, if pixels > 0.0 { 1 } else { -1 });
        ptr.frame();
        pause(&mut queue);
    }
    pause(&mut queue);
    println!("pointer at ({x},{y}) of {w}x{h}{}", if click { ", clicked" } else { "" });
}
