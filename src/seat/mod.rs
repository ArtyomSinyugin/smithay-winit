use std::{collections::HashMap, rc::Rc};

use cursor_icon::CursorIcon;
use smithay_client_toolkit::{
    reexports::client::{
        Connection, Proxy, QueueHandle,
        backend::ObjectId,
        protocol::{wl_keyboard::WlKeyboard, wl_seat::WlSeat, wl_touch::WlTouch},
    },
    seat::{
        Capability, SeatHandler, SeatState as WlSeatState,
        pointer::{PointerData, ThemeSpec, ThemedPointer},
        touch::TouchData,
    },
};
use tracing::{error, warn};
use ui_events::{
    keyboard::Modifiers,
    pointer::{PointerEvent, PointerId, PointerInfo, PointerType},
};

use crate::{Events, WaylandState};

pub mod keyboard;
pub mod pointer;
pub mod touch;

pub type WlSeatId = ObjectId;
pub type WlPointerId = ObjectId;

#[derive(Debug)]
pub struct SeatState {
    /// The seat state responsible for all sorts of input.
    pub seat: WlSeatState,
    pub modifiers: Modifiers,
    pub pointers: PointerRegistry,
    pub keyboard: Option<WlKeyboard>,
    pub keyboard_focus: Option<ObjectId>,
}

impl SeatState {
    pub fn new(state: WlSeatState) -> Self {
        Self {
            seat: state,
            modifiers: Modifiers::default(),
            pointers: PointerRegistry::default(),
            keyboard: None,
            keyboard_focus: None,
        }
    }
}

#[derive(Debug)]
pub enum PointerKind {
    Mouse(ThemedPointer),
    Touch(WlTouch),
}

impl PointerKind {
    pub fn set(&self) -> Result<(), String> {
        match self {
            PointerKind::Mouse(themed_pointer) => {
                themed_pointer.hide_cursor().map_err(|err| err.to_string())
            }
            _ => Err(String::from("Icons unsupported for touch")),
        }
    }

    pub fn set_cursor(&self, conn: &Connection, icon: CursorIcon) -> Result<(), String> {
        match self {
            PointerKind::Mouse(themed_pointer) => themed_pointer
                .set_cursor(conn, icon)
                .map_err(|err| err.to_string()),
            _ => Err(String::from("Icons unsupported for touch")),
        }
    }

    pub fn latest_serial(&self) -> Option<u32> {
        match self {
            PointerKind::Mouse(themed_pointer) => {
                let data = themed_pointer.pointer().data::<PointerData>()?;
                data.latest_button_serial()
            }
            PointerKind::Touch(wl_touch) => {
                let data = wl_touch.data::<TouchData>()?;
                data.latest_down_serial()
            }
        }
    }

    pub fn seat(&self) -> Option<&WlSeat> {
        match self {
            PointerKind::Mouse(themed_pointer) => {
                let data = themed_pointer.pointer().data::<PointerData>()?;
                Some(data.seat())
            }
            PointerKind::Touch(wl_touch) => {
                let data = wl_touch.data::<TouchData>()?;
                Some(data.seat())
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct PointerRegistry {
    by_seat: HashMap<WlSeatId, (WlPointerId, Rc<PointerKind>)>,
    by_pointer: HashMap<WlPointerId, (WlSeatId, PointerInfo)>,
}

impl PointerRegistry {
    pub fn add(
        &mut self,
        seat_id: ObjectId,
        pointer_id: ObjectId,
        pointer: PointerKind,
        info: PointerInfo,
    ) {
        self.by_seat
            .insert(seat_id.clone(), (pointer_id.clone(), Rc::new(pointer)));
        self.by_pointer.insert(pointer_id.clone(), (seat_id, info));
    }

    pub fn remove(&mut self, seat_id: ObjectId) -> Option<PointerInfo> {
        let pointer = self.by_seat.remove(&seat_id);
        if let Some((id, pointer)) = pointer {
            let _ = self.by_pointer.remove(&id);
            match pointer.as_ref() {
                PointerKind::Mouse(wl_pointer) => {
                    wl_pointer.pointer().release();
                    // TODO: do we need destroy pointer surface this way?
                    wl_pointer.surface().destroy();
                }
                PointerKind::Touch(wl_touch) => wl_touch.release(),
            }
            return self.by_pointer.get(&id).map(|(_, info)| info).copied();
        }
        None
    }

    pub fn kind(&self, seat_id: ObjectId) -> Option<Rc<PointerKind>> {
        self.by_seat.get(&seat_id).map(|k| k.1.clone())
    }

    pub fn info(&self, pointer_id: ObjectId) -> Option<PointerInfo> {
        self.by_pointer
            .get(&pointer_id)
            .map(|(_, info)| info)
            .copied()
    }
}

impl SeatHandler for WaylandState {
    fn seat_state(&mut self) -> &mut WlSeatState {
        &mut self.seat_state.seat
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer => {
                let surface = self.compositor_state.create_surface(qh);

                if let Ok(pointer) = self.seat_state.seat.get_pointer_with_theme_and_data(
                    qh,
                    &seat,
                    self.shm.wl_shm(),
                    surface,
                    ThemeSpec::System,
                    PointerData::new(seat.clone()),
                ) {
                    let pointer_id = pointer.pointer().id();
                    let info = PointerInfo {
                        pointer_id: Some(PointerId::new(pointer_id.protocol_id() as u64).unwrap()),
                        persistent_device_id: None,
                        pointer_type: PointerType::Mouse,
                    };
                    self.seat_state.pointers.add(
                        seat.id(),
                        pointer_id,
                        PointerKind::Mouse(pointer),
                        info,
                    );
                }
            }
            Capability::Touch => {
                if let Ok(touch) = self.seat_state.seat.get_touch(qh, &seat) {
                    let touch_id = touch.id();
                    let info = PointerInfo {
                        pointer_id: Some(PointerId::new(touch_id.protocol_id() as u64).unwrap()),
                        persistent_device_id: None,
                        pointer_type: PointerType::Mouse,
                    };
                    self.seat_state.pointers.add(
                        seat.id(),
                        touch_id,
                        PointerKind::Touch(touch),
                        info,
                    );
                }
            }
            Capability::Keyboard if self.seat_state.keyboard.is_none() => {
                if let Ok(keyboard) = self.seat_state.seat.get_keyboard(qh, &seat, None) {
                    self.seat_state.keyboard = Some(keyboard);
                }
            }
            _ => {
                error!("Could not recognize unknown capability");
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        seat: WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard if self.seat_state.keyboard.is_some() => {
                if let Some(id) = self.seat_state.keyboard_focus.take() {
                    if let Err(err) = self.event_sender.send(Events::Focus(id, false)) {
                        error!("{err}");
                    };
                }
                self.seat_state.keyboard.take().unwrap().release()
            }
            Capability::Pointer | Capability::Touch => {
                if let Some(info) = self.seat_state.pointers.remove(seat.id()) {
                    for (id, _) in &self.windows.windows {
                        if let Err(err) = self
                            .event_sender
                            .send(Events::Pointer(id.clone(), PointerEvent::Cancel(info)))
                        {
                            error!("Failed to remove capability for window: {id}\n{err}");
                        }
                    }
                } else {
                    warn!("Could not remote unknown capability for {}", seat.id());
                }
            }
            _ => {}
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: WlSeat) {}
}
