//! The channel between the engine thread and the Elm loop.
//!
//! The same shape the import runs use (`crate::platform::progress`): the
//! worker holds a sink — a closure that may be called from any thread — and the
//! app subscribes to the receive half through a [`Recipe`], which is
//! iced_futures' sanctioned extension point for streams whose constructor
//! cannot be a plain function (a receiver is state, not a function).
//!
//! Deliberately a different recipe family from the import runs, and tagged as
//! such in its hash: the tracker keys subscriptions by hash, and two families
//! whose ids are both small integers would collide there the moment a run and
//! the engine were alive at the same time — which, on the shelf, they are.

use std::hash::Hasher as _;
use std::sync::{Arc, Mutex};

use iced::Subscription;
use iced::futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use iced::futures::stream::{self, BoxStream};
use iced_futures::subscription::{EventStream, Hasher, Recipe, from_recipe};

use super::protocol::Event;

/// The send half the worker holds: cloneable, thread-safe, and forgiving — an
/// answer nobody is listening for is dropped, never an error. A dropped answer
/// costs nothing, because the reader asks again the moment it still needs one;
/// an error would only be a sentence about a window that is closing.
pub type EventSink = Arc<dyn Fn(Event) + Send + Sync + 'static>;

/// The receive half, parked until the subscription's first build takes it.
/// Shared rather than moved because `subscription(&self)` cannot hand
/// ownership out; the recipe takes it on first build, and a rebuild of the same
/// id is ignored by the tracker anyway.
pub type SharedEvents = Arc<Mutex<Option<UnboundedReceiver<Event>>>>;

/// One channel pair for one engine.
pub fn channel() -> (EventSink, SharedEvents) {
    let (tx, rx): (UnboundedSender<Event>, UnboundedReceiver<Event>) = unbounded();
    let sink: EventSink = Arc::new(move |event: Event| {
        let _ = tx.unbounded_send(event);
    });
    (sink, Arc::new(Mutex::new(Some(rx))))
}

/// The app's subscription to the engine's answers. One engine per process, so
/// the family tag alone is the whole id.
pub fn subscription(events: SharedEvents) -> Subscription<Event> {
    from_recipe(EngineRecipe { events })
}

struct EngineRecipe {
    events: SharedEvents,
}

impl Recipe for EngineRecipe {
    type Output = Event;

    fn hash(&self, state: &mut Hasher) {
        state.write(b"mareader::pdf-engine");
    }

    fn stream(self: Box<Self>, _input: EventStream) -> BoxStream<'static, Self::Output> {
        match self.events.lock().ok().and_then(|mut guard| guard.take()) {
            Some(rx) => Box::pin(rx),
            // A rebuild after the first build took the receiver: an empty
            // stream the tracker discards, because the hash already exists and
            // the original stays alive.
            None => Box::pin(stream::empty()),
        }
    }
}
