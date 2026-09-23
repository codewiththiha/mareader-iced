//! The progress channel between a blocking filesystem run and the Elm loop.
//!
//! The web app's runs emitted Tauri events the frontend re-broadcast as
//! window events; natively the worker holds a [`ProgressSink`] — the send
//! half of an unbounded channel — and the app subscribes to the receive
//! half through a [`Recipe`], iced_futures' sanctioned extension point for
//! streams the facade's constructors cannot express (they take plain `fn`
//! builders, and a receiver is state, not a function).
//!
//! The lifecycle is the tracker's: the subscription lives while the app
//! returns its recipe (keyed by the run's id), and closes the moment it
//! stops — a finished run costs nothing idle, and a second run never mixes
//! beats with the first because the ids differ.

use std::hash::Hasher as _;
use std::sync::{Arc, Mutex};

use iced::Subscription;
use iced::futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use iced::futures::stream::{self, BoxStream};
use iced_futures::subscription::{from_recipe, EventStream, Hasher, Recipe};

use library_core::wire::ImportProgress;

/// The send half a worker holds: cloneable, thread-safe, and forgiving — a
/// beat nobody is listening for is dropped, never an error.
pub type ProgressSink = Arc<dyn Fn(&ImportProgress) + Send + Sync + 'static>;

/// The receive half, parked until the subscription's first build takes it.
/// Shared rather than moved because `subscription(&self)` cannot hand
/// ownership out; the recipe takes it on first build, and rebuilds of the
/// same id are ignored by the tracker anyway.
pub type SharedProgress = Arc<Mutex<Option<UnboundedReceiver<ImportProgress>>>>;

/// One channel pair for one run.
pub fn channel() -> (ProgressSink, SharedProgress) {
    let (tx, rx): (UnboundedSender<ImportProgress>, UnboundedReceiver<ImportProgress>) =
        unbounded();
    let sink: ProgressSink = Arc::new(move |beat: &ImportProgress| {
        // A closed channel means the app stopped listening (the run was
        // abandoned); the worker winds down on its own and the beat is
        // nobody's to keep.
        let _ = tx.unbounded_send(beat.clone());
    });
    (sink, Arc::new(Mutex::new(Some(rx))))
}

/// The subscription for one run, keyed by its id.
pub fn subscription(task: u64, rx: SharedProgress) -> Subscription<ImportProgress> {
    from_recipe(ProgressRecipe { task, rx })
}

struct ProgressRecipe {
    task: u64,
    rx: SharedProgress,
}

impl Recipe for ProgressRecipe {
    type Output = ImportProgress;

    fn hash(&self, state: &mut Hasher) {
        // Tagged, so the id cannot collide with another recipe family's
        // small integers in the tracker's map.
        state.write(b"mareader::progress");
        state.write_u64(self.task);
    }

    fn stream(self: Box<Self>, _input: EventStream) -> BoxStream<'static, Self::Output> {
        match self.rx.lock().ok().and_then(|mut guard| guard.take()) {
            Some(rx) => Box::pin(rx),
            // A rebuild after the first build took the receiver: an empty
            // stream the tracker discards, because the hash already exists
            // and the original stays alive.
            None => Box::pin(stream::empty()),
        }
    }
}
