use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui::{App, AppContext, Context, Entity, Global, WeakEntity};
use rift_core::ports::{
    ConflictChoice, ConflictDecision, FileSystem, TransferCancellation, TransferConflict,
    TransferKind, TransferOptions, TransferProgress, TransferReport, TransferRequest,
};

use super::{BrowserController, SharedFileClipboard};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const COMPLETED_HISTORY_LIMIT: usize = 8;
const PROGRESS_DISPLAY_DELAY: Duration = Duration::from_millis(500);
const LARGE_TRANSFER_BYTES: u64 = 64 * 1024 * 1024;
const LARGE_TRANSFER_ENTRIES: usize = 128;
const COMPLETION_DISPLAY_DURATION: Duration = Duration::from_secs(3);

struct TransferStore(Entity<TransferTasks>);
impl Global for TransferStore {}

pub(crate) struct TransferTask {
    pub(crate) id: u64,
    pub(crate) request: TransferRequest,
    pub(crate) progress: TransferProgress,
    pub(crate) report: Option<TransferReport>,
    pub(crate) cancel_requested: bool,
    pub(crate) retry_started: bool,
    pub(crate) conflict: Option<TransferConflict>,
    pub(crate) apply_to_all: bool,
    pub(crate) visible: bool,
    keep_after_completion: bool,
    completion_expired: bool,
    display_delay_elapsed: bool,
    options: TransferOptions,
    accumulated: TransferReport,
    pending_sources: Vec<std::path::PathBuf>,
    clipboard: Option<(SharedFileClipboard, u64)>,
    cancellation: TransferCancellation,
    latest_progress: Arc<Mutex<TransferProgress>>,
    file_system: Arc<dyn FileSystem>,
    browsers: Vec<WeakEntity<BrowserController>>,
}

impl TransferTask {
    fn needs_attention(&self) -> bool {
        self.conflict.is_some()
            || self
                .report
                .as_ref()
                .is_some_and(|report| !report.failures.is_empty())
    }

    fn update_visibility(&mut self) -> bool {
        let needs_attention = self.needs_attention();
        if self.report.as_ref().is_some_and(|report| {
            report.cancelled || (self.completion_expired && !self.keep_after_completion)
        }) && !needs_attention
        {
            self.visible = false;
            return false;
        }
        let large = self
            .progress
            .total_bytes
            .is_some_and(|bytes| bytes >= LARGE_TRANSFER_BYTES)
            || self.progress.total_entries >= LARGE_TRANSFER_ENTRIES
            || self.progress.total_items >= LARGE_TRANSFER_ENTRIES;
        let show =
            needs_attention || (self.report.is_none() && self.display_delay_elapsed && large);
        let newly_visible = !self.visible && show;
        self.visible |= show;
        newly_visible
    }
}

#[derive(Default)]
pub(crate) struct TransferTasks {
    pub(crate) tasks: Vec<TransferTask>,
    pub(crate) expanded: bool,
    expanded_by_user: bool,
    next_id: u64,
}

impl TransferTasks {
    pub(crate) fn entity(cx: &mut App) -> Entity<Self> {
        if let Some(store) = cx.try_global::<TransferStore>() {
            return store.0.clone();
        }
        let tasks = cx.new(|_| Self::default());
        cx.set_global(TransferStore(tasks.clone()));
        tasks
    }

    pub(crate) fn start(
        request: TransferRequest,
        file_system: Arc<dyn FileSystem>,
        browsers: Vec<WeakEntity<BrowserController>>,
        cx: &mut App,
    ) {
        if request.sources.is_empty() {
            return;
        }
        let manager = Self::entity(cx);
        let clipboard = browsers.iter().find_map(|browser| {
            let browser = browser.upgrade()?;
            let clipboard = browser.read(cx).clipboard_handle();
            let generation = clipboard.generation();
            Some((clipboard, generation))
        });
        manager.update(cx, |manager, cx| {
            if manager.overlaps_running_move(&request) {
                uic::components::toast::error("These items are already being transferred", cx);
                return;
            }
            manager.spawn(request, file_system, browsers, clipboard, cx)
        });
    }

    fn overlaps_running_move(&self, request: &TransferRequest) -> bool {
        self.tasks.iter().any(|task| {
            task.report.is_none()
                && (task.request.kind == TransferKind::Move || request.kind == TransferKind::Move)
                && request.sources.iter().any(|source| {
                    task.request
                        .sources
                        .iter()
                        .any(|busy| source.starts_with(busy) || busy.starts_with(source))
                })
        })
    }

    fn spawn(
        &mut self,
        request: TransferRequest,
        file_system: Arc<dyn FileSystem>,
        browsers: Vec<WeakEntity<BrowserController>>,
        clipboard: Option<(SharedFileClipboard, u64)>,
        cx: &mut Context<Self>,
    ) {
        let mut successful = self
            .tasks
            .iter()
            .filter(|task| {
                task.report
                    .as_ref()
                    .is_some_and(|report| !report.cancelled && report.failures.is_empty())
            })
            .count();
        self.tasks.retain(|task| {
            if successful >= COMPLETED_HISTORY_LIMIT
                && task
                    .report
                    .as_ref()
                    .is_some_and(|report| !report.cancelled && report.failures.is_empty())
            {
                successful -= 1;
                false
            } else {
                true
            }
        });
        self.next_id += 1;
        let id = self.next_id;
        let cancellation = TransferCancellation::default();
        let progress = TransferProgress {
            total_items: request.sources.len(),
            ..Default::default()
        };
        let latest = Arc::new(Mutex::new(progress.clone()));
        self.tasks.push(TransferTask {
            id,
            request: request.clone(),
            progress,
            report: None,
            cancel_requested: false,
            retry_started: false,
            conflict: None,
            apply_to_all: false,
            visible: false,
            keep_after_completion: self.expanded_by_user,
            completion_expired: false,
            display_delay_elapsed: false,
            options: TransferOptions::default(),
            accumulated: TransferReport::default(),
            pending_sources: request.sources.clone(),
            clipboard,
            cancellation: cancellation.clone(),
            latest_progress: latest.clone(),
            file_system: file_system.clone(),
            browsers: browsers.clone(),
        });
        cx.notify();

        cx.spawn(async move |this, cx| {
            let mut elapsed = Duration::ZERO;
            loop {
                cx.background_executor().timer(PROGRESS_INTERVAL).await;
                elapsed += PROGRESS_INTERVAL;
                let Some(this) = this.upgrade() else {
                    return;
                };
                let done = this.update(cx, |manager, cx| {
                    let Some(task) = manager.tasks.iter_mut().find(|task| task.id == id) else {
                        return true;
                    };
                    if task.report.is_some() {
                        return true;
                    }
                    let latest = task
                        .latest_progress
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .clone();
                    if latest != task.progress {
                        task.progress = latest;
                        cx.notify();
                    }
                    task.display_delay_elapsed = elapsed >= PROGRESS_DISPLAY_DELAY;
                    if task.update_visibility() {
                        cx.notify();
                    }
                    false
                });
                if done {
                    return;
                }
            }
        })
        .detach();

        self.spawn_segment(id, cx);
    }

    fn spawn_segment(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(task) = self.tasks.iter().find(|task| task.id == id) else {
            return;
        };
        let request = task.request.clone();
        let request_for_work = TransferRequest {
            sources: task.pending_sources.clone(),
            ..request.clone()
        };
        let options = task.options.clone();
        let cancellation = task.cancellation.clone();
        let latest = task.latest_progress.clone();
        let file_system = task.file_system.clone();
        let browsers = task.browsers.clone();
        let clipboard = task.clipboard.clone();
        let base_bytes = task.progress.transferred_bytes;
        let base_completed = task.accumulated.completed.len();
        let base_skipped = task.accumulated.skipped.len();
        let total_items = task.request.sources.len();
        let previous_total = task.progress.total_bytes;
        let read = cx.background_spawn(async move {
            file_system.transfer_with_options(
                request_for_work,
                options,
                &cancellation,
                &mut |mut progress| {
                    progress.transferred_bytes =
                        progress.transferred_bytes.saturating_add(base_bytes);
                    progress.total_bytes = progress
                        .total_bytes
                        .map(|total| total.saturating_add(base_bytes))
                        .or(previous_total);
                    progress.completed_items += base_completed;
                    progress.skipped_items += base_skipped;
                    progress.total_items = total_items;
                    *latest.lock().unwrap_or_else(|error| error.into_inner()) = progress;
                },
            )
        });
        cx.spawn(async move |this, cx| {
            let report = read.await;
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |manager, cx| {
                if let Some(task) = manager.tasks.iter_mut().find(|task| task.id == id) {
                    task.progress = task
                        .latest_progress
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .clone();
                    task.accumulated.completed.extend(report.completed.clone());
                    task.accumulated.skipped.extend(report.skipped.clone());
                    task.accumulated.failures.extend(report.failures.clone());
                    task.pending_sources = report.remaining.clone();
                    task.conflict = report.conflict.clone();
                    // Cancellation may arrive after the worker yielded but
                    // before the conflict reaches the UI.
                    if report.cancelled || task.cancel_requested || task.conflict.is_none() {
                        task.accumulated.remaining = report.remaining.clone();
                        task.accumulated.cancelled = report.cancelled
                            || (task.cancel_requested && report.conflict.is_some());
                        task.report = Some(std::mem::take(&mut task.accumulated));
                        task.conflict = None;
                    }
                    task.update_visibility();
                    if task.needs_attention() {
                        manager.expanded = true;
                    }
                }
                manager.schedule_completion_expiry(id, cx);
                manager.reset_if_hidden();
                let moved_sources = if request.kind == TransferKind::Move {
                    report
                        .completed
                        .iter()
                        .map(|item| item.source.clone())
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                if let Some((clipboard, generation)) = clipboard {
                    clipboard.complete_move(generation, &moved_sources, cx);
                }
                // Completion belongs to the directories that were submitted,
                // not whichever tab or directory the user happens to view now.
                for browser in &browsers {
                    if let Some(browser) = browser.upgrade() {
                        browser.update(cx, |controller, cx| {
                            let current = controller.state().current_directory();
                            if current == request.directory
                                || request
                                    .sources
                                    .iter()
                                    .any(|source| source.parent() == Some(current))
                            {
                                controller.refresh_silently(cx);
                            }
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn toggle(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        self.expanded_by_user = self.expanded;
        for task in self.tasks.iter_mut().filter(|task| task.visible) {
            task.keep_after_completion = self.expanded;
            task.update_visibility();
        }
        self.reset_if_hidden();
        cx.notify();
    }

    fn schedule_completion_expiry(&mut self, id: u64, cx: &mut Context<Self>) {
        let eligible = self.tasks.iter().any(|task| {
            task.id == id
                && task.visible
                && task
                    .report
                    .as_ref()
                    .is_some_and(|report| !report.cancelled && report.failures.is_empty())
        });
        if !eligible {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(COMPLETION_DISPLAY_DURATION)
                .await;
            let Some(this) = this.upgrade() else {
                return;
            };
            this.update(cx, |manager, cx| {
                if let Some(task) = manager.tasks.iter_mut().find(|task| task.id == id) {
                    task.completion_expired = true;
                    task.update_visibility();
                }
                manager.reset_if_hidden();
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn cancel(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(task) = self
            .tasks
            .iter_mut()
            .find(|task| task.id == id && task.report.is_none())
        {
            task.cancellation.cancel();
            task.cancel_requested = true;
            if task.conflict.take().is_some() {
                task.accumulated.cancelled = true;
                task.accumulated.remaining = task.pending_sources.clone();
                task.report = Some(std::mem::take(&mut task.accumulated));
            }
            task.update_visibility();
            cx.notify();
        }
        self.reset_if_hidden();
    }

    pub(crate) fn toggle_apply_to_all(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(task) = self
            .tasks
            .iter_mut()
            .find(|task| task.id == id && task.conflict.is_some())
        {
            task.apply_to_all = !task.apply_to_all;
            cx.notify();
        }
    }

    pub(crate) fn resolve(&mut self, id: u64, choice: ConflictChoice, cx: &mut Context<Self>) {
        let Some(task) = self
            .tasks
            .iter_mut()
            .find(|task| task.id == id && !task.cancel_requested)
        else {
            return;
        };
        let Some(conflict) = task.conflict.take() else {
            return;
        };
        if choice == ConflictChoice::Replace && !conflict.replace_allowed {
            task.conflict = Some(conflict);
            return;
        }
        task.options = TransferOptions {
            decision: Some(ConflictDecision { conflict, choice }),
            policy: task.apply_to_all.then_some(choice),
        };
        self.spawn_segment(id, cx);
        cx.notify();
    }

    pub(crate) fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.tasks
            .retain(|task| task.id != id || task.report.is_none());
        self.reset_if_hidden();
        cx.notify();
    }

    pub(crate) fn clear_finished(&mut self, cx: &mut Context<Self>) {
        self.tasks.retain(|task| task.report.is_none());
        self.reset_if_hidden();
        cx.notify();
    }

    fn reset_if_hidden(&mut self) {
        if !self.tasks.iter().any(|task| task.visible) {
            self.expanded = false;
            self.expanded_by_user = false;
        }
    }

    pub(crate) fn retry(id: u64, cx: &mut App) {
        let manager = Self::entity(cx);
        manager.update(cx, |manager, cx| {
            let Some(task) = manager
                .tasks
                .iter_mut()
                .find(|task| task.id == id && !task.retry_started)
            else {
                return;
            };
            let Some(report) = &task.report else {
                return;
            };
            let sources = report.retry_sources();
            if sources.is_empty() {
                return;
            }
            let request = TransferRequest {
                sources,
                ..task.request.clone()
            };
            let file_system = task.file_system.clone();
            let browsers = task.browsers.clone();
            let clipboard = task.clipboard.clone();
            if manager.overlaps_running_move(&request) {
                uic::components::toast::error("These items are already being transferred", cx);
                return;
            }
            manager
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .unwrap()
                .retry_started = true;
            manager.spawn(request, file_system, browsers, clipboard, cx);
        });
    }
}

pub(crate) fn start_browser_transfer(
    controller: &Entity<BrowserController>,
    request: TransferRequest,
    additional_browsers: Vec<WeakEntity<BrowserController>>,
    cx: &mut App,
) {
    let file_system = controller.read(cx).file_system();
    let mut browsers = vec![controller.downgrade()];
    for browser in additional_browsers {
        if !browsers
            .iter()
            .any(|existing| existing.entity_id() == browser.entity_id())
        {
            browsers.push(browser);
        }
    }
    TransferTasks::start(request, file_system, browsers, cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use rift_core::application::{BrowserMessage, BrowserState, LoadState, SelectionMode};
    use rift_fs::LocalFileSystem;
    use std::fs;

    #[gpui::test]
    fn progress_is_delayed_compact_and_expires_when_details_are_closed(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("small.txt");
        let target = root.path().join("target");
        fs::write(&source, "small").unwrap();
        fs::create_dir(&target).unwrap();
        cx.update(|cx| {
            TransferTasks::start(
                TransferRequest {
                    kind: TransferKind::Copy,
                    sources: vec![source],
                    directory: target,
                },
                Arc::new(LocalFileSystem),
                Vec::new(),
                cx,
            )
        });
        cx.run_until_parked();
        let manager = cx.update(TransferTasks::entity);
        let completed = manager.update(cx, |manager, cx| {
            let task = &mut manager.tasks[0];
            assert!(task.report.as_ref().unwrap().failures.is_empty());
            assert!(!task.visible);
            assert!(!manager.expanded);
            // Reuse the successful fixture with controlled worker snapshots to
            // exercise the display lifecycle independently of disk speed.
            let completed = task.report.take().unwrap();
            task.progress.total_bytes = Some(LARGE_TRANSFER_BYTES - 1);
            task.display_delay_elapsed = true;
            assert!(!task.update_visibility());
            task.progress.total_bytes = Some(LARGE_TRANSFER_BYTES);
            task.display_delay_elapsed = false;
            assert!(!task.update_visibility());
            task.display_delay_elapsed = true;
            assert!(task.update_visibility());
            assert!(!manager.expanded);
            task.report = Some(completed.clone());
            task.update_visibility();
            assert!(task.visible);
            let id = task.id;
            manager.schedule_completion_expiry(id, cx);
            completed
        });
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();
        manager.update(cx, |manager, _| assert!(manager.tasks[0].visible));
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        manager.update(cx, |manager, cx| {
            let task = &mut manager.tasks[0];
            assert!(!task.visible);
            assert!(!manager.expanded);

            task.report = None;
            task.completion_expired = false;
            task.progress.total_bytes = Some(0);
            task.progress.total_entries = LARGE_TRANSFER_ENTRIES;
            assert!(task.update_visibility());
            manager.toggle(cx);
            assert!(manager.expanded);
            let task = &mut manager.tasks[0];
            task.report = Some(completed);
            task.update_visibility();
            assert!(task.visible);
            let id = task.id;
            manager.schedule_completion_expiry(id, cx);
        });
        cx.run_until_parked();
        cx.executor().advance_clock(COMPLETION_DISPLAY_DURATION);
        cx.run_until_parked();
        manager.update(cx, |manager, cx| {
            assert!(manager.tasks[0].visible);
            assert!(manager.expanded);
            manager.toggle(cx);
            assert!(!manager.tasks[0].visible);
            assert!(!manager.expanded);
            manager.clear_finished(cx);
            assert!(manager.tasks.is_empty());
            assert!(!manager.expanded);
        });
    }

    #[gpui::test]
    fn cut_paste_moves_successes_and_retains_failed_clipboard_items(cx: &mut TestAppContext) {
        cx.update(uic::init);
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        let good = root.path().join("good.txt");
        let missing = root.path().join("missing.txt");
        fs::write(&good, "good").unwrap();
        let clipboard = SharedFileClipboard::default();
        fs::write(&missing, "vanished after selection").unwrap();
        let source = cx.new(|_| {
            let mut state = BrowserState::new(root.path().to_path_buf());
            state.update(BrowserMessage::DirectoryChanged {
                path: root.path().to_path_buf(),
                entries: LocalFileSystem.read_directory(root.path()).unwrap(),
            });
            state.update(BrowserMessage::SetSelection(vec![
                good.clone(),
                missing.clone(),
            ]));
            BrowserController::with_clipboard(state, Arc::new(LocalFileSystem), clipboard.clone())
        });
        fs::remove_file(&missing).unwrap();
        let destination = cx.new(|_| {
            BrowserController::with_clipboard(
                BrowserState::new(target.clone()),
                Arc::new(LocalFileSystem),
                clipboard.clone(),
            )
        });
        source.update(cx, |controller, cx| {
            assert_eq!(controller.cut_selected(cx), 2);
        });
        cx.update(|cx| {
            let request = destination.read(cx).paste_request(target.clone()).unwrap();
            assert_eq!(request.kind, TransferKind::Move);
            start_browser_transfer(&destination, request.clone(), Vec::new(), cx);
            // Another paste while this move is pending must not create a task.
            start_browser_transfer(&destination, request, Vec::new(), cx);
            assert_eq!(TransferTasks::entity(cx).read(cx).tasks.len(), 1);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert!(!source.read(cx).is_cut(&good));
            assert!(source.read(cx).is_cut(&missing));
            assert_eq!(destination.read(cx).clipboard(), [missing]);
            let manager = TransferTasks::entity(cx);
            let report = manager.read(cx).tasks[0].report.as_ref().unwrap();
            assert_eq!(report.completed.len(), 1);
            assert_eq!(report.failures.len(), 1);
            assert!(manager.read(cx).tasks[0].visible);
            assert!(manager.read(cx).expanded);
        });
        assert!(!good.exists());
        assert_eq!(fs::read(target.join("good.txt")).unwrap(), b"good");
    }

    #[gpui::test]
    fn conflict_resolution_accumulates_progress_and_does_not_repeat_completed_items(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        for name in ["first.txt", "second.txt", "third.txt"] {
            fs::write(source.join(name), "incoming").unwrap();
        }
        for name in ["second.txt", "third.txt"] {
            fs::write(target.join(name), "existing").unwrap();
        }
        cx.update(|cx| {
            TransferTasks::start(
                TransferRequest {
                    kind: TransferKind::Copy,
                    sources: ["first.txt", "second.txt", "third.txt"]
                        .map(|name| source.join(name))
                        .to_vec(),
                    directory: target.clone(),
                },
                Arc::new(LocalFileSystem),
                Vec::new(),
                cx,
            )
        });
        cx.run_until_parked();
        let manager = cx.update(TransferTasks::entity);
        manager.update(cx, |manager, cx| {
            assert_eq!(
                manager.tasks[0].conflict.as_ref().unwrap().source,
                source.join("second.txt")
            );
            assert_eq!(manager.tasks[0].progress.completed_items, 1);
            let id = manager.tasks[0].id;
            manager.resolve(id, ConflictChoice::KeepBoth, cx);
        });
        cx.run_until_parked();
        manager.update(cx, |manager, cx| {
            assert_eq!(
                manager.tasks[0].conflict.as_ref().unwrap().source,
                source.join("third.txt")
            );
            assert_eq!(manager.tasks[0].progress.completed_items, 2);
            let id = manager.tasks[0].id;
            manager.resolve(id, ConflictChoice::Skip, cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            let task = &manager.read(cx).tasks[0];
            let report = task.report.as_ref().unwrap();
            assert_eq!(report.completed.len(), 2);
            assert_eq!(report.skipped, [source.join("third.txt")]);
            assert_eq!(task.progress.completed_items, 2);
            assert_eq!(task.progress.transferred_bytes, 16);
            assert_eq!(task.progress.skipped_items, 1);
        });
        assert_eq!(fs::read_dir(target).unwrap().count(), 4);
    }

    #[gpui::test]
    fn cancelling_a_waiting_conflict_keeps_the_cut_mark_and_can_be_retried(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        let source = root.path().join("file.txt");
        fs::write(&source, "incoming").unwrap();
        fs::write(target.join("file.txt"), "existing").unwrap();
        let browser = cx.new(|_| {
            BrowserController::with_clipboard(
                BrowserState::new(root.path().to_path_buf()),
                Arc::new(LocalFileSystem),
                SharedFileClipboard::default(),
            )
        });
        cx.update(|cx| {
            browser.update(cx, |controller, cx| {
                controller.cut_selection(source.clone(), cx)
            });
            let request = browser.read(cx).paste_request(target.clone()).unwrap();
            start_browser_transfer(&browser, request, Vec::new(), cx);
        });
        cx.run_until_parked();
        let manager = cx.update(TransferTasks::entity);
        let id = manager.update(cx, |manager, cx| {
            assert!(manager.tasks[0].conflict.is_some());
            let id = manager.tasks[0].id;
            manager.cancel(id, cx);
            assert!(manager.tasks[0].report.as_ref().unwrap().cancelled);
            assert!(!manager.tasks[0].visible);
            assert!(!manager.expanded);
            id
        });
        cx.update(|cx| {
            assert!(browser.read(cx).is_cut(&source));
            TransferTasks::retry(id, cx);
        });
        cx.run_until_parked();
        manager.update(cx, |manager, cx| {
            let id = manager.tasks[1].id;
            assert!(manager.tasks[1].conflict.is_some());
            manager.toggle_apply_to_all(id, cx);
            manager.resolve(id, ConflictChoice::KeepBoth, cx);
        });
        cx.run_until_parked();
        cx.update(|cx| assert!(!browser.read(cx).is_cut(&source)));
        assert_eq!(fs::read(target.join("file copy.txt")).unwrap(), b"incoming");
        assert_eq!(fs::read(target.join("file.txt")).unwrap(), b"existing");
    }

    #[gpui::test]
    fn retry_submits_only_incomplete_sources(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        let good = root.path().join("good.txt");
        let missing = root.path().join("missing.txt");
        fs::write(&good, "good").unwrap();
        let manager = cx.update(TransferTasks::entity);
        cx.update(|cx| {
            TransferTasks::start(
                TransferRequest {
                    kind: TransferKind::Copy,
                    sources: vec![good, missing.clone()],
                    directory: target.clone(),
                },
                Arc::new(LocalFileSystem),
                Vec::new(),
                cx,
            )
        });
        cx.run_until_parked();
        let id = cx.update(|cx| {
            let state = manager.read(cx);
            let task = &state.tasks[0];
            assert_eq!(task.report.as_ref().unwrap().completed.len(), 1);
            assert_eq!(task.report.as_ref().unwrap().failures.len(), 1);
            task.id
        });
        fs::write(&missing, "now exists").unwrap();
        cx.update(|cx| TransferTasks::retry(id, cx));
        cx.run_until_parked();
        cx.update(|cx| {
            let state = manager.read(cx);
            assert_eq!(state.tasks.len(), 2);
            assert_eq!(state.tasks[1].request.sources, [missing]);
            assert!(state.tasks[1].report.as_ref().unwrap().failures.is_empty());
        });
        assert_eq!(fs::read_dir(target).unwrap().count(), 2);
    }

    #[gpui::test]
    fn completion_does_not_refresh_the_directory_user_navigated_to(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        let other = root.path().join("other");
        fs::create_dir(&target).unwrap();
        fs::create_dir(&other).unwrap();
        let source = root.path().join("source.txt");
        fs::write(&source, "data").unwrap();
        let active = other.join("active.txt");
        fs::write(&active, "keep selected").unwrap();
        let controller = cx.new(|_| {
            BrowserController::with_clipboard(
                BrowserState::new(target.clone()),
                Arc::new(LocalFileSystem),
                super::super::SharedFileClipboard::default(),
            )
        });
        // Change the browser before the worker completion is delivered.
        cx.update(|cx| {
            start_browser_transfer(
                &controller,
                TransferRequest {
                    kind: TransferKind::Copy,
                    sources: vec![source],
                    directory: target.clone(),
                },
                Vec::new(),
                cx,
            );
            controller.update(cx, |controller, cx| {
                controller.dispatch(BrowserMessage::Navigate(other.clone()), cx)
            });
        });
        cx.run_until_parked();
        controller.update(cx, |controller, cx| {
            controller.dispatch(
                BrowserMessage::Select {
                    path: active.clone(),
                    mode: SelectionMode::Replace,
                },
                cx,
            )
        });
        cx.update(|cx| {
            let state = controller.read(cx).state();
            assert_eq!(state.current_directory(), other);
            assert_eq!(state.load_state(), &LoadState::Idle);
            assert_eq!(state.active_selection(), Some(active.as_path()));
            assert_eq!(state.snapshot_revision(), 1);
        });
        assert!(target.join("source.txt").exists());
    }
}
