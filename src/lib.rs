pub mod event_loop;
pub mod seat;
pub mod state;
pub mod viewporter;
pub mod window;

pub use event_loop::{AccesskitEvents, AccesskitHandler, ApplicationHandler, Events, LoopHandler};
pub use state::WaylandState;
pub use viewporter::ViewporterState;
pub use window::{WindowCore, WaylandWindow, attributes::*, registry::WindowsRegistry};

pub mod xdg {
    pub use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_toplevel::ResizeEdge;
}
