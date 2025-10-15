use std::collections::HashMap;

use indexmap::IndexSet;
use smithay_client_toolkit::reexports::client::backend::ObjectId;

use crate::{WaylandWindow, WindowId};

#[derive(Default)]
pub struct WindowsRegistry {
    pub(crate) id_converter: HashMap<WindowId, ObjectId>,
    pub(crate) windows: HashMap<ObjectId, WaylandWindow>,
    pub(crate) rescale_request: IndexSet<ObjectId>,
    pub(crate) resize_request: IndexSet<ObjectId>,
    pub(crate) redraw_request: IndexSet<ObjectId>,
    pub(crate) close_request: IndexSet<ObjectId>,
}

impl WindowsRegistry {
    pub fn insert(&mut self, window_id: WindowId, object_id: ObjectId, window: WaylandWindow) {
        if self
            .id_converter
            .insert(window_id, object_id.clone())
            .is_some()
            || self.windows.insert(object_id, window).is_some()
        {
            panic!("Failed to add window with the existing id");
        }
    }

    pub fn remove(&mut self, object_id: &ObjectId) -> WindowId {
        if let Some(window) = self.windows.remove(object_id) {
            let id = &window.immutable.window_id;
            if let Some(_) = self.id_converter.remove(id) {
                return *id;
            }
        }
        panic!("Failed to remove window");
    }

    pub(crate) fn get_mut_by_object_id(&mut self, id: &ObjectId) -> Option<&mut WaylandWindow> {
        self.windows.get_mut(id)
    }

    pub(crate) fn get_by_object_id(&self, id: &ObjectId) -> Option<&WaylandWindow> {
        self.windows.get(id)
    }

    pub fn get_mut(&mut self, id: &WindowId) -> Option<&mut WaylandWindow> {
        if let Some(id) = self.id_converter.get(id) {
            return self.windows.get_mut(id);
        }
        None
    }

    pub fn get(&self, id: &WindowId) -> Option<&WaylandWindow> {
        self.id_converter
            .get(id)
            .and_then(|id| self.windows.get(id))
    }

    pub fn get_id(&self, object_id: &ObjectId) -> Option<&WindowId> {
        self.windows.get(object_id).map(|w| &w.immutable.window_id)
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }
}
