pub mod attributes;
pub mod locked;
pub mod registry;

use std::{
    num::NonZeroU32,
    rc::{Rc, Weak},
    sync::{Arc, LazyLock},
    time::Duration,
};

use accesskit_unix::Adapter;
use cursor_icon::CursorIcon;
use dpi::{LogicalPosition, LogicalSize, PhysicalSize, Position};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use sctk_adwaita::{AdwaitaFrame, FrameConfig};
use smithay_client_toolkit::{
    compositor::Region,
    reexports::{
        client::protocol::{wl_display::WlDisplay, wl_output::WlOutput, wl_seat::WlSeat},
        csd_frame::{FrameAction, FrameClick, ResizeEdge},
        protocols::wp::viewporter::client::wp_viewport::WpViewport,
    },
    shell::xdg::{
        XdgSurface,
        window::{DecorationMode, Window},
    },
};
use smithay_client_toolkit::{
    reexports::{
        calloop::channel::Sender as WlSender,
        protocols::xdg::shell::client::xdg_toplevel::ResizeEdge as XdgResizeEdge,
    },
    shell::WaylandSurface,
};
use smithay_client_toolkit::{
    reexports::{
        client::Proxy,
        csd_frame::{DecorationsFrame, WindowState},
    },
    shell::xdg::window::WindowConfigure,
};
use tracing::error;
use wayland_backend::client::ObjectId;

use crate::{
    Events, WaylandState, WindowAttributes, WindowId, seat::PointerKind,
    state::logical_to_physical_rounded,
};

pub(crate) static DEFAULT_WINDOW_SIZE: LazyLock<LogicalSize<u32>> =
    LazyLock::new(|| LogicalSize::from((256, 256)));

pub(crate) const DEFAULT_SCALE_FACTOR: i32 = 1;

// Minimum window surface size.
const MIN_WINDOW_SIZE: LogicalSize<u32> = LogicalSize::new(2, 1);

pub struct WindowCore {
    pub(crate) id: WindowId,
    /// The wayland display used solely for raw window handle.
    #[allow(dead_code)]
    display: WlDisplay,
}

impl WindowCore {
    pub fn new(id: WindowId, display: WlDisplay) -> Self {
        Self { id, display }
    }

    pub fn get_id(&self) -> WindowId {
        self.id.clone()
    }

    #[inline]
    pub(crate) fn raw_window_handle_rwh_06(&self) -> Result<RawWindowHandle, HandleError> {
        Ok(WaylandWindowHandle::new({
            let ptr = ObjectId::from(self.id.clone()).as_ptr();
            std::ptr::NonNull::new(ptr as *mut _).expect("wl_surface will never be null")
        })
        .into())
    }

    #[inline]
    pub(crate) fn raw_display_handle_rwh_06(&self) -> Result<RawDisplayHandle, HandleError> {
        Ok(WaylandDisplayHandle::new({
            let ptr = self.display.id().as_ptr();
            std::ptr::NonNull::new(ptr as *mut _).expect("wl_proxy should never be null")
        })
        .into())
    }
}

/// New window
pub struct WaylandWindow {
    pub core: Arc<WindowCore>,
    pub(crate) window: Window,
    pub(crate) title: String,
    pub(crate) visible: bool,
    pub(crate) resizable: bool,
    pub(crate) hide_titlebar: bool,
    pub(crate) decorations: bool,
    pub(crate) transparent: bool,
    pub(crate) light_theme: Option<bool>,
    pub(crate) state: WindowState,
    pub(crate) window_frame: Option<AdwaitaFrame<WaylandState>>,
    pub(crate) output: Option<WlOutput>,
    pub(crate) viewport: Option<WpViewport>,
    pub(crate) size: LogicalSize<u32>,
    /// Min size.
    pub(crate) min_surface_size: LogicalSize<u32>,
    pub(crate) max_surface_size: Option<LogicalSize<u32>>,
    pub(crate) stateless_size: LogicalSize<u32>,
    pub scale_factor: i32,
    pub(crate) event_sender: WlSender<Events>,
    pub accesskit_adapter: Adapter,
    pub(crate) decorate: bool,
    pub(crate) region: Option<Region>,
    pub(crate) stateless: bool,
    /// The pointers observed on the window.
    pub(crate) pointers: Vec<Weak<PointerKind>>,
    pub(crate) selected_cursor: CursorIcon,
    /// Whether the cursor is visible.
    pub(crate) cursor_visible: bool,
}

impl WaylandWindow {
    pub(crate) fn new(
        immutable: Arc<WindowCore>,
        window: Window,
        last_output: Option<&WlOutput>,
        attr: WindowAttributes,
        event_sender: WlSender<Events>,
        accesskit_adapter: Adapter,
        region: Option<Region>,
        viewport: Option<WpViewport>,
    ) -> Self {
        // Set the app_id.
        if let Some(name) = attr.app_name.map(|name| name.general) {
            window.set_app_id(name);
        }

        if attr.maximized {
            window.set_maximized();
        }

        if attr.fullscreen {
            window.set_fullscreen(last_output);
        }

        let mut state = Self {
            core: immutable,
            window,
            state: WindowState::empty(),
            window_frame: None,
            output: None,
            viewport,
            size: DEFAULT_WINDOW_SIZE.to_owned(),
            stateless_size: DEFAULT_WINDOW_SIZE.to_owned(),
            scale_factor: DEFAULT_SCALE_FACTOR,
            event_sender,
            accesskit_adapter,
            decorate: true,
            region,
            transparent: false,
            stateless: false,
            pointers: Vec::new(),
            selected_cursor: Default::default(),
            cursor_visible: true,
            title: attr.title,
            visible: attr.visible,
            resizable: attr.resizable,
            hide_titlebar: attr.hide_titlebar,
            decorations: attr.decorations,
            light_theme: attr.light_theme,
            min_surface_size: MIN_WINDOW_SIZE,
            max_surface_size: None,
        };

        if state.decorations {
            // TODO: do we need to make this request or not?
            state
                .window
                .request_decoration_mode(Some(DecorationMode::Server));
        }

        state.set_min_surface_size(attr.min_surface_size.map(|s| s.to_logical(1.0)));
        state.set_max_surface_size(attr.max_surface_size.map(|s| s.to_logical(1.0)));

        state.size = attr
            .surface_size
            .map(|s| s.to_logical(DEFAULT_SCALE_FACTOR as f64))
            .unwrap_or(DEFAULT_WINDOW_SIZE.to_owned())
            .max(state.min_surface_size);

        state
    }

    /// Set maximum inner window size.
    pub fn set_min_surface_size(&mut self, size: Option<LogicalSize<u32>>) {
        // Ensure that the window has the right minimum size.
        let mut size = size.unwrap_or(MIN_WINDOW_SIZE);
        size.width = size.width.max(MIN_WINDOW_SIZE.width);
        size.height = size.height.max(MIN_WINDOW_SIZE.height);

        // Add the borders.
        let size = self
            .window_frame
            .as_ref()
            .map(|frame| frame.add_borders(size.width, size.height).into())
            .unwrap_or(size);

        self.min_surface_size = size;
        self.window.set_min_size(Some(size.into()));
    }

    /// Set maximum inner window size.
    pub fn set_max_surface_size(&mut self, size: Option<LogicalSize<u32>>) {
        let size = size.map(|size| {
            self.window_frame
                .as_ref()
                .map(|frame| frame.add_borders(size.width, size.height).into())
                .unwrap_or(size)
        });

        self.max_surface_size = size;
        self.window.set_max_size(size.map(Into::into));
    }

    pub fn frame_config(&self) -> FrameConfig {
        let config = match self.light_theme {
            Some(true) => FrameConfig::light(),
            Some(false) => FrameConfig::dark(),
            None => FrameConfig::auto(),
        };
        config.hide_titlebar(self.hide_titlebar)
    }

    /// Create a new [`WindowAttributes`] which allows modifying the window's attributes before
    /// creation.
    #[inline]
    pub fn default_attributes() -> WindowAttributes {
        WindowAttributes::default()
    }

    pub fn get_surface_id(&self) -> &WindowId {
        &self.core.id
    }

    pub fn redraw_request(&self) {
        if let Err(err) = self
            .event_sender
            .send(Events::RedrawRequest(self.core.id.clone()))
        {
            error!("{err}");
        }
    }

    #[inline]
    pub fn set_cursor(&mut self, cursor: CursorIcon) {
        self.selected_cursor = cursor;
    }

    /// Whether show or hide client side decorations.
    #[inline]
    pub fn set_decorate(&mut self, decorate: bool) {
        if decorate == self.decorate {
            return;
        }

        self.decorate = decorate;

        if self.decorate {
            self.window
                .request_decoration_mode(Some(DecorationMode::Server));
        }

        if let Some(frame) = self.window_frame.as_mut() {
            frame.set_hidden(!decorate);
            // Force the resize.
            self.resize(self.size);
        }
    }

    /// Set the window title to a new value.
    ///
    /// This will automatically truncate the title to something meaningful.
    pub fn set_title(&mut self, mut title: String) {
        // Truncate the title to at most 1024 bytes, so that it does not blow up the protocol
        // messages
        if title.len() > 1024 {
            let mut new_len = 1024;
            while !title.is_char_boundary(new_len) {
                new_len -= 1;
            }
            title.truncate(new_len);
        }

        // Update the CSD title.
        if let Some(frame) = self.window_frame.as_mut() {
            frame.set_title(&title);
        }

        self.window.set_title(&title);
        self.title = title;
    }

    /// Mark the window as transparent.
    #[inline]
    pub fn set_transparent(&mut self, transparent: bool) {
        self.transparent = transparent;
        self.reload_transparency_hint();
    }

    /// Try to resize the window when the user can do so.
    pub fn request_inner_size(&mut self, inner_size: PhysicalSize<u32>) -> PhysicalSize<u32> {
        if self.stateless {
            self.resize(inner_size.to_logical(self.scale_factor as f64))
        }

        logical_to_physical_rounded(self.size, self.scale_factor as f64)
    }

    pub fn apply_on_pointer(&self, f: impl Fn(Rc<PointerKind>)) {
        self.pointers
            .iter()
            .filter_map(Weak::upgrade)
            .for_each(|p| {
                f(p);
            });
    }

    /// Start the window drag.
    pub fn drag_window(&self) {
        let xdg_toplevel = self.window.xdg_toplevel();
        self.apply_on_pointer(|pointer| {
            if let (Some(serial), Some(seat)) = (pointer.latest_serial(), pointer.seat()) {
                xdg_toplevel._move(seat, serial);
            }
        });
    }

    /// Start interacting drag resize.
    pub fn drag_resize_window(&self, direction: XdgResizeEdge) {
        let xdg_toplevel = self.window.xdg_toplevel();

        self.apply_on_pointer(|pointer| {
            if let (Some(serial), Some(seat)) = (pointer.latest_serial(), pointer.seat()) {
                xdg_toplevel.resize(seat, serial, direction);
            }
        });
    }

    pub fn show_window_menu(&self, position: impl Into<Position>) {
        let position: Position = position.into();
        let position: LogicalPosition<u32> = position.to_logical(self.scale_factor as f64);
        self.apply_on_pointer(|pointer| {
            if let (Some(serial), Some(seat)) = (pointer.latest_serial(), pointer.seat()) {
                self.window.show_window_menu(seat, serial, position.into());
            }
        });
    }

    #[inline]
    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    #[inline]
    pub fn set_visible(&self, _visible: bool) {
        // Not possible on Wayland.
    }

    /// Is [`WindowState::FULLSCREEN`] state is set.
    #[inline]
    pub fn is_fullscreen(&self) -> bool {
        self.state.contains(WindowState::FULLSCREEN)
    }

    /// Is [`WindowState::FULLSCREEN`] state is set.
    #[inline]
    pub fn set_fullscreen(&self) {
        self.window.set_fullscreen(self.output.as_ref());
    }

    #[inline]
    pub fn is_minimized(&self) -> Option<bool> {
        // XXX clients don't know whether they are minimized or not.
        None
    }

    #[inline]
    pub fn set_minimized(&self) {
        // You can't unminimize the window on Wayland.
        self.window.set_minimized();
    }

    #[inline]
    pub fn is_maximized(&self) -> bool {
        self.state.contains(WindowState::MAXIMIZED)
    }

    #[inline]
    pub fn set_maximized(&self, maximized: bool) {
        if maximized {
            self.window.set_maximized()
        } else {
            self.window.unset_maximized()
        }
    }

    pub fn pointer_enter(&mut self, pointer: Rc<PointerKind>) {
        self.pointers.push(Rc::downgrade(&pointer));
    }

    pub fn pointer_leave(&mut self, pointer: Rc<PointerKind>) {
        self.pointers
            .retain(|p| !p.ptr_eq(&Rc::downgrade(&pointer)));
    }

    pub(crate) fn on_frame_action(
        &mut self,
        pressed: bool,
        click: FrameClick,
        seat: &WlSeat,
        time: u32,
        serial: u32,
    ) -> bool {
        if let Some(action) = self
            .window_frame
            .as_mut()
            .and_then(|frame| frame.on_click(Duration::from_millis(time as u64), click, pressed))
        {
            return self.frame_action(seat, serial, action);
        }
        false
    }

    pub fn frame_action(&mut self, seat: &WlSeat, serial: u32, action: FrameAction) -> bool {
        match action {
            FrameAction::Close => return true,
            FrameAction::Minimize => self.window.set_minimized(),
            FrameAction::Maximize => self.window.set_maximized(),
            FrameAction::UnMaximize => self.window.unset_maximized(),
            FrameAction::ShowMenu(x, y) => self.window.show_window_menu(seat, serial, (x, y)),
            FrameAction::Resize(edge) => {
                let edge = match edge {
                    ResizeEdge::None => XdgResizeEdge::None,
                    ResizeEdge::Top => XdgResizeEdge::Top,
                    ResizeEdge::Bottom => XdgResizeEdge::Bottom,
                    ResizeEdge::Left => XdgResizeEdge::Left,
                    ResizeEdge::TopLeft => XdgResizeEdge::TopLeft,
                    ResizeEdge::BottomLeft => XdgResizeEdge::BottomLeft,
                    ResizeEdge::Right => XdgResizeEdge::Right,
                    ResizeEdge::TopRight => XdgResizeEdge::TopRight,
                    ResizeEdge::BottomRight => XdgResizeEdge::BottomRight,
                    _ => return false,
                };
                self.window.resize(seat, serial, edge);
            }
            FrameAction::Move => self.window.move_(seat, serial),
            _ => (),
        }
        false
    }

    /// Compute the bounds for the surface size of the surface.
    pub(crate) fn surface_size_bounds(
        &self,
        configure: &WindowConfigure,
    ) -> (Option<NonZeroU32>, Option<NonZeroU32>) {
        let configure_bounds = match configure.suggested_bounds {
            Some((width, height)) => (NonZeroU32::new(width), NonZeroU32::new(height)),
            None => (None, None),
        };

        if let Some(frame) = self.window_frame.as_ref() {
            let (width, height) = frame.subtract_borders(
                configure_bounds.0.unwrap_or(NonZeroU32::new(1).unwrap()),
                configure_bounds.1.unwrap_or(NonZeroU32::new(1).unwrap()),
            );
            (
                configure_bounds.0.and(width),
                configure_bounds.1.and(height),
            )
        } else {
            configure_bounds
        }
    }

    /// Resize the window to the new surface size.
    pub(crate) fn resize(&mut self, surface_size: LogicalSize<u32>) {
        self.size = surface_size;

        // Update the stateless size.
        if self.stateless {
            self.stateless_size = surface_size;
        }

        // Update the inner frame.
        let ((x, y), outer_size) = if let Some(frame) = self.window_frame.as_mut() {
            // Resize only visible frame.
            if !frame.is_hidden() {
                frame.resize(
                    NonZeroU32::new(self.size.width).unwrap(),
                    NonZeroU32::new(self.size.height).unwrap(),
                );
            }

            (
                frame.location(),
                frame.add_borders(self.size.width, self.size.height).into(),
            )
        } else {
            ((0, 0), self.size)
        };

        // Reload the hint.
        self.reload_transparency_hint();

        // Set the window geometry.
        self.window.xdg_surface().set_window_geometry(
            x,
            y,
            outer_size.width as i32,
            outer_size.height as i32,
        );

        // Update the target viewport, this is used if and only if fractional scaling is in use.
        if let Some(viewport) = self.viewport.as_ref() {
            // Set surface size without the borders.
            viewport.set_destination(self.size.width as _, self.size.height as _);
        }
    }

    /// Reissue the transparency hint to the compositor.
    pub fn reload_transparency_hint(&self) {
        let surface = self.window.wl_surface();

        if self.transparent {
            surface.set_opaque_region(None);
        } else if let Some(region) = self.region.as_ref() {
            region.add(0, 0, i32::MAX, i32::MAX);
            surface.set_opaque_region(Some(region.wl_region()));
        } else {
            error!("Failed to mark window opaque.");
        }
    }

    /// Refresh the decorations frame if it's present returning whether the client should redraw.
    pub fn refresh_frame(&mut self) -> bool {
        if let Some(frame) = self.window_frame.as_mut() {
            if !frame.is_hidden() && frame.is_dirty() {
                return frame.draw();
            }
        }
        false
    }
}

impl HasWindowHandle for WindowCore {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = self.raw_window_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for WindowCore {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = self.raw_display_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}
