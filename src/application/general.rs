use crate::{
    AppWindowAttributes, ApplicationDispatcher, ApplicationExt, Callback, Message, Result, WebView,
    WebViewAttributes, WebViewBuilder, WindowExt,
};
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowAttributes, WindowBuilder},
};
pub use winit::window::WindowId;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub struct WinitWindow(Window);

impl WindowExt<'_> for WinitWindow {
    type Id = WindowId;
    fn id(&self) -> Self::Id {
        self.0.id()
    }
}

type EventLoopProxy<I, T> = Arc<Mutex<winit::event_loop::EventLoopProxy<Message<I, T>>>>;

#[derive(Clone)]
pub struct AppDispatcher<T: 'static> {
    proxy: EventLoopProxy<WindowId, T>,
}

impl<T> ApplicationDispatcher<WindowId, T> for AppDispatcher<T> {
    fn dispatch_message(&self, message: Message<WindowId, T>) -> Result<()> {
        self.proxy
            .lock()
            .unwrap()
            .send_event(message)
            .unwrap_or_else(|_| panic!("failed to dispatch message to event loop"));
        Ok(())
    }
}

pub struct Application<T: 'static> {
    webviews: HashMap<WindowId, WebView>,
    event_loop: EventLoop<Message<WindowId, T>>,
    event_loop_proxy: EventLoopProxy<WindowId, T>,
    message_handler: Option<Box<dyn FnMut(T)>>,
}

impl<T> ApplicationExt<'_, T> for Application<T> {
    type Window = WinitWindow;
    type Dispatcher = AppDispatcher<T>;

    fn new() -> Result<Self> {
        let event_loop = EventLoop::<Message<WindowId, T>>::with_user_event();
        let proxy = event_loop.create_proxy();
        Ok(Self {
            webviews: HashMap::new(),
            event_loop,
            event_loop_proxy: Arc::new(Mutex::new(proxy)),
            message_handler: None,
        })
    }

    fn create_window(&self, attributes: AppWindowAttributes) -> Result<Self::Window> {
        let mut window_builder = WindowBuilder::new();
        let window_attributes = WindowAttributes::from(&attributes);
        window_builder.window = window_attributes;
        let window = window_builder.build(&self.event_loop)?;
        Ok(WinitWindow(window))
    }

    fn create_webview(
        &mut self,
        window: Self::Window,
        attributes: WebViewAttributes,
        callbacks: Option<Vec<Callback>>,
    ) -> Result<()> {
        let mut webview = WebViewBuilder::new(window.0)?;
        for js in attributes.initialization_script {
            webview = webview.initialize_script(&js)?;
        }
        if let Some(cbs) = callbacks {
            for Callback { name, function } in cbs {
                webview = webview.add_callback(&name, function)?;
            }
        }
        webview = match attributes.url {
            Some(url) => webview.load_url(&url)?,
            None => webview,
        };

        let webview = webview.build()?;
        let id = webview.window().id();
        self.webviews.insert(id, webview);
        Ok(())
    }

    fn set_message_handler<F: FnMut(T) + 'static>(&mut self, handler: F) {
        self.message_handler.replace(Box::new(handler));
    }

    fn dispatcher(&self) -> Self::Dispatcher {
        AppDispatcher {
            proxy: self.event_loop_proxy.clone(),
        }
    }

    fn run(self) {
        let mut windows = self.webviews;
        let mut message_handler = self.message_handler;
        self.event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Wait;

            for (_, w) in windows.iter() {
                w.evaluate_script().unwrap();
            }
            match event {
                Event::WindowEvent { event, window_id } => match event {
                    WindowEvent::CloseRequested => {
                        windows.remove(&window_id);

                        if windows.is_empty() {
                            *control_flow = ControlFlow::Exit;
                        }
                    }
                    WindowEvent::Resized(_) => {
                        windows[&window_id].resize();
                    }
                    _ => {}
                },
                Event::UserEvent(message) => match message {
                    Message::Script(id, script) => {
                        if let Some(webview) = windows.get(&id) {
                            webview.dispatcher().dispatch_script(&script).unwrap();
                        }
                    }
                    Message::Custom(message) => {
                        if let Some(ref mut handler) = message_handler {
                            handler(message);
                        }
                    }
                },
                _ => (),
            }
        });
    }
}