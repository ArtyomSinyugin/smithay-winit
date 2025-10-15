// Handling of the wp-viewporter.
// Code from winit (https://github.com/rust-windowing/winit)
// Copyright (c) 2015-2024 The winit Contributors

use smithay_client_toolkit::{
    globals::GlobalData,
    reexports::{
        client::{
            Connection, Dispatch, Proxy, QueueHandle, delegate_dispatch,
            globals::{BindError, GlobalList},
            protocol::wl_surface::WlSurface,
        },
        protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
    },
};

use crate::WaylandState;

/// Viewporter.
#[derive(Debug)]
pub struct ViewporterState {
    viewporter: WpViewporter,
}

impl ViewporterState {
    /// Create new viewporter.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WaylandState>,
    ) -> Result<Self, BindError> {
        let viewporter = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { viewporter })
    }

    /// Get the viewport for the given object.
    pub fn get_viewport(
        &self,
        surface: &WlSurface,
        queue_handle: &QueueHandle<WaylandState>,
    ) -> WpViewport {
        self.viewporter
            .get_viewport(surface, queue_handle, GlobalData)
    }
}

impl Dispatch<WpViewporter, GlobalData, WaylandState> for ViewporterState {
    fn event(
        _: &mut WaylandState,
        _: &WpViewporter,
        _: <WpViewporter as Proxy>::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<WaylandState>,
    ) {
        // No events.
    }
}
impl Dispatch<WpViewport, GlobalData, WaylandState> for ViewporterState {
    fn event(
        _: &mut WaylandState,
        _: &WpViewport,
        _: <WpViewport as Proxy>::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<WaylandState>,
    ) {
        // No events.
    }
}

delegate_dispatch!(WaylandState: [WpViewporter: GlobalData] => ViewporterState);
delegate_dispatch!(WaylandState: [WpViewport: GlobalData] => ViewporterState);
