use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui::{App, AppContext, Context, Entity, Global, WeakEntity};
use rift_core::ports::{
    FileSystem, TransferCancellation, TransferKind, TransferProgress, TransferReport,
    TransferRequest,
};

use super::BrowserController;

const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const COMPLETED_HISTORY_LIMIT: usize = 8;

struct TransferStore(Entity<TransferTasks>);
impl Global for TransferStore {}

pub(crate) struct TransferTask {
    pub(crate) id: u64,
    pub(crate) request: TransferRequest,
    pub(crate) progress: TransferProgress,
    pub(crate) report: Option<TransferReport>,
    pub(crate) cancel_requested: bool,
    pub(crate) retry_started: bool,
    cancellation: TransferCancellation,
    latest_progress: Arc<Mutex<TransferProgress>>,
    file_system: Arc<dyn FileSystem>,
    browsers: Vec<WeakEntity<BrowserController>>,
}

#[derive(Default)]
pub(crate) struct TransferTasks {
    pub(crate) tasks: Vec<TransferTask>,
    pub(crate) expanded: bool,
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
        manager.update(cx, |manager, cx| {
            manager.spawn(request, file_system, browsers, cx)
        });
    }

    fn spawn(
        &mut self,
        request: TransferRequest,
        file_system: Arc<dyn FileSystem>,
        browsers: Vec<WeakEntity<BrowserController>>,
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
            cancellation: cancellation.clone(),
            latest_progress: latest.clone(),
            file_system: file_system.clone(),
            browsers: browsers.clone(),
        });
        self.expanded = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_INTERVAL).await;
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
                    false
                });
                if done {
                    return;
                }
            }
        })
        .detach();

        let request_for_work = request.clone();
        let read = cx.background_spawn(async move {
            file_system.transfer(request_for_work, &cancellation, &mut |progress| {
                *latest.lock().unwrap_or_else(|error| error.into_inner()) = progress;
            })
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
                    task.report = Some(report.clone());
                }
                let moved_sources = if request.kind == TransferKind::Move {
                    report
                        .completed
                        .iter()
                        .map(|item| item.source.clone())
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                // Completion belongs to the directories that were submitted,
                // not whichever tab or directory the user happens to view now.
                for browser in &browsers {
                    if let Some(browser) = browser.upgrade() {
                        browser.update(cx, |controller, cx| {
                            controller.forget_clipboard_paths(&moved_sources);
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
        cx.notify();
    }

    pub(crate) fn cancel(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(task) = self
            .tasks
            .iter_mut()
            .find(|task| task.id == id && task.report.is_none())
        {
            task.cancellation.cancel();
            task.cancel_requested = true;
            cx.notify();
        }
    }

    pub(crate) fn dismiss(&mut self, id: u64, cx: &mut Context<Self>) {
        self.tasks
            .retain(|task| task.id != id || task.report.is_none());
        cx.notify();
    }

    pub(crate) fn clear_finished(&mut self, cx: &mut Context<Self>) {
        self.tasks.retain(|task| task.report.is_none());
        cx.notify();
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
            task.retry_started = true;
            let request = TransferRequest {
                sources,
                ..task.request.clone()
            };
            let file_system = task.file_system.clone();
            let browsers = task.browsers.clone();
            manager.spawn(request, file_system, browsers, cx);
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
