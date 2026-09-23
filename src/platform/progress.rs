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
use std::time::Instant;

use iced::Subscription;
use iced::futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use iced::futures::stream::{self, BoxStream};
use iced_futures::subscription::{from_recipe, EventStream, Hasher, Recipe};

use library_core::wire::{ImportPhase, ImportProgress};

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

/// A 2 000-file folder would otherwise push 2 000 beats through the channel
/// in under a second, and the app would spend the import repainting a ring.
const EMIT_EVERY: u32 = 8;
const EMIT_INTERVAL_MS: u128 = 60;

/// The worker-side throttle: one phase, one total, and a tick per file that
/// lets through enough beats to read as alive and suppresses enough that a
/// fast disk cannot drown the Elm loop. The walk and the store's copies each
/// hold one; the throttle that kept the web app's progress ring off the
/// syscall path is this throttle, moved to the channel's side.
pub struct Emitter<'a> {
    task: String,
    phase: ImportPhase,
    /// The count so far — the worker adjusts it, and the walk adjusts the
    /// total beside it when the walk ends and the total is finally known.
    pub done: u32,
    pub total: u32,
    since_emit: u32,
    last: Instant,
    sink: &'a ProgressSink,
}

impl<'a> Emitter<'a> {
    pub fn new(task: &str, phase: ImportPhase, total: u32, sink: &'a ProgressSink) -> Self {
        Self {
            task: task.to_string(),
            phase,
            done: 0,
            total,
            since_emit: 0,
            last: Instant::now(),
            sink,
        }
    }

    /// A dropped beat is not an error: the next one carries the same totals
    /// and the final one is always flushed. The first file emits too — a
    /// three-file import that showed nothing until its final flush would
    /// read as a hang — and after that the throttle holds.
    pub fn tick(&mut self, name: &str) {
        self.done = self.done.saturating_add(1);
        self.since_emit = self.since_emit.saturating_add(1);
        if self.done > 1
            && self.since_emit < EMIT_EVERY
            && self.last.elapsed().as_millis() < EMIT_INTERVAL_MS
        {
            return;
        }
        self.flush(name);
    }

    pub fn flush(&mut self, name: &str) {
        self.since_emit = 0;
        self.last = Instant::now();
        (self.sink)(&ImportProgress {
            task: self.task.clone(),
            phase: self.phase,
            done: self.done,
            total: self.total,
            name: name.to_string(),
        });
    }
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
