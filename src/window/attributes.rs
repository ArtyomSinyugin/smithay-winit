use dpi::Size;
use wayland_backend::client::ObjectId;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowId(ObjectId);

impl WindowId {
    pub fn inner(&self) -> &ObjectId {
        &self.0
    }

    pub fn into_inner(self) -> ObjectId {
        self.0
    }
}

impl From<ObjectId> for WindowId {
    fn from(id: ObjectId) -> Self {
        WindowId(id)
    }
}

impl From<WindowId> for ObjectId {
    fn from(window_id: WindowId) -> Self {
        window_id.0
    }
}

impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WindowId({:?})", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationName {
    pub general: String,
    pub instance: String,
}

impl ApplicationName {
    pub fn new(general: String, instance: String) -> Self {
        Self { general, instance }
    }
}

#[derive(Debug, Clone)]
pub struct WindowAttributes {
    pub title: String,
    // TODO: consider to delete
    pub app_id: String,
    pub visible: bool,
    pub surface_size: Option<Size>,
    pub min_surface_size: Option<Size>,
    pub max_surface_size: Option<Size>,
    // TODO
    pub resizable: bool,
    // TODO
    pub fullscreen: bool,
    pub maximized: bool,
    pub hide_titlebar: bool,
    pub decorations: bool,
    pub light_theme: Option<bool>,
    pub transparent: bool,
    // TODO: consider to use as app_id
    pub app_name: Option<ApplicationName>,
}

impl Default for WindowAttributes {
    fn default() -> Self {
        Self {
            title: "Wayland window".to_owned(),
            app_id: "wayland.window".to_owned(),
            visible: true,
            surface_size: None,
            min_surface_size: None,
            max_surface_size: None,
            resizable: Default::default(),
            fullscreen: false,
            maximized: false,
            hide_titlebar: false,
            decorations: true,
            light_theme: None,
            transparent: false,
            app_name: Default::default(),
        }
    }
}

impl WindowAttributes {
    /// Sets the initial title of the window in the title bar.
    ///
    /// The default is `"winit window"`.
    ///
    /// See [`Window::set_title`] for details.
    #[inline]
    pub fn with_title<T: Into<String>>(mut self, title: T) -> Self {
        self.title = title.into();
        self
    }

    #[inline]
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Requests the window to be of specific dimensions.
    ///
    /// If this is not set, some platform-specific dimensions will be used.
    ///
    /// See [`Window::request_inner_size`] for details.
    #[inline]
    pub fn with_size<S: Into<Size>>(mut self, size: S) -> Self {
        self.surface_size = Some(size.into());
        self
    }

    /// Sets the minimum dimensions the surface can have.
    ///
    /// If this is not set, the surface will have no minimum dimensions (aside from reserved).
    ///
    /// See [`Window::set_min_surface_size`] for details.
    #[inline]
    pub fn with_min_surface_size<S: Into<Size>>(mut self, min_size: S) -> Self {
        self.min_surface_size = Some(min_size.into());
        self
    }

    /// Sets the maximum dimensions the surface can have.
    ///
    /// If this is not set, the surface will have no maximum, or the maximum will be restricted to
    /// the primary monitor's dimensions by the platform.
    ///
    /// See [`Window::set_max_surface_size`] for details.
    #[inline]
    pub fn with_max_surface_size<S: Into<Size>>(mut self, max_size: S) -> Self {
        self.max_surface_size = Some(max_size.into());
        self
    }

    /// Request that the window is maximized upon creation.
    ///
    /// The default is `false`.
    ///
    /// See [`Window::set_maximized`] for details.
    #[inline]
    pub fn with_maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }

    /// Sets whether the window is resizable or not.
    ///
    /// The default is `true`.
    ///
    /// See [`Window::set_resizable`] for details.
    #[inline]
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Sets whether the window should be put into fullscreen upon creation.
    ///
    /// The default is `None`.
    ///
    /// See [`Window::set_fullscreen`] for details.
    #[inline]
    pub fn with_fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Sets whether the window should have a border, a title bar, etc.
    ///
    /// The default is `true`.
    ///
    /// See [`Window::set_decorations`] for details.
    #[inline]
    pub fn with_decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }
}

/// Additional methods on [`WindowAttributes`] that are specific to Wayland.
pub trait WindowAttributesExtWayland {
    /// Build window with the given name.
    ///
    /// The `general` name sets an application ID, which should match the `.desktop`
    /// file distributed with your program. The `instance` is a `no-op`.
    ///
    /// For details about application ID conventions, see the
    /// [Desktop Entry Spec](https://specifications.freedesktop.org/desktop-entry-spec/desktop-entry-spec-latest.html#desktop-file-id)
    fn with_name(self, general: impl Into<String>, instance: impl Into<String>) -> Self;
}

impl WindowAttributesExtWayland for WindowAttributes {
    #[inline]
    fn with_name(mut self, general: impl Into<String>, instance: impl Into<String>) -> Self {
        self.app_name = Some(ApplicationName::new(general.into(), instance.into()));
        self
    }
}
