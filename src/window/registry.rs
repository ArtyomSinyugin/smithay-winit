use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use dpi::LogicalSize;
use indexmap::{IndexMap, IndexSet};
use wayland_backend::client::ObjectId;

use crate::{WaylandWindow, WindowCore, WindowId, window::locked::ScreenLock};

#[derive(Default)]
pub struct WindowsRegistry {
    pub(crate) windows: HashMap<WindowId, WaylandWindow>,
    pub(crate) new_windows: Vec<Arc<WindowCore>>,
    pub(crate) screenlocks: HashMap<WindowId, ScreenLock>,
    pub(crate) new_screenlock: IndexMap<WindowId, (Option<LogicalSize<u32>>, Weak<WindowCore>)>,
    pub(crate) rescale_request: IndexSet<WindowId>,
    pub(crate) resize_request: IndexSet<WindowId>,
    pub(crate) redraw_request: IndexSet<WindowId>,
    pub(crate) close_request: IndexSet<WindowId>,
}

impl WindowsRegistry {
    pub fn insert_window(&mut self, id: WindowId, window: WaylandWindow) {
        if self.windows.insert(id, window).is_some() {
            panic!("Failed to add window with the existing id");
        }
    }

    pub fn remove_window(&mut self, id: &WindowId) -> WindowId {
        if let Some(window) = self.windows.remove(id) {
            return window.core.id.clone();
        }
        panic!("Failed to remove window");
    }

    pub fn insert_screenlock(&mut self, id: WindowId, screenlock: ScreenLock) {
        if self.screenlocks.insert(id, screenlock).is_some() {
            panic!("Failed to add window with the existing id");
        }
    }

    pub fn remove_screenlock(&mut self, id: &WindowId) {
        self.screenlocks.remove(id);
        if self.new_screenlock.swap_remove(id).is_none() {
            // Ask app to close screenlock surface
            self.close_request.insert(id.clone());
        }
    }

    pub fn remove_screenlock_by_output_id(&mut self, output_id: &ObjectId) {
        let mut id = ObjectId::null().into();
        self.screenlocks.retain(|_, locked| {
            if &locked.output_id != output_id {
                id = locked.get_id();
                false
            } else {
                true
            }
        });

        if self.new_screenlock.swap_remove(&id).is_none() {
            // Ask app to close screenlock surface
            self.close_request.insert(id);
        }
    }

    // TODO: somehow we should get WaylandWindow or ScreenLock depending on the global LOCKED state
    pub fn get_mut(&mut self, id: &WindowId) -> Option<&mut WaylandWindow> {
        self.windows.get_mut(id)
    }

    // TODO: somehow we should get WaylandWindow or ScreenLock depending on the global LOCKED state
    pub fn get(&self, id: &WindowId) -> Option<&WaylandWindow> {
        self.windows.get(id)
    }

    // TODO: somehow we should get WaylandWindow or ScreenLock depending on the global LOCKED state
    pub fn get_locked_mut(&mut self, id: &WindowId) -> Option<&mut ScreenLock> {
        self.screenlocks.get_mut(id)
    }

    // TODO: somehow we should get WaylandWindow or ScreenLock depending on the global LOCKED state
    pub fn get_locked(&self, id: &WindowId) -> Option<&ScreenLock> {
        self.screenlocks.get(id)
    }

    // TODO: it is troublesome to check both maps for emptiness... So, doublecheck is needed
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty() && self.screenlocks.is_empty()
    }
}
