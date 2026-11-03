//! The window's state machine: when it paints, when it waits, what it does with no frame callback.

use std::time::Duration;

pub const COLOURS: [u32; 2] = [0xFFE0_A030, 0xFF30_80E0];
pub const DEFAULT_SIZE: (u32, u32) = (400, 300);
/// A frame callback that has not come after this long means the compositor is not showing us.
pub const OVERDUE: Duration = Duration::from_millis(1000);

/// Where the window is in asking for frames. A commit asks for a callback; until it comes the
/// window does not paint again, and changes that arrive meanwhile wait as one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pacing {
    /// Nothing changed and no callback is owed: the loop sleeps.
    Quiet,
    /// A callback is owed; no change has come since.
    Pending,
    /// A callback is owed and a change is waiting.
    PendingChanged,
    /// The callback is overdue: presumed hidden. Nothing paints, nothing spins.
    Stalled,
    /// Stalled, and a change is waiting for the callback to come back.
    StalledChanged,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    /// Asked the compositor for a surface; no size has arrived yet. The colour is what it will show.
    Waiting { colour: usize },
    /// The compositor took the surface away (an output was unplugged); we need a new one.
    Gone { colour: usize },
    Showing { w: u32, h: u32, colour: usize, pacing: Pacing },
    Closed,
}

pub enum Event {
    Configured { w: Option<u32>, h: Option<u32> },
    /// The content changed (here: the other colour).
    Changed,
    FrameCallback,
    /// A callback has been owed for `OVERDUE`.
    CallbackOverdue,
    CloseRequested,
    /// The compositor destroyed the surface (layer-shell `closed`).
    SurfaceClosed,
    /// An output became available.
    OutputAppeared,
    /// A new surface was made after `Gone`.
    Recreated,
}

#[derive(Debug, PartialEq)]
pub enum Effect {
    /// Draw at this size in this colour, and ask for a frame callback with the commit.
    Paint { w: u32, h: u32, colour: usize },
    Nothing,
    /// Make a new surface on some output if one exists; say `Recreated` when it does.
    TryRecreate,
    Exit,
}

pub fn step(state: State, event: Event) -> (State, Effect) {
    use Pacing::*;
    match (state, event) {
        (State::Closed, _) => (State::Closed, Effect::Nothing),
        (_, Event::CloseRequested) => (State::Closed, Effect::Exit),
        (State::Waiting { colour } | State::Showing { colour, .. }, Event::SurfaceClosed) => (State::Gone { colour }, Effect::TryRecreate),
        (State::Gone { colour }, Event::OutputAppeared) => (State::Gone { colour }, Effect::TryRecreate),
        (State::Gone { colour }, Event::Recreated) => (State::Waiting { colour }, Effect::Nothing),
        (State::Gone { colour }, Event::Changed) => (State::Gone { colour: (colour + 1) % COLOURS.len() }, Effect::Nothing),
        (State::Gone { .. }, _) => (state, Effect::Nothing),
        (State::Waiting { colour }, Event::Configured { w, h }) => {
            let (w, h) = (w.unwrap_or(DEFAULT_SIZE.0), h.unwrap_or(DEFAULT_SIZE.1));
            (State::Showing { w, h, colour, pacing: Pending }, Effect::Paint { w, h, colour })
        }
        (State::Waiting { colour }, Event::Changed) => (State::Waiting { colour: (colour + 1) % COLOURS.len() }, Effect::Nothing),
        (State::Waiting { .. }, _) => (state, Effect::Nothing),
        // A configure that changes only window state (focus, activated) needs no new buffer.
        (State::Showing { w, h, .. }, Event::Configured { w: nw, h: nh }) if nw.unwrap_or(w) == w && nh.unwrap_or(h) == h => (state, Effect::Nothing),
        // A new size must be answered with a buffer of that size whatever the pacing.
        (State::Showing { w, h, colour, .. }, Event::Configured { w: nw, h: nh }) => {
            let (nw, nh) = (nw.unwrap_or(w), nh.unwrap_or(h));
            (State::Showing { w: nw, h: nh, colour, pacing: Pending }, Effect::Paint { w: nw, h: nh, colour })
        }
        (State::Showing { w, h, colour, pacing }, Event::Changed) => {
            let colour = (colour + 1) % COLOURS.len();
            match pacing {
                Quiet => (State::Showing { w, h, colour, pacing: Pending }, Effect::Paint { w, h, colour }),
                Pending | PendingChanged => (State::Showing { w, h, colour, pacing: PendingChanged }, Effect::Nothing),
                Stalled | StalledChanged => (State::Showing { w, h, colour, pacing: StalledChanged }, Effect::Nothing),
            }
        }
        (State::Showing { w, h, colour, pacing }, Event::FrameCallback) => match pacing {
            PendingChanged | StalledChanged => (State::Showing { w, h, colour, pacing: Pending }, Effect::Paint { w, h, colour }),
            Pending | Stalled => (State::Showing { w, h, colour, pacing: Quiet }, Effect::Nothing),
            Quiet => (state, Effect::Nothing),
        },
        (State::Showing { w, h, colour, pacing }, Event::CallbackOverdue) => match pacing {
            Pending => (State::Showing { w, h, colour, pacing: Stalled }, Effect::Nothing),
            PendingChanged => (State::Showing { w, h, colour, pacing: StalledChanged }, Effect::Nothing),
            Quiet | Stalled | StalledChanged => (state, Effect::Nothing),
        },
        (State::Showing { .. }, Event::OutputAppeared | Event::Recreated) => (state, Effect::Nothing),
    }
}

pub fn describe(s: State) -> String {
    match s {
        State::Showing { pacing, colour, .. } => format!("{pacing:?}/c{colour}"),
        other => format!("{other:?}"),
    }
}
