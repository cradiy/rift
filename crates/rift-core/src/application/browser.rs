use std::{
    cmp::Ordering,
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use crate::{
    domain::{Entry, is_trash_location},
    ports::FileSystemError,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Grid,
    List,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortField {
    Name,
    Modified,
    Size,
    Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub field: SortField,
    pub direction: SortDirection,
    pub directories_first: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            field: SortField::Name,
            direction: SortDirection::Ascending,
            directories_first: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionMode {
    Replace,
    Toggle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadState {
    Idle,
    Loading { path: PathBuf, request_id: u64 },
    Failed { path: PathBuf, message: String },
}

#[derive(Clone, Debug)]
pub struct BrowserState {
    current_directory: PathBuf,
    entries: Vec<Entry>,
    selection: BTreeSet<PathBuf>,
    active_selection: Option<PathBuf>,
    selection_anchor: Option<PathBuf>,
    selection_after_load: Option<PathBuf>,
    history: Vec<PathBuf>,
    history_index: usize,
    view_mode: ViewMode,
    sort: SortSpec,
    show_hidden_files: bool,
    load_state: LoadState,
    next_request_id: u64,
    snapshot_revision: u64,
}

#[derive(Clone, Debug)]
pub enum BrowserMessage {
    Navigate(PathBuf),
    GoBack,
    GoForward,
    GoUp,
    Refresh,
    RefreshSelecting(PathBuf),
    SetViewMode(ViewMode),
    SetSort(SortSpec),
    SetShowHiddenFiles(bool),
    Select {
        path: PathBuf,
        mode: SelectionMode,
    },
    SetSelection(Vec<PathBuf>),
    SetSelectionState {
        paths: Vec<PathBuf>,
        active: PathBuf,
        anchor: PathBuf,
    },
    ClearSelection,
    SelectAll,
    DirectoryLoaded {
        path: PathBuf,
        request_id: u64,
        entries: Vec<Entry>,
    },
    DirectoryChanged {
        path: PathBuf,
        entries: Vec<Entry>,
    },
    DirectoryLoadFailed {
        path: PathBuf,
        request_id: u64,
        error: FileSystemError,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowserEffect {
    ReadDirectory { path: PathBuf, request_id: u64 },
}

impl BrowserState {
    pub fn new(initial_directory: PathBuf) -> Self {
        Self {
            history: vec![initial_directory.clone()],
            current_directory: initial_directory,
            entries: Vec::new(),
            selection: BTreeSet::new(),
            active_selection: None,
            selection_anchor: None,
            selection_after_load: None,
            history_index: 0,
            view_mode: ViewMode::Grid,
            sort: SortSpec::default(),
            show_hidden_files: false,
            load_state: LoadState::Idle,
            next_request_id: 0,
            snapshot_revision: 0,
        }
    }

    pub fn current_directory(&self) -> &Path {
        &self.current_directory
    }

    pub fn is_trash(&self) -> bool {
        is_trash_location(&self.current_directory)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn selection(&self) -> &BTreeSet<PathBuf> {
        &self.selection
    }

    pub fn active_selection(&self) -> Option<&Path> {
        self.active_selection.as_deref()
    }

    pub fn selection_anchor(&self) -> Option<&Path> {
        self.selection_anchor.as_deref()
    }

    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    pub fn sort(&self) -> SortSpec {
        self.sort
    }

    pub fn show_hidden_files(&self) -> bool {
        self.show_hidden_files
    }

    pub fn visible_entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|entry| self.show_hidden_files || !entry.is_hidden())
    }

    pub fn load_state(&self) -> &LoadState {
        &self.load_state
    }

    pub fn snapshot_revision(&self) -> u64 {
        self.snapshot_revision
    }

    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    pub fn can_go_up(&self) -> bool {
        !self.is_trash()
            && self
                .current_directory
                .parent()
                .is_some_and(|parent| parent != self.current_directory)
    }

    pub fn update(&mut self, message: BrowserMessage) -> Vec<BrowserEffect> {
        match message {
            BrowserMessage::Navigate(path) => {
                if path == self.current_directory {
                    return self.start_load();
                }
                self.selection_after_load = None;
                self.history.truncate(self.history_index + 1);
                self.history.push(path.clone());
                self.history_index = self.history.len() - 1;
                self.current_directory = path;
                self.clear_selection();
                self.start_load()
            }
            BrowserMessage::GoBack if self.can_go_back() => {
                self.selection_after_load = None;
                self.history_index -= 1;
                self.current_directory = self.history[self.history_index].clone();
                self.clear_selection();
                self.start_load()
            }
            BrowserMessage::GoForward if self.can_go_forward() => {
                self.selection_after_load = None;
                self.history_index += 1;
                self.current_directory = self.history[self.history_index].clone();
                self.clear_selection();
                self.start_load()
            }
            BrowserMessage::GoUp if self.can_go_up() => {
                let child = self.current_directory.clone();
                let parent = self
                    .current_directory
                    .parent()
                    .expect("can_go_up guarantees a parent")
                    .to_path_buf();
                self.history.truncate(self.history_index + 1);
                self.history.push(parent.clone());
                self.history_index = self.history.len() - 1;
                self.current_directory = parent;
                self.clear_selection();
                self.selection_after_load = Some(child);
                self.start_load()
            }
            BrowserMessage::Refresh => self.start_load(),
            BrowserMessage::RefreshSelecting(path) => {
                self.selection_after_load = Some(path);
                self.start_load()
            }
            BrowserMessage::SetViewMode(mode) => {
                self.view_mode = mode;
                Vec::new()
            }
            BrowserMessage::SetSort(sort) => {
                self.sort = sort;
                self.sort_entries();
                Vec::new()
            }
            BrowserMessage::SetShowHiddenFiles(show) => {
                self.show_hidden_files = show;
                if !show {
                    self.selection.retain(|path| {
                        self.entries
                            .iter()
                            .find(|entry| entry.path() == path)
                            .is_some_and(|entry| !entry.is_hidden())
                    });
                    self.reconcile_selection_focus();
                }
                Vec::new()
            }
            BrowserMessage::Select { path, mode } => {
                match mode {
                    SelectionMode::Replace => {
                        self.select_single(path);
                    }
                    SelectionMode::Toggle => {
                        if !self.selection.remove(&path) {
                            self.selection.insert(path.clone());
                            self.active_selection = Some(path.clone());
                            self.selection_anchor = Some(path);
                        } else {
                            self.reconcile_selection_focus();
                        }
                    }
                }
                Vec::new()
            }
            BrowserMessage::SetSelection(paths) => {
                self.selection = paths.into_iter().collect();
                self.reconcile_selection_focus();
                Vec::new()
            }
            BrowserMessage::SetSelectionState {
                paths,
                active,
                anchor,
            } => {
                self.selection = paths.into_iter().collect();
                if self.selection.contains(&active) {
                    self.active_selection = Some(active);
                } else {
                    self.active_selection = self.selection.iter().next().cloned();
                }
                if self.selection.contains(&anchor) {
                    self.selection_anchor = Some(anchor);
                } else {
                    self.selection_anchor = self.active_selection.clone();
                }
                Vec::new()
            }
            BrowserMessage::ClearSelection => {
                self.clear_selection();
                Vec::new()
            }
            BrowserMessage::SelectAll => {
                let paths = self
                    .entries
                    .iter()
                    .filter(|entry| self.show_hidden_files || !entry.is_hidden())
                    .map(|entry| entry.path().to_path_buf())
                    .collect::<Vec<_>>();
                self.active_selection = paths.first().cloned();
                self.selection_anchor = self.active_selection.clone();
                self.selection = paths.into_iter().collect();
                Vec::new()
            }
            BrowserMessage::DirectoryLoaded {
                path,
                request_id,
                entries,
            } if self.is_current_request(&path, request_id) => {
                self.replace_entries_preserving_selection(entries);
                if let Some(path) = self.selection_after_load.take()
                    && self.entries.iter().any(|entry| {
                        entry.path() == path && (self.show_hidden_files || !entry.is_hidden())
                    })
                {
                    self.select_single(path);
                }
                self.load_state = LoadState::Idle;
                self.snapshot_revision = self.snapshot_revision.wrapping_add(1);
                Vec::new()
            }
            BrowserMessage::DirectoryChanged { path, entries }
                if path == self.current_directory && matches!(self.load_state, LoadState::Idle) =>
            {
                self.replace_entries_preserving_selection(entries);
                self.snapshot_revision = self.snapshot_revision.wrapping_add(1);
                Vec::new()
            }
            BrowserMessage::DirectoryLoadFailed {
                path,
                request_id,
                error,
            } if self.is_current_request(&path, request_id) => {
                self.load_state = LoadState::Failed {
                    path,
                    message: error.to_string(),
                };
                Vec::new()
            }
            BrowserMessage::GoBack
            | BrowserMessage::GoForward
            | BrowserMessage::GoUp
            | BrowserMessage::DirectoryLoaded { .. }
            | BrowserMessage::DirectoryChanged { .. }
            | BrowserMessage::DirectoryLoadFailed { .. } => Vec::new(),
        }
    }

    fn start_load(&mut self) -> Vec<BrowserEffect> {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1);
        let path = self.current_directory.clone();
        self.load_state = LoadState::Loading {
            path: path.clone(),
            request_id,
        };
        vec![BrowserEffect::ReadDirectory { path, request_id }]
    }

    fn clear_selection(&mut self) {
        self.selection.clear();
        self.active_selection = None;
        self.selection_anchor = None;
    }

    fn select_single(&mut self, path: PathBuf) {
        self.selection.clear();
        self.selection.insert(path.clone());
        self.active_selection = Some(path.clone());
        self.selection_anchor = Some(path);
    }

    fn reconcile_selection_focus(&mut self) {
        if self
            .active_selection
            .as_ref()
            .is_none_or(|path| !self.selection.contains(path))
        {
            self.active_selection = self.selection.iter().next().cloned();
        }
        if self
            .selection_anchor
            .as_ref()
            .is_none_or(|path| !self.selection.contains(path))
        {
            self.selection_anchor = self.active_selection.clone();
        }
    }

    fn replace_entries_preserving_selection(&mut self, entries: Vec<Entry>) {
        let had_selection = !self.selection.is_empty();
        let previous_active_index = self.active_selection.as_ref().and_then(|active| {
            self.visible_entries()
                .position(|entry| entry.path() == active)
        });

        self.entries = entries;
        self.sort_entries();
        let visible_paths = self
            .visible_entries()
            .map(|entry| entry.path().to_path_buf())
            .collect::<Vec<_>>();
        self.selection
            .retain(|path| visible_paths.iter().any(|visible| visible == path));

        if self.selection.is_empty() && had_selection && !visible_paths.is_empty() {
            let index = previous_active_index
                .unwrap_or_default()
                .min(visible_paths.len() - 1);
            self.select_single(visible_paths[index].clone());
            return;
        }

        if self
            .active_selection
            .as_ref()
            .is_none_or(|path| !self.selection.contains(path))
        {
            self.active_selection = visible_paths
                .iter()
                .find(|path| self.selection.contains(*path))
                .cloned();
        }
        if self
            .selection_anchor
            .as_ref()
            .is_none_or(|path| !self.selection.contains(path))
        {
            self.selection_anchor = self.active_selection.clone();
        }
    }

    fn is_current_request(&self, path: &Path, request_id: u64) -> bool {
        matches!(
            &self.load_state,
            LoadState::Loading {
                path: pending_path,
                request_id: pending_id,
            } if pending_path == path && *pending_id == request_id
        )
    }

    fn sort_entries(&mut self) {
        let sort = self.sort;
        self.entries.sort_by(|left, right| {
            if sort.directories_first && left.is_directory() != right.is_directory() {
                return if left.is_directory() {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }

            let field_ordering = match sort.field {
                SortField::Name => compare_names(left, right),
                SortField::Modified => compare_modified(left, right, sort.direction),
                SortField::Size => left.size().cmp(&right.size()),
                SortField::Kind => left.category().cmp(&right.category()),
            };
            let field_ordering = match (sort.field, sort.direction) {
                (SortField::Modified, _) => field_ordering,
                (_, SortDirection::Ascending) => field_ordering,
                (_, SortDirection::Descending) => field_ordering.reverse(),
            };

            field_ordering
                .then_with(|| compare_names(left, right))
                .then_with(|| left.path().cmp(right.path()))
        });
    }
}

fn compare_names(left: &Entry, right: &Entry) -> Ordering {
    left.name()
        .to_string_lossy()
        .to_lowercase()
        .cmp(&right.name().to_string_lossy().to_lowercase())
        .then_with(|| left.name().cmp(right.name()))
}

fn compare_modified(left: &Entry, right: &Entry, direction: SortDirection) -> Ordering {
    match (left.modified(), right.modified()) {
        (Some(left), Some(right)) => match direction {
            SortDirection::Ascending => left.cmp(&right),
            SortDirection::Descending => right.cmp(&left),
        },
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        path::PathBuf,
        time::{Duration, SystemTime},
    };

    use crate::domain::{Entry, EntryKind, trash_location_path};

    use super::{
        BrowserEffect, BrowserMessage, BrowserState, LoadState, SelectionMode, SortDirection,
        SortField, SortSpec, ViewMode,
    };

    #[test]
    fn trash_is_a_navigation_root() {
        let state = BrowserState::new(trash_location_path());

        assert!(state.is_trash());
        assert!(!state.can_go_up());
    }

    #[test]
    fn navigation_truncates_forward_history() {
        let mut state = BrowserState::new(PathBuf::from("/home"));
        state.update(BrowserMessage::Navigate(PathBuf::from("/home/a")));
        state.update(BrowserMessage::Navigate(PathBuf::from("/home/b")));
        state.update(BrowserMessage::GoBack);
        state.update(BrowserMessage::Navigate(PathBuf::from("/home/c")));

        assert!(!state.can_go_forward());
        assert_eq!(state.current_directory(), PathBuf::from("/home/c"));
    }

    #[test]
    fn stale_directory_results_are_ignored() {
        let mut state = BrowserState::new(PathBuf::from("/home"));
        let first = state.update(BrowserMessage::Refresh);
        let second = state.update(BrowserMessage::Navigate(PathBuf::from("/tmp")));
        let [
            BrowserEffect::ReadDirectory {
                path: first_path,
                request_id: first_id,
            },
        ] = first.as_slice()
        else {
            panic!("refresh should emit one read effect");
        };
        let [
            BrowserEffect::ReadDirectory {
                path: second_path,
                request_id: second_id,
            },
        ] = second.as_slice()
        else {
            panic!("navigation should emit one read effect");
        };

        state.update(BrowserMessage::DirectoryLoaded {
            path: first_path.clone(),
            request_id: *first_id,
            entries: Vec::new(),
        });
        assert!(matches!(state.load_state(), LoadState::Loading { .. }));

        state.update(BrowserMessage::DirectoryLoaded {
            path: second_path.clone(),
            request_id: *second_id,
            entries: Vec::new(),
        });
        assert_eq!(state.load_state(), &LoadState::Idle);
    }

    #[test]
    fn view_mode_is_pure_state_without_io_effects() {
        let mut state = BrowserState::new(PathBuf::from("/home"));
        let effects = state.update(BrowserMessage::SetViewMode(ViewMode::List));

        assert!(effects.is_empty());
        assert_eq!(state.view_mode(), ViewMode::List);
    }

    #[test]
    fn go_up_navigates_to_parent_and_enters_history() {
        let mut state = BrowserState::new(PathBuf::from("/home/me/code"));

        let effects = state.update(BrowserMessage::GoUp);

        assert_eq!(state.current_directory(), PathBuf::from("/home/me"));
        assert!(matches!(
            effects.as_slice(),
            [BrowserEffect::ReadDirectory { .. }]
        ));
        assert!(state.can_go_back());
    }

    #[test]
    fn go_up_selects_the_directory_that_was_left_after_loading_parent() {
        let child = PathBuf::from("/home/me/code");
        let parent = PathBuf::from("/home/me");
        let mut state = BrowserState::new(child.clone());

        let effects = state.update(BrowserMessage::GoUp);
        let [BrowserEffect::ReadDirectory { request_id, .. }] = effects.as_slice() else {
            panic!("go up should emit one read effect");
        };
        state.update(BrowserMessage::DirectoryLoaded {
            path: parent,
            request_id: *request_id,
            entries: vec![Entry::new(
                child.clone(),
                OsString::from("code"),
                EntryKind::Directory,
                0,
                None,
                false,
            )],
        });

        assert_eq!(state.selection().len(), 1);
        assert_eq!(state.active_selection(), Some(child.as_path()));
        assert_eq!(state.selection_anchor(), Some(child.as_path()));
    }

    #[test]
    fn refresh_selecting_restores_the_renamed_path_after_loading() {
        let directory = PathBuf::from("/home/me");
        let renamed = directory.join("renamed.txt");
        let mut state = BrowserState::new(directory.clone());

        let effects = state.update(BrowserMessage::RefreshSelecting(renamed.clone()));
        let [BrowserEffect::ReadDirectory { request_id, .. }] = effects.as_slice() else {
            panic!("refresh should emit one read effect");
        };
        state.update(BrowserMessage::DirectoryLoaded {
            path: directory,
            request_id: *request_id,
            entries: vec![Entry::new(
                renamed.clone(),
                OsString::from("renamed.txt"),
                EntryKind::File,
                12,
                None,
                false,
            )],
        });

        assert_eq!(state.selection().len(), 1);
        assert_eq!(state.active_selection(), Some(renamed.as_path()));
        assert_eq!(state.selection_anchor(), Some(renamed.as_path()));
    }

    #[test]
    fn automatic_directory_changes_preserve_selection_without_entering_loading_state() {
        let directory = PathBuf::from("/home/me");
        let first = directory.join("first.txt");
        let second = directory.join("second.txt");
        let mut state = BrowserState::new(directory.clone());
        let effects = state.update(BrowserMessage::Refresh);
        let [BrowserEffect::ReadDirectory { request_id, .. }] = effects.as_slice() else {
            panic!("refresh should emit one read effect");
        };
        state.update(BrowserMessage::DirectoryLoaded {
            path: directory.clone(),
            request_id: *request_id,
            entries: vec![
                Entry::new(
                    first.clone(),
                    OsString::from("first.txt"),
                    EntryKind::File,
                    1,
                    None,
                    false,
                ),
                Entry::new(
                    second.clone(),
                    OsString::from("second.txt"),
                    EntryKind::File,
                    2,
                    None,
                    false,
                ),
            ],
        });
        state.update(BrowserMessage::Select {
            path: second.clone(),
            mode: SelectionMode::Replace,
        });
        let revision = state.snapshot_revision();

        state.update(BrowserMessage::DirectoryChanged {
            path: directory,
            entries: vec![
                Entry::new(
                    first,
                    OsString::from("first.txt"),
                    EntryKind::File,
                    10,
                    None,
                    false,
                ),
                Entry::new(
                    second.clone(),
                    OsString::from("second.txt"),
                    EntryKind::File,
                    20,
                    None,
                    false,
                ),
            ],
        });

        assert_eq!(state.load_state(), &LoadState::Idle);
        assert_eq!(state.active_selection(), Some(second.as_path()));
        assert_eq!(state.selection_anchor(), Some(second.as_path()));
        assert_eq!(state.snapshot_revision(), revision + 1);
    }

    #[test]
    fn automatic_refresh_selects_the_next_item_when_the_active_item_disappears() {
        let directory = PathBuf::from("/home/me");
        let first = directory.join("first.txt");
        let second = directory.join("second.txt");
        let third = directory.join("third.txt");
        let mut state = BrowserState::new(directory.clone());
        let effects = state.update(BrowserMessage::Refresh);
        let [BrowserEffect::ReadDirectory { request_id, .. }] = effects.as_slice() else {
            panic!("refresh should emit one read effect");
        };
        state.update(BrowserMessage::DirectoryLoaded {
            path: directory.clone(),
            request_id: *request_id,
            entries: [first.clone(), second.clone(), third.clone()]
                .into_iter()
                .map(|path| {
                    Entry::new(
                        path.clone(),
                        path.file_name().unwrap().to_os_string(),
                        EntryKind::File,
                        1,
                        None,
                        false,
                    )
                })
                .collect(),
        });
        state.update(BrowserMessage::Select {
            path: second,
            mode: SelectionMode::Replace,
        });

        state.update(BrowserMessage::DirectoryChanged {
            path: directory,
            entries: [first, third.clone()]
                .into_iter()
                .map(|path| {
                    Entry::new(
                        path.clone(),
                        path.file_name().unwrap().to_os_string(),
                        EntryKind::File,
                        1,
                        None,
                        false,
                    )
                })
                .collect(),
        });

        assert_eq!(state.active_selection(), Some(third.as_path()));
        assert_eq!(state.selection_anchor(), Some(third.as_path()));
    }

    #[test]
    fn toggle_selection_supports_multi_select_and_removal() {
        let mut state = BrowserState::new(PathBuf::from("/home/me"));
        let first = PathBuf::from("/home/me/first");
        let second = PathBuf::from("/home/me/second");

        state.update(BrowserMessage::Select {
            path: first.clone(),
            mode: SelectionMode::Replace,
        });
        state.update(BrowserMessage::Select {
            path: second.clone(),
            mode: SelectionMode::Toggle,
        });
        state.update(BrowserMessage::Select {
            path: first.clone(),
            mode: SelectionMode::Toggle,
        });

        assert_eq!(state.selection().len(), 1);
        assert!(state.selection().contains(&second));
        assert!(!state.selection().contains(&first));
        assert_eq!(state.active_selection(), Some(second.as_path()));
        assert_eq!(state.selection_anchor(), Some(second.as_path()));
    }

    #[test]
    fn range_selection_keeps_an_explicit_active_item_and_anchor() {
        let mut state = BrowserState::new(PathBuf::from("/home/me"));
        let first = PathBuf::from("/home/me/first");
        let second = PathBuf::from("/home/me/second");
        let third = PathBuf::from("/home/me/third");

        state.update(BrowserMessage::SetSelectionState {
            paths: vec![first.clone(), second, third.clone()],
            active: third.clone(),
            anchor: first.clone(),
        });

        assert_eq!(state.selection().len(), 3);
        assert_eq!(state.active_selection(), Some(third.as_path()));
        assert_eq!(state.selection_anchor(), Some(first.as_path()));
    }

    #[test]
    fn set_selection_replaces_the_complete_selection_without_duplicates() {
        let mut state = BrowserState::new(PathBuf::from("/home/me"));
        let old = PathBuf::from("/home/me/old");
        let first = PathBuf::from("/home/me/first");
        let second = PathBuf::from("/home/me/second");

        state.update(BrowserMessage::Select {
            path: old,
            mode: SelectionMode::Replace,
        });
        state.update(BrowserMessage::SetSelection(vec![
            first.clone(),
            second.clone(),
            first.clone(),
        ]));

        assert_eq!(state.selection().len(), 2);
        assert!(state.selection().contains(&first));
        assert!(state.selection().contains(&second));
    }

    #[test]
    fn select_all_selects_every_loaded_entry() {
        let directory = PathBuf::from("/home/me");
        let first = directory.join("first");
        let second = directory.join("second.txt");
        let mut state = BrowserState::new(directory.clone());
        let effects = state.update(BrowserMessage::Refresh);
        let [BrowserEffect::ReadDirectory { request_id, .. }] = effects.as_slice() else {
            panic!("refresh should emit one read effect");
        };
        let request_id = *request_id;

        state.update(BrowserMessage::DirectoryLoaded {
            path: directory,
            request_id,
            entries: vec![
                Entry::new(
                    first.clone(),
                    OsString::from("first"),
                    EntryKind::Directory,
                    0,
                    None,
                    false,
                ),
                Entry::new(
                    second.clone(),
                    OsString::from("second.txt"),
                    EntryKind::File,
                    42,
                    None,
                    false,
                ),
            ],
        });

        state.update(BrowserMessage::SelectAll);

        assert_eq!(state.selection().len(), 2);
        assert!(state.selection().contains(&first));
        assert!(state.selection().contains(&second));
    }

    #[test]
    fn changing_sort_reorders_loaded_entries_and_keeps_directories_first() {
        let directory = PathBuf::from("/home/me");
        let mut state = BrowserState::new(directory.clone());
        let effects = state.update(BrowserMessage::Refresh);
        let [BrowserEffect::ReadDirectory { request_id, .. }] = effects.as_slice() else {
            panic!("refresh should emit one read effect");
        };
        let request_id = *request_id;
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);

        state.update(BrowserMessage::DirectoryLoaded {
            path: directory.clone(),
            request_id,
            entries: vec![
                Entry::new(
                    directory.join("small.txt"),
                    OsString::from("small.txt"),
                    EntryKind::File,
                    10,
                    Some(now),
                    false,
                ),
                Entry::new(
                    directory.join("Archive"),
                    OsString::from("Archive"),
                    EntryKind::Directory,
                    0,
                    None,
                    false,
                ),
                Entry::new(
                    directory.join("large.txt"),
                    OsString::from("large.txt"),
                    EntryKind::File,
                    100,
                    Some(now + Duration::from_secs(20)),
                    false,
                ),
            ],
        });

        state.update(BrowserMessage::SetSort(SortSpec {
            field: SortField::Size,
            direction: SortDirection::Descending,
            directories_first: true,
        }));

        let names = state
            .entries()
            .iter()
            .map(|entry| entry.name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Archive", "large.txt", "small.txt"]);
    }

    #[test]
    fn modified_sort_leaves_entries_without_dates_at_the_end() {
        let directory = PathBuf::from("/home/me");
        let mut state = BrowserState::new(directory.clone());
        state.update(BrowserMessage::SetSort(SortSpec {
            field: SortField::Modified,
            direction: SortDirection::Descending,
            directories_first: false,
        }));
        let effects = state.update(BrowserMessage::Refresh);
        let [BrowserEffect::ReadDirectory { request_id, .. }] = effects.as_slice() else {
            panic!("refresh should emit one read effect");
        };
        let request_id = *request_id;
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);

        state.update(BrowserMessage::DirectoryLoaded {
            path: directory.clone(),
            request_id,
            entries: vec![
                Entry::new(
                    directory.join("unknown"),
                    OsString::from("unknown"),
                    EntryKind::File,
                    0,
                    None,
                    false,
                ),
                Entry::new(
                    directory.join("older"),
                    OsString::from("older"),
                    EntryKind::File,
                    0,
                    Some(now),
                    false,
                ),
                Entry::new(
                    directory.join("newer"),
                    OsString::from("newer"),
                    EntryKind::File,
                    0,
                    Some(now + Duration::from_secs(20)),
                    false,
                ),
            ],
        });

        let names = state
            .entries()
            .iter()
            .map(|entry| entry.name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(names, ["newer", "older", "unknown"]);
    }

    #[test]
    fn kind_sort_orders_files_by_display_category() {
        let directory = PathBuf::from("/home/me");
        let mut state = BrowserState::new(directory.clone());
        state.update(BrowserMessage::SetSort(SortSpec {
            field: SortField::Kind,
            direction: SortDirection::Ascending,
            directories_first: true,
        }));
        let effects = state.update(BrowserMessage::Refresh);
        let [BrowserEffect::ReadDirectory { request_id, .. }] = effects.as_slice() else {
            panic!("refresh should emit one read effect");
        };

        state.update(BrowserMessage::DirectoryLoaded {
            path: directory.clone(),
            request_id: *request_id,
            entries: [
                ("main.rs", EntryKind::File),
                ("photo.png", EntryKind::File),
                ("Projects", EntryKind::Directory),
                ("notes.pdf", EntryKind::File),
                ("track.flac", EntryKind::File),
            ]
            .into_iter()
            .map(|(name, kind)| {
                Entry::new(
                    directory.join(name),
                    OsString::from(name),
                    kind,
                    0,
                    None,
                    false,
                )
            })
            .collect(),
        });

        let names = state
            .entries()
            .iter()
            .map(|entry| entry.name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "Projects",
                "notes.pdf",
                "photo.png",
                "track.flac",
                "main.rs"
            ]
        );
    }

    #[test]
    fn linux_hidden_files_are_filtered_and_never_remain_selected_when_hidden() {
        let directory = PathBuf::from("/home/me");
        let visible = directory.join("notes.txt");
        let hidden = directory.join(".secret");
        let mut state = BrowserState::new(directory.clone());
        let effects = state.update(BrowserMessage::Refresh);
        let [BrowserEffect::ReadDirectory { request_id, .. }] = effects.as_slice() else {
            panic!("refresh should emit one read effect");
        };

        state.update(BrowserMessage::DirectoryLoaded {
            path: directory,
            request_id: *request_id,
            entries: vec![
                Entry::new(
                    visible.clone(),
                    OsString::from("notes.txt"),
                    EntryKind::File,
                    0,
                    None,
                    false,
                ),
                Entry::new(
                    hidden.clone(),
                    OsString::from(".secret"),
                    EntryKind::File,
                    0,
                    None,
                    true,
                ),
            ],
        });

        assert_eq!(state.visible_entries().count(), 1);
        state.update(BrowserMessage::SetShowHiddenFiles(true));
        state.update(BrowserMessage::SelectAll);
        assert_eq!(state.selection().len(), 2);

        state.update(BrowserMessage::SetShowHiddenFiles(false));
        assert_eq!(state.visible_entries().count(), 1);
        assert_eq!(state.selection().iter().next(), Some(&visible));
        assert!(!state.selection().contains(&hidden));
    }
}
