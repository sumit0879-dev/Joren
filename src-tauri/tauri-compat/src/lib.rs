pub use tauri_macros::command;

use serde::Serialize;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Clone, Default)]
pub struct AppPath;

impl AppPath {
    pub fn app_data_dir(&self) -> Result<PathBuf, String> {
        let appdata = std::env::var("APPDATA")
            .unwrap_or_else(|_| ".".to_string());
        let path = PathBuf::from(appdata).join("Joren");
        std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn app_config_dir(&self) -> Result<PathBuf, String> {
        self.app_data_dir()
    }
}

pub struct State<'r, T: ?Sized + 'static>(&'r T);

impl<'r, T: ?Sized + 'static> State<'r, T> {
    pub fn new(inner: &'r T) -> Self {
        Self(inner)
    }
}

impl<'r, T: ?Sized + 'static> std::ops::Deref for State<'r, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

pub trait Emitter {
    fn emit<T: Serialize>(&self, event: &str, payload: T) -> Result<(), String>;
}

pub trait Manager {
    fn app_handle(&self) -> &AppHandle;
    fn manage<T: 'static + Send + Sync>(&self, state: T);
    fn state<T: 'static + Send + Sync>(&self) -> State<'_, T>;
    fn try_state<T: 'static + Send + Sync>(&self) -> Option<State<'_, T>>;
}

type EventCallback = Box<dyn Fn(&str, &str) + Send + Sync>;

#[derive(Clone)]
pub struct AppHandle {
    states: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
    event_dispatcher: Arc<RwLock<Option<EventCallback>>>,
}

impl Default for AppHandle {
    fn default() -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            event_dispatcher: Arc::new(RwLock::new(None)),
        }
    }
}

impl AppHandle {
    pub fn set_event_dispatcher<F: Fn(&str, &str) + Send + Sync + 'static>(&self, f: F) {
        let mut d = self.event_dispatcher.write().unwrap();
        *d = Some(Box::new(f));
    }

    pub fn path(&self) -> AppPath {
        AppPath
    }

    pub fn manage<T: 'static + Send + Sync>(&self, state: T) {
        let mut states = self.states.write().unwrap();
        states.insert(TypeId::of::<T>(), Box::new(state));
    }

    pub fn try_state<T: 'static + Send + Sync>(&self) -> Option<State<'_, T>> {
        let states = self.states.read().unwrap();
        if let Some(boxed) = states.get(&TypeId::of::<T>()) {
            let ptr = boxed.as_ref() as *const (dyn Any + Send + Sync);
            unsafe {
                let concrete = &*(ptr as *const T);
                Some(State(concrete))
            }
        } else {
            None
        }
    }

    pub fn state<T: 'static + Send + Sync>(&self) -> State<'_, T> {
        self.try_state::<T>().expect("State not found in AppHandle")
    }

    pub fn emit<T: Serialize>(&self, event: &str, payload: T) -> Result<(), String> {
        let json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        if let Some(cb) = self.event_dispatcher.read().unwrap().as_ref() {
            cb(event, &json);
        }
        Ok(())
    }

    pub fn get_window(&self, _label: &str) -> Option<Window> {
        Some(Window { app: self.clone() })
    }

    pub fn get_webview_window(&self, _label: &str) -> Option<WebviewWindow> {
        Some(WebviewWindow)
    }
}

impl Emitter for AppHandle {
    fn emit<T: Serialize>(&self, event: &str, payload: T) -> Result<(), String> {
        self.emit(event, payload)
    }
}

impl Manager for AppHandle {
    fn app_handle(&self) -> &AppHandle {
        self
    }

    fn manage<T: 'static + Send + Sync>(&self, state: T) {
        self.manage(state);
    }

    fn state<T: 'static + Send + Sync>(&self) -> State<'_, T> {
        self.state::<T>()
    }

    fn try_state<T: 'static + Send + Sync>(&self) -> Option<State<'_, T>> {
        self.try_state::<T>()
    }
}

pub struct App {
    handle: AppHandle,
}

impl App {
    pub fn handle(&self) -> &AppHandle {
        &self.handle
    }

    pub fn manage<T: 'static + Send + Sync>(&self, state: T) {
        self.handle.manage(state);
    }
}

impl Manager for App {
    fn app_handle(&self) -> &AppHandle {
        &self.handle
    }

    fn manage<T: 'static + Send + Sync>(&self, state: T) {
        self.handle.manage(state);
    }

    fn state<T: 'static + Send + Sync>(&self) -> State<'_, T> {
        self.handle.state::<T>()
    }

    fn try_state<T: 'static + Send + Sync>(&self) -> Option<State<'_, T>> {
        self.handle.try_state::<T>()
    }
}

#[derive(Clone)]
pub struct Window {
    app: AppHandle,
}

impl Window {
    pub fn app_handle(&self) -> &AppHandle {
        &self.app
    }
}

#[derive(Clone, Debug)]
pub struct LogicalPosition<P> {
    pub x: P,
    pub y: P,
}

impl<P> LogicalPosition<P> {
    pub fn new(x: P, y: P) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Debug)]
pub struct LogicalSize<S> {
    pub width: S,
    pub height: S,
}

impl<S> LogicalSize<S> {
    pub fn new(width: S, height: S) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Debug)]
pub enum Position {
    Logical(LogicalPosition<f64>),
}

#[derive(Clone, Debug)]
pub enum Size {
    Logical(LogicalSize<f64>),
}

#[derive(Clone, Debug)]
pub struct Rect {
    pub position: Position,
    pub size: Size,
}

pub type Url = reqwest::Url;

#[derive(Clone, Debug)]
pub enum WebviewUrl {
    External(Url),
    App(PathBuf),
    Custom(String),
}

pub struct WebviewWindow;

impl WebviewWindow {
    pub fn set_bounds(&self, _bounds: Rect) -> Result<(), String> {
        Ok(())
    }

    pub fn show(&self) -> Result<(), String> {
        Ok(())
    }

    pub fn hide(&self) -> Result<(), String> {
        Ok(())
    }

    pub fn set_focus(&self) -> Result<(), String> {
        Ok(())
    }

    pub fn navigate(&self, _url: Url) -> Result<(), String> {
        Ok(())
    }

    pub fn eval(&self, _script: &str) -> Result<(), String> {
        Ok(())
    }

    pub fn close(&self) -> Result<(), String> {
        Ok(())
    }
}

pub struct WebviewBuilder {
    pub label: String,
    pub url: WebviewUrl,
    pub data_dir: Option<PathBuf>,
    pub init_scripts: Vec<String>,
}

impl WebviewBuilder {
    pub fn new(label: &str, url: WebviewUrl) -> Self {
        Self {
            label: label.to_string(),
            url,
            data_dir: None,
            init_scripts: Vec::new(),
        }
    }

    pub fn data_directory(mut self, path: PathBuf) -> Self {
        self.data_dir = Some(path);
        self
    }

    pub fn initialization_script(mut self, script: &str) -> Self {
        self.init_scripts.push(script.to_string());
        self
    }

    pub fn on_navigation<F: 'static + Fn(Url) -> bool + Send>(self, _f: F) -> Self {
        self
    }

    pub fn build(self, _window: &Window) -> Result<WebviewWindow, String> {
        Ok(WebviewWindow)
    }
}

#[derive(Clone, Debug)]
pub enum WindowEvent {
    CloseRequested { has_prevented_default: bool },
    Resized(LogicalSize<u32>),
    Focused(bool),
}

pub struct Context;

#[derive(Default)]
pub struct Builder {
    app: AppHandle,
    setup_fn: Option<Box<dyn FnOnce(&mut App) -> Result<(), Box<dyn std::error::Error>>>>,
}

impl Builder {
    pub fn default() -> Self {
        Self {
            app: AppHandle::default(),
            setup_fn: None,
        }
    }

    pub fn plugin<P>(self, _plugin: P) -> Self {
        self
    }

    pub fn setup<F>(mut self, setup: F) -> Self
    where
        F: 'static + FnOnce(&mut App) -> Result<(), Box<dyn std::error::Error>>,
    {
        self.setup_fn = Some(Box::new(setup));
        self
    }

    pub fn on_window_event<F>(self, _handler: F) -> Self
    where
        F: 'static + Fn(&Window, WindowEvent) + Send + Sync,
    {
        self
    }

    pub fn invoke_handler<H>(self, _handler: H) -> Self {
        self
    }

    pub fn run(mut self, _ctx: Context) {
        let mut app = App {
            handle: self.app.clone(),
        };
        if let Some(setup) = self.setup_fn.take() {
            let _ = setup(&mut app);
        }
    }
}

pub struct InvokeHandler;

impl InvokeHandler {
    pub fn new() -> Self {
        Self
    }
}

#[macro_export]
macro_rules! generate_handler {
    ($($cmd:path),* $(,)?) => {
        $crate::InvokeHandler::new()
    };
}

#[macro_export]
macro_rules! generate_context {
    () => {
        $crate::Context
    };
}
