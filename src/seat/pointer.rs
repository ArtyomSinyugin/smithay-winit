use std::time::Duration;

use dpi::LogicalPosition;
use smithay_client_toolkit::{
    compositor::SurfaceData,
    reexports::{
        client::{Connection, Proxy, QueueHandle, protocol::wl_pointer::WlPointer},
        csd_frame::{DecorationsFrame, FrameClick},
    },
    seat::pointer::{PointerEvent as WlPointerEvent, PointerEventKind, PointerHandler},
};
use tracing::error;
use ui_events::pointer::{PointerButton, PointerEvent, PointerState, PointerUpdate};

use crate::{Events, WaylandState, WindowId};

impl PointerHandler for WaylandState {
    fn pointer_frame(
        &mut self,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
        pointer: &WlPointer,
        events: &[WlPointerEvent],
    ) {
        if let Some(mouse) = self.seat_state.pointers.info(pointer.id().into()) {
            for event in events {
                let surface = &event.surface;
                let id = surface.id();

                let parent_id: WindowId = surface
                    .data::<SurfaceData>()
                    .and_then(|data| data.parent_surface().map(|s| s.id()))
                    .unwrap_or(id.clone().into())
                    .into();

                let pointer_kind = match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Leave { .. } => {
                        self.pointer_kind(pointer)
                    }
                    PointerEventKind::Press { .. } | PointerEventKind::Release { .. }
                        if parent_id != id.clone().into() =>
                    {
                        self.windows.redraw_request.insert(parent_id.clone());
                        None
                    }
                    _ => None,
                };
                if let Some(window) = self.windows.get_mut(&parent_id) {
                    let position = LogicalPosition::<f64>::from(event.position);
                    let mut state = PointerState {
                        position: position.to_physical(window.scale_factor as f64),
                        modifiers: self.seat_state.modifiers,
                        ..Default::default()
                    };
                    if parent_id != id.into() {
                        // Decoration events
                        match event.kind {
                            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                                if let (Some(frame), Some(pointer_kind)) =
                                    (window.window_frame.as_mut(), pointer_kind)
                                {
                                    if let Some(icon) = frame.click_point_moved(
                                        Duration::ZERO,
                                        &surface.id(),
                                        event.position.0,
                                        event.position.1,
                                    ) {
                                        if let Err(err) = pointer_kind.set_cursor(conn, icon) {
                                            error!("{err}");
                                        }
                                    }
                                }
                            }
                            PointerEventKind::Leave { .. } => {
                                if let Some(frame) = window.window_frame.as_mut() {
                                    frame.click_point_left();
                                }
                            }
                            PointerEventKind::Press {
                                time,
                                button,
                                serial,
                            }
                            | PointerEventKind::Release {
                                time,
                                button,
                                serial,
                            } => {
                                let pressed = matches!(event.kind, PointerEventKind::Press { .. });
                                let click = match button {
                                    0x110 => FrameClick::Normal,
                                    0x111 => FrameClick::Alternate,
                                    _ => continue,
                                };

                                if let Some(action) =
                                    window.window_frame.as_mut().and_then(|frame| {
                                        frame.on_click(
                                            Duration::from_millis(time as u64),
                                            click,
                                            pressed,
                                        )
                                    })
                                {
                                    if window.frame_action(pointer, serial, action) {
                                        self.windows.close_request.insert(parent_id);
                                    }
                                }
                            }
                            PointerEventKind::Axis { .. } => {}
                        }
                    } else {
                        // Window events
                        match event.kind {
                            PointerEventKind::Enter { .. } => {
                                if let Some(pointer_kind) = pointer_kind {
                                    if let Err(err) =
                                        pointer_kind.set_cursor(conn, window.selected_cursor)
                                    {
                                        error!("{err}");
                                    }
                                    window.pointer_enter(pointer_kind);
                                }
                                self.events.push_back(Events::Pointer(
                                    parent_id,
                                    PointerEvent::Enter(mouse),
                                ));
                            }
                            PointerEventKind::Leave { .. } => {
                                if let Some(pointer_kind) = pointer_kind {
                                    window.pointer_leave(pointer_kind);
                                }
                                self.events.push_back(Events::Pointer(
                                    parent_id,
                                    PointerEvent::Leave(mouse),
                                ));
                            }
                            PointerEventKind::Motion { time } => {
                                state.time = time as u64;
                                self.events.push_back(Events::Pointer(
                                    parent_id,
                                    PointerEvent::Move(PointerUpdate {
                                        pointer: mouse,
                                        current: state,
                                        coalesced: Vec::new(),
                                        predicted: Vec::new(),
                                    }),
                                ));
                            }
                            PointerEventKind::Press { time, button, .. } => {
                                state.time = time as u64;
                                let button = try_from_button(button);
                                self.events.push_back(Events::Pointer(
                                    parent_id.clone(),
                                    PointerEvent::Down {
                                        button,
                                        pointer: mouse,
                                        state,
                                    },
                                ))
                            }
                            PointerEventKind::Release { time, button, .. } => {
                                state.time = time as u64;
                                let button = try_from_button(button);
                                self.events.push_back(Events::Pointer(
                                    parent_id,
                                    PointerEvent::Up {
                                        button,
                                        pointer: mouse,
                                        state,
                                    },
                                ))
                            }
                            PointerEventKind::Axis { .. } => {}
                        }
                    }
                }
            }
        }
    }
}

fn try_from_button(code: u32) -> Option<PointerButton> {
    Some(match code {
        // Основные кнопки мыши
        0x110 => PointerButton::Primary,
        0x111 => PointerButton::Secondary,
        0x112 => PointerButton::Auxiliary,
        0x113 => PointerButton::X1,
        0x114 => PointerButton::X2,

        0x115 => PointerButton::B7,
        0x116 => PointerButton::B8,
        0x117 => PointerButton::B9,

        0x14b => PointerButton::PenEraser,

        0x118 => PointerButton::B10,
        0x119 => PointerButton::B11,
        0x11a => PointerButton::B12,
        0x11b => PointerButton::B13,
        0x11c => PointerButton::B14,
        0x11d => PointerButton::B15,
        0x11e => PointerButton::B16,
        0x11f => PointerButton::B17,
        0x120 => PointerButton::B18,
        0x121 => PointerButton::B19,
        0x122 => PointerButton::B20,
        0x123 => PointerButton::B21,
        0x124 => PointerButton::B22,
        0x125 => PointerButton::B23,
        0x126 => PointerButton::B24,
        0x127 => PointerButton::B25,
        0x128 => PointerButton::B26,
        0x129 => PointerButton::B27,
        0x12a => PointerButton::B28,
        0x12b => PointerButton::B29,
        0x12c => PointerButton::B30,
        0x12d => PointerButton::B31,
        0x12e => PointerButton::B32,
        _ => return None,
    })
}
