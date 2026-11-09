//! Own the platform timer until its reply deadline fires or the caller cancels it.
use futures_channel::oneshot;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use worker::wasm_bindgen::{self, JsCast, JsValue, closure::Closure};

#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = globalThis, js_name = setTimeout)]
    fn schedule_timeout(callback: &js_sys::Function, milliseconds: i32) -> JsValue;
    #[wasm_bindgen(js_namespace = globalThis, js_name = clearTimeout)]
    fn cancel_timeout(handle: &JsValue);
}

pub(crate) struct EventDeadline {
    completed: oneshot::Receiver<()>,
    // Retain the callback until Drop has cancelled the corresponding timer.
    _callback: Closure<dyn FnMut()>,
    handle: JsValue,
}
impl EventDeadline {
    pub(crate) fn new(remaining: Duration) -> Self {
        let (complete, completed) = oneshot::channel();
        let mut complete = Some(complete);
        let callback = Closure::wrap(Box::new(move || {
            if let Some(complete) = complete.take() {
                let _ = complete.send(());
            }
        }) as Box<dyn FnMut()>);
        let milliseconds = i32::try_from(remaining.as_millis()).unwrap_or(i32::MAX);
        // The platform may return a number or a Timeout object. Neither its
        // representation nor its numeric conversion belongs to this adapter.
        let handle = schedule_timeout(callback.as_ref().unchecked_ref(), milliseconds);
        Self {
            completed,
            _callback: callback,
            handle,
        }
    }
}
impl Future for EventDeadline {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        Pin::new(&mut self.completed).poll(context).map(|_| ())
    }
}
impl Drop for EventDeadline {
    fn drop(&mut self) {
        cancel_timeout(&self.handle);
    }
}
