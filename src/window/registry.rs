use std::{collections::HashMap, sync::Arc};

use indexmap::IndexSet;

use crate::{WaylandWindow, WindowCore, WindowId};

#[derive(Default)]
pub struct WindowsRegistry {
    pub(crate) windows: HashMap<WindowId, WaylandWindow>,
    pub(crate) new_windows: Vec<Arc<WindowCore>>,
    pub(crate) new_locked_windows: Vec<WindowCore>,
    pub(crate) rescale_request: IndexSet<WindowId>,
    pub(crate) resize_request: IndexSet<WindowId>,
    pub(crate) redraw_request: IndexSet<WindowId>,
    pub(crate) close_request: IndexSet<WindowId>,
}

impl WindowsRegistry {
    pub fn insert(&mut self, id: WindowId, window: WaylandWindow) {
        if self.windows.insert(id, window).is_some() {
            panic!("Failed to add window with the existing id");
        }
    }

    pub fn remove(&mut self, id: &WindowId) -> WindowId {
        if let Some(window) = self.windows.remove(id) {
            return window.core.id.clone();
        }
        panic!("Failed to remove window");
    }

    pub fn get_mut(&mut self, id: &WindowId) -> Option<&mut WaylandWindow> {
        self.windows.get_mut(id)
    }

    pub fn get(&self, id: &WindowId) -> Option<&WaylandWindow> {
        self.windows.get(id)
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }
}
