//! The Pdfium engine: one thread owns the process-global bind, and every
//! request answers with an event.

use iced::Subscription;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use super::channel::{self, EventSink, SharedEvents};
use super::protocol::{Event, Request};
use thread::worker;

mod read;
mod render;
mod thread;

/// The UI's handle on the engine: a door to the worker, and the events it
/// answers with. The app owns exactly one, for the whole run.
pub struct Engine {
    /// The send half of the request channel. Created at launch, so a request
    /// posted before the worker exists is still a request the worker will find
    /// waiting when it starts.
    requests: Sender<Request>,
    /// The receive half, parked until the worker starts. Behind a lock because
    /// taking it is what makes the start happen once, and the start is what
    /// needs it.
    inbox: Mutex<Option<Receiver<Request>>>,
    /// The sink the worker answers into.
    sink: EventSink,
    /// The subscription side of that sink.
    events: SharedEvents,
    /// Whether the worker has been started.
    started: AtomicBool,
}

impl Engine {
    /// An engine that has not started its worker yet. Nothing here touches
    /// Pdfium, so this is safe to call while the app boots on a machine that
    /// has no engine at all.
    pub fn new() -> Self {
        let (sink, events) = channel::channel();
        Self::from_parts(sink, events)
    }

    fn from_parts(sink: EventSink, events: SharedEvents) -> Self {
        let (requests, inbox) = std::sync::mpsc::channel::<Request>();
        Self {
            requests,
            inbox: Mutex::new(Some(inbox)),
            sink,
            events,
            started: AtomicBool::new(false),
        }
    }

    /// Post a request. The first one starts the worker; a closed request
    /// channel means the worker is gone (a binding failure ends it), so the
    /// request is dropped — there is nothing left that could answer it.
    pub fn send(&self, request: Request) {
        self.start_worker();
        let _ = self.requests.send(request);
    }

    /// The answers this engine produces, for the app's subscription.
    pub fn subscription(&self) -> Subscription<Event> {
        channel::subscription(Arc::clone(&self.events))
    }

    /// Start the worker, once.
    fn start_worker(&self) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(inbox) = self.inbox.lock().ok().and_then(|mut slot| slot.take()) else {
            return;
        };
        let sink = Arc::clone(&self.sink);
        // Named, so a stack dump taken during a render says where it was. A
        // spawn failure (the OS is out of threads) is not reported: the
        // receiver drops with the closure, the channel closes, and the app's
        // next request goes nowhere — which is the truth, and nothing is left
        // waiting on an answer.
        let _ = std::thread::Builder::new()
            .name("mareader-pdf".to_string())
            .spawn(move || worker(inbox, &sink));
    }

    /// An engine whose answers go to a sink of the caller's choosing: what the
    /// engine's own test uses to block on events instead of subscribing to
    /// them.
    #[cfg(test)]
    fn with_sink(sink: EventSink) -> Self {
        let (_, events) = channel::channel();
        Self::from_parts(sink, events)
    }

    /// An engine that can never start a worker: what the reader's own tests
    /// build, and the reason is Pdfium's binding, which is process-global.
    /// There is exactly one engine test in this binary, and it owns the
    /// binding; a reader test that started a worker of its own would race it
    /// for the one bind the process is allowed, and the loser would be told
    /// the library is already initialised.
    ///
    /// The door is shut the same way a started engine's is: the worker is
    /// started by taking the receiver out of this slot, and there is no
    /// receiver to take. A request then falls into a channel nobody holds —
    /// which is exactly what a request does in the app after the worker has
    /// gone: dropped, with nothing left waiting on an answer.
    #[cfg(test)]
    pub fn inert() -> Self {
        let engine = Self::new();
        if let Ok(mut slot) = engine.inbox.lock() {
            let _ = slot.take();
        }
        engine
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
