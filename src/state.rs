use std::{collections::VecDeque, rc::Rc, sync::Arc};

use accesskit_unix::Adapter;
use dpi::{LogicalSize, PhysicalSize};
use sctk_adwaita::AdwaitaFrame;
use smithay_client_toolkit::{
    activation::{ActivationHandler as WlActivationHandler, ActivationState, RequestData},
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_activation, delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_session_lock, delegate_shm, delegate_subcompositor,
    delegate_touch, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{self, EventLoop, LoopHandle, RegistrationToken, channel::Sender as WlSender},
        calloop_wayland_source::WaylandSource,
        client::{
            Connection, Proxy, QueueHandle,
            globals::registry_queue_init,
            protocol::{
                wl_output::{Transform, WlOutput},
                wl_pointer::WlPointer,
                wl_surface::WlSurface,
            },
        },
        csd_frame::{DecorationsFrame, WindowState},
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{SeatState as WlSeatState, pointer::PointerData},
    session_lock::{
        SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
        SessionLockSurfaceConfigure,
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{DecorationMode, Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler},
    subcompositor::SubcompositorState,
};
use tracing::error;

use crate::{
    AccesskitEvents, AccesskitHandler, Events, WindowCore, ViewporterState, WaylandWindow,
    WindowAttributes, WindowId, WindowsRegistry,
    seat::{PointerKind, SeatState},
    window::locked::LockedSurface,
};

pub struct WaylandState {
    pub conn: Connection,
    pub event_sender: WlSender<Events>,
    pub accesskit_event_sender: WlSender<AccesskitEvents>,

    pub event_source_token: Vec<RegistrationToken>,

    pub running: bool,
    /// The compositor state which is used to create new windows and regions.
    pub compositor_state: Arc<CompositorState>,

    /// The state of the subcompositor.
    pub subcompositor_state: Option<Arc<SubcompositorState>>,

    pub viewport_state: Option<ViewporterState>,

    /// The WlRegistry.
    pub registry_state: RegistryState,

    pub seat_state: SeatState,

    pub last_output: Option<WlOutput>,

    /// The state of the WlOutput handling.
    pub output_state: OutputState,

    /// The shm for software buffers, such as cursors.
    pub shm: Shm,

    /// The XDG shell that is used for windows.
    pub xdg_shell: XdgShell,

    // TODO: внедрить поле surface
    // surfaces: HashMap<HandleId, RenderSurface<'a>>,
    pub windows: WindowsRegistry,
    pub activation_state: Option<ActivationState>,

    pub accesskit_events: VecDeque<AccesskitEvents>,
    pub events: VecDeque<Events>,

    /// Loop handle to re-register event sources, such as keyboard repeat.
    /// Also need to close app correctly, if user event source is used.
    // pub loop_handle: LoopHandle<'static, Self>,

    /// Queue handle
    pub queue_handle: QueueHandle<Self>,
    loop_handle: LoopHandle<'static, Self>,

    // Client side decorations
    pub csd_fails: bool,
    // The pool where images are allocated (used for window icons and custom cursors)
    // Пока непонятно, зачем мне это нужно. Возможно, xilem выделяет буфер самостоятельно как то
    // pub image_pool: SlotPool,
    session_lock_state: SessionLockState,
    session_lock: Option<SessionLock>,
    lock_surfaces: Vec<LockedSurface>,
}

impl WaylandState {
    pub fn new() -> (Self, EventLoop<'static, WaylandState>) {
        // All Wayland apps start by connecting the compositor (server).
        let conn = Connection::connect_to_env().unwrap();

        // Enumerate the list of globals to get the protocols the server implements.
        let (globals, event_queue) = registry_queue_init(&conn).unwrap();
        let queue_handle = event_queue.handle();
        let event_loop: EventLoop<'static, WaylandState> =
            EventLoop::try_new().expect("Failed to initialize the event loop!");
        let loop_handle = event_loop.handle();
        let token = WaylandSource::new(conn.clone(), event_queue)
            .insert(loop_handle)
            .unwrap();
        // The compositor (not to be confused with the server which is commonly called the compositor) allows
        // configuring surfaces to be presented.
        let compositor =
            CompositorState::bind(&globals, &queue_handle).expect("wl_compositor not available");
        let subcompositor =
            SubcompositorState::bind(compositor.wl_compositor().clone(), &globals, &queue_handle)
                .map(|sbcr| Arc::new(sbcr))
                .ok();
        // For desktop platforms, the XDG shell is the standard protocol for creating desktop windows.
        let xdg_shell =
            XdgShell::bind(&globals, &queue_handle).expect("xdg shell is not available");
        // Since we are not using the GPU in this example, we use wl_shm to allow software rendering to a buffer
        // we share with the compositor process.
        let shm = Shm::bind(&globals, &queue_handle).expect("wl shm is not available.");
        // If the compositor supports xdg-activation it probably wants us to use it to get focus
        let activation_state = ActivationState::bind(&globals, &queue_handle).ok();
        // Suggest min allocation for our app.
        // let image_pool = SlotPool::new(2, &shm).expect("Failed to create pool");
        let seat_state = WlSeatState::new(&globals, &queue_handle);
        let viewport_state = ViewporterState::new(&globals, &queue_handle).ok();
        let (event_sender, events_channel) = calloop::channel::channel();
        let event_source_token: RegistrationToken = event_loop
            .handle()
            .insert_source(events_channel, move |event, _, state| {
                if let calloop::channel::Event::Msg(msg) = event {
                    state.events.push_back(msg);
                }
            })
            .expect("Faild to insert wayland events into calloop channel");
        let (accesskit_event_sender, events_channel) = calloop::channel::channel();
        let accesskit_source_token: RegistrationToken = event_loop
            .handle()
            .insert_source(events_channel, move |event, _, state| {
                if let calloop::channel::Event::Msg(msg) = event {
                    state.accesskit_events.push_back(msg);
                }
            })
            .expect("Faild to insert accesskit events into calloop channel");
        (
            Self {
                conn,
                event_sender,
                accesskit_event_sender,
                event_source_token: vec![token, event_source_token, accesskit_source_token],
                running: false,
                compositor_state: Arc::new(compositor),
                subcompositor_state: subcompositor,
                viewport_state,
                registry_state: RegistryState::new(&globals),
                seat_state: SeatState::new(seat_state),
                last_output: None,
                output_state: OutputState::new(&globals, &queue_handle),
                shm,
                xdg_shell,
                windows: WindowsRegistry::default(),
                activation_state,
                accesskit_events: VecDeque::new(),
                events: VecDeque::new(),
                session_lock_state: SessionLockState::new(&globals, &queue_handle),
                session_lock: None,
                lock_surfaces: Vec::new(),
                queue_handle,
                loop_handle: event_loop.handle(),
                csd_fails: true,
                // image_pool,
            },
            event_loop,
        )
    }

    pub fn create_locked_surfaces(&mut self) {
        if let Some(session_lock) = self.session_lock.as_ref() {
            for output in self.output_state.outputs() {
                let surface = self.compositor_state.create_surface(&self.queue_handle);
                let accesskit =
                    AccesskitHandler::new(surface.id().into(), self.accesskit_event_sender.clone());

                let accesskit_adapter =
                    Adapter::new(accesskit.clone(), accesskit.clone(), accesskit);

                // It's important to keep the `SessionLockSurface` returned here around, as the
                // surface will be destroyed when the `SessionLockSurface` is dropped.
                let lock_surface =
                    session_lock.create_lock_surface(surface, &output, &self.queue_handle);

                // let locked_surface =
                //     LockedSurface::new(lock_surface, self.conn.display(), accesskit_adapter);

                // self.lock_surfaces.push(locked_surface);
            }
        }
    }

    pub fn create_window(&mut self, new_window: WindowAttributes) {
        let surface = self.compositor_state.create_surface(&self.queue_handle);
        let viewport = self
            .viewport_state
            .as_ref()
            .map(|v| v.get_viewport(&surface, &self.queue_handle));
        let wl_id = surface.id();
        let decorations = match new_window.decorations {
            true => WindowDecorations::RequestServer,
            false => WindowDecorations::RequestClient,
        };

        // Здесь окно сразу оборачивается в toplevel
        let window = self
            .xdg_shell
            .create_window(surface, decorations, &self.queue_handle);

        let accesskit =
            AccesskitHandler::new(wl_id.clone().into(), self.accesskit_event_sender.clone());

        let accesskit_adapter = Adapter::new(accesskit.clone(), accesskit.clone(), accesskit);

        window.set_title(&new_window.title);
        // In order for the window to be mapped, we need to perform an initial commit with no attached buffer.
        // For more info, see WaylandSurface::commit
        //
        // The compositor will respond with an initial configure that we can then use to present to the window with
        // the correct options.
        window.commit();

        // To request focus, we first need to request a token. When we create a window it should
        // catch a focus, or this block should be deleted
        if let Some(activation) = self.activation_state.as_ref() {
            activation.request_token(
                &self.queue_handle,
                RequestData {
                    seat_and_serial: None,
                    surface: Some(window.wl_surface().clone()),
                    app_id: Some(new_window.app_id.clone()),
                },
            )
        }

        let immutable = Arc::new(WindowCore::new(wl_id.clone().into(), self.conn.display()));
        self.windows.new_windows.push(immutable.clone());

        self.windows.insert(
            wl_id.into(),
            WaylandWindow::new(
                immutable,
                window,
                self.last_output.as_ref(),
                new_window,
                self.event_sender.clone(),
                accesskit_adapter,
                Region::new(&*self.compositor_state).ok(),
                viewport,
            ),
        );
    }

    pub fn close_window(&mut self, id: &WindowId) -> WindowId {
        // Panic, if there is no windows to remove
        let id = self.windows.remove(&id);
        if self.windows.is_empty() {
            // Free event sources to close an app properly
            for token in self.event_source_token.drain(..) {
                self.loop_handle.remove(token);
            }
        }
        id
    }

    pub(crate) fn pointer_kind(&self, pointer: &WlPointer) -> Option<Rc<PointerKind>> {
        if let Some(data) = pointer.data::<PointerData>() {
            if let Some(pointer) = self.seat_state.pointers.kind(data.seat().id()) {
                return Some(pointer.clone());
            }
        }
        None
    }
}

// BEGIN: Code from winit (https://github.com/rust-windowing/winit)
// Copyright (c) 2015-2024 The winit Contributors
#[inline]
pub(crate) fn logical_to_physical_rounded(
    size: LogicalSize<u32>,
    scale_factor: f64,
) -> PhysicalSize<u32> {
    let width = size.width as f64 * scale_factor;
    let height = size.height as f64 * scale_factor;
    (width.round(), height.round()).into()
}

#[inline]
fn is_stateless(configure: &WindowConfigure) -> bool {
    !(configure.is_maximized() || configure.is_fullscreen() || configure.is_tiled())
}
// END: Code from winit

impl CompositorHandler for WaylandState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        new_factor: i32,
    ) {
        let id = surface.id().into();
        if let Some(window) = self.windows.get_mut(&id) {
            window.scale_factor = new_factor;
            self.windows.rescale_request.insert(id);
        }
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _new_transform: Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        _time: u32,
    ) {
        let id = surface.id().into();
        self.windows.redraw_request.insert(id);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &WlSurface,
        output: &WlOutput,
    ) {
        self.last_output = Some(output.clone());
        tracing::debug!("Last output");
        if let Some(window) = self.windows.get_mut(&surface.id().into()) {
            window.output = Some(output.to_owned());
            tracing::debug!("Window output");
        }
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _output: &WlOutput,
    ) {
    }
}

impl OutputHandler for WaylandState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {}

    fn update_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {}

    fn output_destroyed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {
    }
}

impl ShmHandler for WaylandState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl WindowHandler for WaylandState {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, window: &Window) {
        let id = window.wl_surface().id().into();
        self.windows.close_request.insert(id);
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        let id = window.wl_surface().id().into();
        let mut resize = false;
        if let Some(window) = self.windows.get_mut(&id) {
            if configure.decoration_mode == DecorationMode::Client
                && window.window_frame.is_none()
                && self.subcompositor_state.is_some()
                && self.csd_fails
            {
                match AdwaitaFrame::new(
                    &window.window,
                    &self.shm,
                    self.compositor_state.clone(),
                    self.subcompositor_state.as_ref().unwrap().clone(),
                    qh.clone(),
                    window.frame_config(),
                ) {
                    Ok(mut frame) => {
                        frame.set_title(&window.title);
                        frame.set_scaling_factor(window.scale_factor as f64);
                        // Hide the frame if we were asked to not decorate.
                        frame.set_hidden(!window.decorate);
                        window.window_frame = Some(frame);
                    }
                    Err(err) => {
                        error!("Failed to create client side decorations frame: {err}");
                        self.csd_fails = true;
                    }
                }
            } else if configure.decoration_mode == DecorationMode::Server {
                // Drop the frame for server side decorations to save resources.
                window.window_frame = None;
            }

            window.stateless = is_stateless(&configure);

            let (mut new_size, constrain) = if let Some(frame) = window.window_frame.as_mut() {
                // Configure the window states.
                frame.update_state(configure.state);
                frame.update_wm_capabilities(configure.capabilities);

                match configure.new_size {
                    (Some(width), Some(height)) => {
                        let (width, height) = frame.subtract_borders(width, height);
                        let width = width.map(|w| w.get()).unwrap_or(1);
                        let height = height.map(|h| h.get()).unwrap_or(1);
                        ((width, height).into(), false)
                    }
                    (None, None) if window.stateless => (window.stateless_size, true),
                    _ => (window.size, true),
                }
            } else {
                match configure.new_size {
                    (Some(width), Some(height)) => ((width.get(), height.get()).into(), false),
                    _ if window.stateless => (window.stateless_size, true),
                    _ => (window.size, true),
                }
            };

            // Apply configure bounds only when compositor let the user decide what size to pick.
            if constrain {
                let bounds = window.surface_size_bounds(&configure);
                new_size.width = bounds
                    .0
                    .map(|bound_w| new_size.width.min(bound_w.get()))
                    .unwrap_or(new_size.width);
                new_size.height = bounds
                    .1
                    .map(|bound_h| new_size.height.min(bound_h.get()))
                    .unwrap_or(new_size.height);
            }

            let new_state = configure.state;
            let old_state = window.state;

            let state_change_requires_resize = !old_state
                .symmetric_difference(new_state)
                .difference(WindowState::ACTIVATED | WindowState::SUSPENDED)
                .is_empty();

            // NOTE: Set the configure before doing a resize, since we query it during it.
            window.state = new_state;

            resize = state_change_requires_resize || new_size != window.size;
            if resize {
                window.resize(new_size);
            }
        }
        if resize {
            self.windows.resize_request.insert(id.clone());
        }
        self.windows.redraw_request.insert(id);
    }
}

impl SessionLockHandler for WaylandState {
    fn locked(&mut self, _conn: &Connection, qh: &QueueHandle<Self>, _session_lock: SessionLock) {
        todo!()
    }

    fn finished(&mut self, _conn: &Connection, qh: &QueueHandle<Self>, _session_lock: SessionLock) {
        todo!()
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: SessionLockSurface,
        _configure: SessionLockSurfaceConfigure,
        _serial: u32,
    ) {
        todo!()
    }
}

impl WlActivationHandler for WaylandState {
    type RequestData = RequestData;

    fn new_token(&mut self, token: String, data: &Self::RequestData) {
        self.activation_state
            .as_ref()
            .unwrap()
            .activate::<WaylandState>(data.surface.as_ref().unwrap(), token);
    }
}

delegate_compositor!(WaylandState);
delegate_subcompositor!(WaylandState);
delegate_output!(WaylandState);
delegate_shm!(WaylandState);

delegate_seat!(WaylandState);
delegate_keyboard!(WaylandState);
delegate_pointer!(WaylandState);
delegate_touch!(WaylandState);
delegate_session_lock!(WaylandState);

delegate_xdg_shell!(WaylandState);
delegate_xdg_window!(WaylandState);
delegate_activation!(WaylandState);

delegate_registry!(WaylandState);

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, WlSeatState,];
}
