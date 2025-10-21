use dpi::LogicalPosition;
use smithay_client_toolkit::{
    compositor::SurfaceData,
    reexports::{
        client::{
            Connection, Proxy, QueueHandle,
            protocol::{wl_surface::WlSurface, wl_touch::WlTouch},
        },
        csd_frame::FrameClick,
    },
    seat::touch::{TouchData, TouchHandler},
};
use ui_events::pointer::{
    ContactGeometry, PointerButton, PointerEvent, PointerId, PointerInfo, PointerOrientation,
    PointerState, PointerType, PointerUpdate,
};

use crate::{Events, WaylandState, WindowId};

#[derive(Debug)]
pub(crate) struct TouchState {
    window_id: WindowId,
    frame_touch: bool,
    scale_factor: i32,
    state: PointerState,
}

impl TouchState {
    pub(crate) fn new(surface_id: WindowId, scale_factor: i32, state: PointerState) -> Self {
        Self {
            window_id: surface_id,
            frame_touch: false,
            scale_factor,
            state,
        }
    }

    pub(crate) fn get_mut_state(&mut self) -> &mut PointerState {
        &mut self.state
    }

    pub(crate) fn frame_touch(&mut self, state: bool) {
        self.frame_touch = state;
    }
}

impl TouchHandler for WaylandState {
    fn down(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        touch: &WlTouch,
        serial: u32,
        time: u32,
        surface: WlSurface,
        id: i32,
        position: (f64, f64),
    ) {
        if let Some(mut pointer) = self.seat_state.pointers.info(touch.id().into()) {
            pointer.pointer_id = Some(PointerId::new(id as u64).unwrap_or(PointerId::PRIMARY));
            let window_id: WindowId = surface.id().into();
            let parent_id: WindowId = surface
                .data::<SurfaceData>()
                .and_then(|data| data.parent_surface().map(|s| s.id()))
                .unwrap_or(window_id.clone().into())
                .into();
            if self.windows.windows.contains_key(&parent_id)
                || self.windows.screenlocks.contains_key(&parent_id)
            {
                let position = LogicalPosition::<f64>::from(position);
                let scale_factor = self
                    .windows
                    .get(&parent_id)
                    .map(|w| w.scale_factor)
                    .unwrap_or(1);

                let mut state = PointerState {
                    position: position.to_physical(scale_factor as f64),
                    modifiers: self.seat_state.modifiers,
                    pressure: 0.5,
                    ..Default::default()
                };
                // Save touch state
                self.seat_state.pointers.add_touch(
                    id,
                    TouchState::new(parent_id.clone(), scale_factor, state.clone()),
                );
                if window_id != parent_id {
                    let window = self.windows.get_mut(&parent_id).unwrap();
                    self.seat_state.pointers.frame_touch(id, true);
                    let pointer_data = touch.data::<TouchData>().unwrap();
                    let seat = pointer_data.seat();
                    if window.on_frame_action(true, FrameClick::Normal, seat, time, serial) {
                        self.windows.close_request.insert(parent_id);
                    }
                } else {
                    state.time = time as u64;
                    self.events.push_back(Events::Pointer(
                        parent_id.clone(),
                        PointerEvent::Down {
                            button: Some(PointerButton::Primary),
                            pointer,
                            state,
                        },
                    ))
                }
            }
        }
    }

    fn up(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        touch: &WlTouch,
        serial: u32,
        time: u32,
        id: i32,
    ) {
        if let (Some(mut pointer), Some(touch_state)) = (
            self.seat_state.pointers.info(touch.id().into()),
            self.seat_state.pointers.remove_touch(&id),
        ) {
            pointer.pointer_id = Some(PointerId::new(id as u64).unwrap_or(PointerId::PRIMARY));

            if touch_state.frame_touch {
                if let Some(window) = self.windows.get_mut(&touch_state.window_id) {
                    let pointer_data = touch.data::<TouchData>().unwrap();
                    let seat = pointer_data.seat();
                    if window.on_frame_action(true, FrameClick::Normal, seat, time, serial) {
                        self.windows
                            .close_request
                            .insert(touch_state.window_id.clone());
                    }
                }
            } else {
                self.events.push_back(Events::Pointer(
                    touch_state.window_id,
                    PointerEvent::Up {
                        button: Some(PointerButton::Primary),
                        pointer,
                        state: touch_state.state,
                    },
                ))
            }
        }
    }

    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        touch: &WlTouch,
        time: u32,
        id: i32,
        position: (f64, f64),
    ) {
        if let (Some(mut pointer), Some(touch_state)) = (
            self.seat_state.pointers.info(touch.id().into()),
            self.seat_state.pointers.get_mut_touch(id),
        ) {
            pointer.pointer_id = Some(PointerId::new(id as u64).unwrap_or(PointerId::PRIMARY));
            let position = LogicalPosition::<f64>::from(position);
            let scale_factor = touch_state.scale_factor;
            let state = touch_state.get_mut_state();
            state.position = position.to_physical(scale_factor as f64);
            state.modifiers = self.seat_state.modifiers;
            state.time = time as u64;
            self.events.push_back(Events::Pointer(
                touch_state.window_id.clone(),
                PointerEvent::Move(PointerUpdate {
                    pointer,
                    current: touch_state.state.clone(),
                    coalesced: Vec::new(),
                    predicted: Vec::new(),
                }),
            ))
        }
    }

    fn shape(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _touch: &WlTouch,
        id: i32,
        major: f64,
        minor: f64,
    ) {
        if let Some(touch_state) = self.seat_state.pointers.get_mut_touch(id) {
            let state = touch_state.get_mut_state();
            state.contact_geometry = ContactGeometry {
                width: major,
                height: minor,
            };
        }
    }

    fn orientation(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _touch: &WlTouch,
        id: i32,
        orientation: f64,
    ) {
        if let Some(touch_state) = self.seat_state.pointers.get_mut_touch(id) {
            let state = touch_state.get_mut_state();
            state.orientation = PointerOrientation {
                altitude: core::f32::consts::FRAC_PI_2,
                azimuth: orientation as f32,
            };
        }
    }

    fn cancel(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _touch: &WlTouch) {
        for (id, state) in self.seat_state.pointers.touch_state.drain(..) {
            self.events.push_back(Events::Pointer(
                state.window_id,
                PointerEvent::Cancel(PointerInfo {
                    pointer_id: Some(PointerId::new(id as u64).unwrap_or(PointerId::PRIMARY)),
                    persistent_device_id: None,
                    pointer_type: PointerType::Touch,
                }),
            ));
        }
    }
}
