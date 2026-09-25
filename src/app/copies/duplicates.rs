//! The duplicate queue: one entry at a time, each counter name counted against
//! the level as the last landing left it.

use crate::app::Mareader;
use crate::app::message::Message;
use crate::app::walk::Stage;

use super::{DupWork, partition_store_results};
use crate::library::duplicate::{self, DupPlan, Duplicated};
use crate::platform::now_ms;
use crate::ui::toast::Tone;
use iced::Task;
use iced::time::Instant;
use library_core::wire::{BookFileRequest, StoreResult};
use std::collections::HashMap;

impl Mareader {
    /// The store's answer for one duplicate run: land what came home, count
    /// it for the report, toast what the store refused, and let the queue
    /// walk on.
    pub(super) fn duplicates_done(&mut self, work: DupWork, results: Vec<StoreResult>) -> Task<Message> {
        let (copies, failure) = partition_store_results(results);
        let now = now_ms();
        let recorded = match work {
            DupWork::Book(copy) => copies.get(&copy.new_id).map(|(store, measured)| {
                let title = duplicate::land_book(
                    &mut self.library.books,
                    &mut self.library.shelves,
                    &self.shelf,
                    *copy,
                    store.clone(),
                    *measured,
                    now,
                );
                Duplicated { name: title, shelf: false }
            }),
            DupWork::Tree(plan) => {
                let name = duplicate::land_tree(
                    &mut self.library.books,
                    &mut self.library.shelves,
                    plan,
                    &copies,
                    now,
                );
                Some(Duplicated { name, shelf: true })
            }
        };
        if let Some(one) = recorded {
            self.dup_landed.push(one);
        }
        if let Some(error) = failure {
            self.toasts.show(Tone::Error, error, Instant::now());
        }
        self.pump_dup()
    }

    /// The duplicate queue's step: one entry at a time, because each counter
    /// name counts against the level as the last landing left it — the web's
    /// own sequential loop, walked by messages instead of an async fn. The
    /// entries that owe no bytes land on the spot and the walk continues;
    /// the ones that do ride a run, and the queue resumes when its copies
    /// come home.
    pub(in crate::app) fn pump_dup(&mut self) -> Task<Message> {
        while let Some(entry) = self.dup_queue.first().cloned() {
            self.dup_queue.remove(0);
            let now = now_ms();
            let plan =
                duplicate::plan_one(&self.library.books, &self.library.shelves, &entry, now);
            match plan {
                DupPlan::Skip => {}
                DupPlan::Dead(note) => {
                    self.toasts.show(Tone::Error, note, Instant::now());
                }
                DupPlan::ShelfLink { row_id, name, target } => {
                    let title = duplicate::land_shelf_link(
                        &mut self.library.books,
                        &mut self.library.shelves,
                        &self.shelf,
                        &row_id,
                        &name,
                        &target,
                        now,
                    );
                    self.dup_landed.push(Duplicated { name: title, shelf: false });
                }
                DupPlan::Book(copy) => {
                    let label = copy.shown.clone();
                    let requests = vec![BookFileRequest {
                        from: copy.book.path().to_string(),
                        id: copy.new_id.clone(),
                    }];
                    return self.begin_dup(label, requests, DupWork::Book(copy));
                }
                DupPlan::Tree(plan) => {
                    let requests = duplicate::tree_requests(&plan);
                    if requests.is_empty() {
                        // A tree of links and dead rows copies nothing: no
                        // card, and the landing is the whole run.
                        let name = duplicate::land_tree(
                            &mut self.library.books,
                            &mut self.library.shelves,
                            plan,
                            &HashMap::new(),
                            now,
                        );
                        self.dup_landed.push(Duplicated { name, shelf: true });
                        continue;
                    }
                    let label = plan.label.clone();
                    return self.begin_dup(label, requests, DupWork::Tree(plan));
                }
            }
        }
        // The queue ran out: the report the whole batch ends with, and the
        // one persist that covers it — a link-only batch, which never met a
        // run, lands here too.
        if self.dup_landed.is_empty() {
            return Task::none();
        }
        let report = duplicate::report(&self.dup_landed);
        self.dup_landed.clear();
        self.toasts.show(Tone::Info, report, Instant::now());
        self.persist_library()
    }

    /// A duplicate's store run: the card wears the name of what is being
    /// duplicated, the way a folder run wears its folder.
    fn begin_dup(
        &mut self,
        label: String,
        requests: Vec<BookFileRequest>,
        work: DupWork,
    ) -> Task<Message> {
        self.begin_store_run(label, requests, Stage::Duplicating { work: Box::new(work) })
    }
}
