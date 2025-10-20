use std::sync::{Arc, Weak};

use accesskit_unix::Adapter;
use cursor_icon::CursorIcon;
use dpi::LogicalSize;
use smithay_client_toolkit::{
    reexports::calloop::channel::Sender, session_lock::SessionLockSurface,
};
use tracing::error;
use wayland_backend::client::ObjectId;

use crate::{Events, WindowCore, WindowId};

pub struct ScreenLock {
    pub core: Arc<WindowCore>,
    pub(crate) _lock_surface: SessionLockSurface,
    pub(crate) output_id: ObjectId,
    pub accesskit_adapter: Adapter,
    pub size: Option<LogicalSize<u32>>,
    pub(crate) selected_cursor: CursorIcon,
    /// Whether the cursor is visible.
    pub(crate) cursor_visible: bool,
    pub(crate) event_sender: Sender<Events>,
}

impl ScreenLock {
    pub fn new(
        core: Arc<WindowCore>,
        _lock_surface: SessionLockSurface,
        output_id: ObjectId,
        accesskit_adapter: Adapter,
        event_sender: Sender<Events>,
    ) -> Self {
        Self {
            core,
            _lock_surface,
            output_id,
            accesskit_adapter,
            size: None,
            selected_cursor: Default::default(),
            cursor_visible: true,
            event_sender,
        }
    }

    #[inline]
    pub fn set_cursor(&mut self, cursor: CursorIcon) {
        self.selected_cursor = cursor;
    }

    #[inline]
    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    #[inline]
    pub fn get_id(&self) -> WindowId {
        self.core.get_id()
    }

    #[inline]
    pub fn get_output_id(&self) -> ObjectId {
        self.output_id.clone()
    }

    #[inline]
    pub fn get_core(&self) -> Weak<WindowCore> {
        Arc::downgrade(&self.core)
    }

    pub fn redraw_request(&self) {
        if let Err(err) = self
            .event_sender
            .send(Events::RedrawRequest(self.core.id.clone()))
        {
            error!("{err}");
        }
    }
}
