use std::{
    cell::RefCell,
    collections::VecDeque,
    mem,
    rc::Rc,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler};
use accesskit_unix::Adapter;
use dpi::{LogicalSize, PhysicalSize};
use smithay_client_toolkit::reexports::calloop::{self, EventLoop, channel::Sender as WlSender};
use tracing::error;
use ui_events::{keyboard::KeyboardEvent, pointer::PointerEvent};

use crate::{
    WaylandState, WaylandWindow, WindowAttributes, WindowCore, WindowId, WindowsRegistry,
    state::logical_to_physical_rounded,
    window::{DEFAULT_SCALE_FACTOR, DEFAULT_WINDOW_SIZE},
};

static LOOP_RUNNING: AtomicBool = AtomicBool::new(true);

static WINDOWS_CREATION_EVENT: OnceLock<WlSender<WindowAttributes>> = OnceLock::new();

#[derive(Debug)]
pub enum AccesskitEvents {
    AccessabilityActivate(WindowId),   // done
    AccessibilityDeactivate(WindowId), // done
    Action(WindowId, ActionRequest),   // done
}

/// Do not rewrite this trait methods
pub trait LoopHandler {
    fn request_new_window(&self, new_window: WindowAttributes) -> Result<(), String> {
        WINDOWS_CREATION_EVENT
            .get()
            .and_then(|s| s.send(new_window).ok())
            // TODO: rewrite error
            .ok_or(String::from("Event loop has not been initialized yet"))
    }

    fn default_window_size(&self) -> LogicalSize<u32> {
        DEFAULT_WINDOW_SIZE.to_owned()
    }

    fn default_scale_factor(&self) -> i32 {
        DEFAULT_SCALE_FACTOR
    }

    fn stop(&self) {
        LOOP_RUNNING.store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone)]
pub struct AccesskitHandler {
    id: WindowId,
    event_sender: WlSender<AccesskitEvents>,
}

impl AccesskitHandler {
    pub fn new(id: WindowId, event_sender: WlSender<AccesskitEvents>) -> Self {
        Self { id, event_sender }
    }
}

impl ActionHandler for AccesskitHandler {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        if let Err(err) = self
            .event_sender
            .send(AccesskitEvents::Action(self.id.clone(), request))
        {
            error!("{err}");
        }
    }
}

impl ActivationHandler for AccesskitHandler {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        if let Err(err) = self
            .event_sender
            .send(AccesskitEvents::AccessabilityActivate(self.id.clone()))
        {
            error!("{err}");
        }
        None
    }
}

impl DeactivationHandler for AccesskitHandler {
    fn deactivate_accessibility(&mut self) {
        if let Err(err) = self
            .event_sender
            .send(AccesskitEvents::AccessibilityDeactivate(self.id.clone()))
        {
            error!("{err}");
        }
    }
}

#[derive(Debug)]
pub enum Events {
    RedrawRequest(WindowId),
    Keyboard(KeyboardEvent),
    Pointer(WindowId, PointerEvent),
    Focus(WindowId, bool),
}

pub struct WlEventLoop<UserEvent> {
    state: WaylandState,
    user_events: Rc<RefCell<VecDeque<UserEvent>>>,
    event_loop: EventLoop<'static, WaylandState>,
    event_sender: WlSender<UserEvent>,
    running: bool,
}

impl<UserEvent> WlEventLoop<UserEvent>
where
    UserEvent: 'static + Send,
{
    pub fn init() -> Self {
        let (mut state, event_loop) = WaylandState::new();

        // Windows creation preparation
        let (create_window, rx) = calloop::channel::channel::<WindowAttributes>();
        let create_window_token = event_loop
            .handle()
            .insert_source(rx, move |event, _, state| {
                if let calloop::channel::Event::Msg(new_window) = event {
                    state.create_window(new_window);
                }
            })
            .expect("Failed to create user event handle");
        WINDOWS_CREATION_EVENT.set(create_window).unwrap();

        // User events handler preparation
        let user_events = Rc::new(RefCell::new(VecDeque::new()));
        let user_events_clone = user_events.clone();
        let (event_sender, rx) = calloop::channel::channel::<UserEvent>();
        let user_event_token = event_loop
            .handle()
            .insert_source(rx, move |event, _, _state| {
                if let calloop::channel::Event::Msg(msg) = event {
                    user_events_clone.borrow_mut().push_back(msg);
                }
            })
            .expect("Failed to create user event handle");

        // To release sources after app exit properly
        state.event_source_token.push(create_window_token);
        state.event_source_token.push(user_event_token);
        Self {
            state,
            user_events,
            event_loop,
            event_sender,
            running: true,
        }
    }

    pub fn run(&mut self, app: &mut impl ApplicationHandler<UserEvent>) -> Result<(), String> {
        self.running = true;
        while self.running {
            tracing::trace!("Wayland app running");
            // TODO: what timeout should be set?
            match self.event_loop.dispatch(None, &mut self.state) {
                Ok(_) => {
                    let new_windows = mem::take(&mut self.state.windows.new_windows);
                    let rescale_req = mem::take(&mut self.state.windows.rescale_request);
                    let mut resize_req = mem::take(&mut self.state.windows.resize_request);
                    let mut redraw_req = mem::take(&mut self.state.windows.redraw_request);
                    let close_req = mem::take(&mut self.state.windows.close_request);

                    // Let's notify user about all new windows to handle them
                    for window in new_windows {
                        app.create_window(window);
                    }

                    // Let's handle all user events
                    if let Ok(mut events) = self.user_events.try_borrow_mut() {
                        while let Some(event) = (*events).pop_front() {
                            app.user_events_handle(event);
                        }
                    }
                    for object_id in rescale_req.iter() {
                        if let Some(window) = self.state.windows.get(object_id) {
                            app.rescale_handle(
                                window.get_surface_id().into(),
                                window.scale_factor as f64,
                            );
                            resize_req.insert(object_id.clone());
                        }
                    }
                    for object_id in resize_req.iter() {
                        if let Some(window) = self.state.windows.get(object_id) {
                            app.resize_handle(
                                window.get_surface_id(),
                                logical_to_physical_rounded(
                                    window.size,
                                    window.scale_factor as f64,
                                ),
                            );
                            redraw_req.insert(object_id.clone());
                        }
                    }
                    // Let's handle all user changes to windows
                    app.user_signals_handle(&mut self.state.windows);
                    // Let's handle accesskit events and then compositor events
                    while let Some(event) = self.state.accesskit_events.pop_front() {
                        // Accesskit events do not request `draw_handle` method. So, one needs to request this in `user_signals_handle` via `redraw_request()` method on WaylandWindow
                        let window = match &event {
                            AccesskitEvents::AccessabilityActivate(object_id)
                            | AccesskitEvents::AccessibilityDeactivate(object_id)
                            | AccesskitEvents::Action(object_id, _) => {
                                self.state.windows.get_mut(object_id)
                            }
                        };
                        if let Some(window) = window {
                            let object_id = window.get_surface_id().clone();
                            let adapter = &mut window.accesskit_adapter;
                            match event {
                                AccesskitEvents::AccessabilityActivate(_) => {
                                    app.accesskit_activate_handle(object_id, adapter)
                                }
                                AccesskitEvents::AccessibilityDeactivate(_) => {
                                    app.accesskit_deactivate_handle(object_id, adapter)
                                }
                                AccesskitEvents::Action(_, action_request) => {
                                    app.accesskit_action_handle(object_id, action_request, adapter)
                                }
                            }
                        }
                    }
                    while let Some(event) = self.state.events.pop_front() {
                        match event {
                            // Receiving redraw request from WaylandWindow
                            Events::RedrawRequest(object_id) => {
                                self.state.windows.redraw_request.insert(object_id);
                            }
                            Events::Keyboard(keyboard_event) => {
                                if let Some(object_id) =
                                    self.state.seat_state.keyboard_focus.as_ref()
                                {
                                    self.state.windows.redraw_request.insert(object_id.clone());
                                    app.keyboard_handle(object_id, keyboard_event);
                                }
                            }
                            Events::Pointer(object_id, pointer_event) => {
                                app.pointer_handle(&object_id, pointer_event)
                            }
                            Events::Focus(object_id, new_focus) => {
                                app.focus_handle(&object_id, new_focus)
                            }
                        }
                    }
                    for object_id in redraw_req {
                        if let Some(window) = self.state.windows.get_mut(&object_id) {
                            // TODO: Чтобы делать нормальный refresh frame, нужно вызывать draw_handle, а не запрос на перерисовку
                            window.refresh_frame();
                            app.draw_handle(window.core.clone(), &mut window.accesskit_adapter);
                        }
                    }
                    for id in close_req.iter() {
                        self.state.windows.remove(id);
                        app.close_handle(id);
                    }
                }
                Err(err) => {
                    tracing::error!("Error dispatching event loop: {}", err);
                    return Err(String::from("Error dispatching event loop"));
                }
            }

            // Let's handle all wayland state events and close an app, if we receive close request
            if self.state.windows.is_empty() || !LOOP_RUNNING.load(Ordering::Acquire) {
                tracing::debug!("Closing an app...");
                self.running = false;
            }
        }
        Ok(())
    }

    pub fn send_event(&self, event: UserEvent) {
        if let Err(err) = self.event_sender.send(event) {
            error!("{err}");
        }
    }
}

pub trait ApplicationHandler<UserEvent>
where
    UserEvent: 'static + Send,
{
    fn create_window(&mut self, new_window: Arc<WindowCore>);
    fn draw_handle(&mut self, window: Arc<WindowCore>, adapter: &mut Adapter);
    fn keyboard_handle(&mut self, window_id: &WindowId, keyboard_event: KeyboardEvent);
    fn pointer_handle(&mut self, window_id: &WindowId, pointer_event: PointerEvent);
    fn resize_handle(&mut self, window_id: &WindowId, size: PhysicalSize<u32>);
    fn focus_handle(&mut self, window_id: &WindowId, new_focus: bool);
    fn rescale_handle(&mut self, window_id: &WindowId, scale_factor: f64);
    fn user_signals_handle(&mut self, windows: &mut WindowsRegistry);
    fn user_events_handle(&mut self, event: UserEvent);
    fn accesskit_activate_handle(&self, window_id: WindowId, adapter: &mut Adapter);
    fn accesskit_action_handle(
        &self,
        window_id: WindowId,
        action: ActionRequest,
        adapter: &mut Adapter,
    );
    fn accesskit_deactivate_handle(&self, window_id: WindowId, adapter: &mut Adapter);

    /// Do something before main event loop will be stopped: save state, etc.
    fn close_handle(&mut self, window_id: &WindowId);
}
