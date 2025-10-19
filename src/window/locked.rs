use accesskit_unix::Adapter;
use dpi::LogicalSize;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::{
    reexports::client::{Proxy, protocol::wl_display::WlDisplay},
    session_lock::SessionLockSurface,
};
use wayland_backend::client::ObjectId;

use crate::WindowId;

pub struct LockedSurface {
    pub window_id: WindowId,
    pub(crate) lock_surface: SessionLockSurface,
    pub(crate) display: WlDisplay,
    pub accesskit_adapter: Adapter,
    pub size: Option<LogicalSize<u32>>,
}

impl LockedSurface {
    pub fn new(
        window_id: WindowId,
        lock_surface: SessionLockSurface,
        display: WlDisplay,
        accesskit_adapter: Adapter,
    ) -> Self {
        Self {
            window_id,
            lock_surface,
            display,
            accesskit_adapter,
            size: None,
        }
    }

    #[inline]
    pub fn get_id(&self) -> ObjectId {
        self.lock_surface.wl_surface().id()
    }

    #[inline]
    pub(crate) fn raw_window_handle_rwh_06(&self) -> Result<RawWindowHandle, HandleError> {
        Ok(WaylandWindowHandle::new({
            let ptr = self.get_id().as_ptr();
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

impl HasWindowHandle for LockedSurface {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = self.raw_window_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for LockedSurface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = self.raw_display_handle_rwh_06()?;

        // SAFETY: The window handle will never be deallocated while the window is alive,
        // and the main thread safety requirements are upheld internally by each platform.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}
