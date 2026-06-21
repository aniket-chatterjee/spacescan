//! Interactive terminal UI: a directory browser and a reclaim view.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap,
};
use ratatui::{DefaultTerminal, Frame};

use crate::constants;
use crate::deletion::{self, DeleteMode};
use crate::export;
use crate::format::{human_size, sanitize};
use crate::metric::Metric;
use crate::node::Node;
use crate::picker::{EntryKind, FsSource, Picker};
use crate::reclaim::{self, CatAgg, Category, Hotspot};
use crate::reveal;
use crate::stats::{ext_breakdown, top_files_in, ExtStat};
use crate::theme;
use crate::util::clamp_index;

#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Browser,
    Reclaim,
}

/// A modal layer drawn on top of the active pane; it captures all input until
/// dismissed.
enum Overlay {
    Help,
    DeleteChoice(Confirm),
    Confirm(Confirm),
    Picker(Picker),
}

/// What the TUI session ended with, so the caller can re-scan a new root.
pub enum TuiOutcome {
    Quit,
    Rescan(PathBuf),
}

/// Pending deletion awaiting confirmation.
struct Confirm {
    /// Index path to the directory containing the target.
    parent_path: Vec<usize>,
    /// Index of the target within that directory's children.
    child_idx: usize,
    name: String,
    target: PathBuf,
    is_dir: bool,
    apparent: u64,
    disk: u64,
    files: u64,
    dirs: u64,
    mode: DeleteMode,
    /// Buffer for the type-the-name confirmation (permanent deletes only).
    typed: String,
}

impl Confirm {
    fn with_mode(mut self, mode: DeleteMode) -> Self {
        self.mode = mode;
        self.typed.clear();
        self
    }

    fn work_total(&self) -> u64 {
        match self.mode {
            DeleteMode::Trash => 0,
            DeleteMode::Permanent if self.is_dir => {
                self.files.saturating_add(self.dirs).saturating_add(1)
            }
            DeleteMode::Permanent => 1,
        }
    }
}

/// Delete job currently running on the worker thread.
struct Deleting {
    parent_path: Vec<usize>,
    child_idx: usize,
    name: String,
    target: PathBuf,
    is_dir: bool,
    apparent: u64,
    disk: u64,
    files: u64,
    dirs: u64,
    mode: DeleteMode,
    progress: Arc<deletion::DeleteProgress>,
    started: Instant,
    handle: Option<JoinHandle<()>>,
}

impl Deleting {
    fn into_permanent_confirm(self) -> Confirm {
        Confirm {
            parent_path: self.parent_path,
            child_idx: self.child_idx,
            name: self.name,
            target: self.target,
            is_dir: self.is_dir,
            apparent: self.apparent,
            disk: self.disk,
            files: self.files,
            dirs: self.dirs,
            mode: DeleteMode::Permanent,
            typed: String::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Size,
    Name,
    Files,
}

impl Sort {
    fn next(self) -> Self {
        match self {
            Sort::Size => Sort::Name,
            Sort::Name => Sort::Files,
            Sort::Files => Sort::Size,
        }
    }
    /// Sort caret shown on the active column header.
    fn caret(self, column: Sort) -> &'static str {
        if self == column {
            " ▾"
        } else {
            ""
        }
    }
}

struct App {
    root: Node,
    display_root: PathBuf,
    /// Indices (into each level's `children`) from the root to the current dir.
    path: Vec<usize>,
    /// Child indices of the current dir in display order.
    view: Vec<usize>,
    selected: usize,
    table_state: TableState,
    sort: Sort,
    metric: Metric,
    overlay: Option<Overlay>,
    /// Delete job currently running off the UI thread.
    deleting: Option<Deleting>,
    /// Right-top panel shows largest files when true, else file types.
    show_top: bool,
    ext_cache: Vec<ExtStat>,
    top_cache: Vec<(PathBuf, u64)>,
    message: String,
    /// True when `message` reports an error (shown red instead of green).
    message_error: bool,
    /// Which top-level pane is active.
    pane: Pane,
    /// All removable clusters found in the tree (any size).
    hotspots: Vec<Hotspot>,
    /// Per-category aggregates of `hotspots`.
    aggs: Vec<CatAgg>,
    /// Map of cluster path -> category, for inline badges in the browser.
    hotspot_map: HashMap<PathBuf, Category>,
    /// Display threshold for the reclaim list.
    min_size: u64,
    /// Indices into `hotspots` shown in the reclaim list (filtered + sorted).
    rec_order: Vec<usize>,
    rec_sel: usize,
    rec_state: TableState,
    /// Screen rect of the browser table, captured each frame for mouse hits.
    table_area: Rect,
    /// Set when the user picks a new root to scan; ends the session.
    rescan_to: Option<PathBuf>,
    /// Case-insensitive name filter applied to the current directory listing.
    filter: String,
    /// True while the user is typing into the filter.
    filter_mode: bool,
}

fn node_at<'a>(root: &'a Node, path: &[usize]) -> &'a Node {
    let mut n = root;
    for &i in path {
        n = &n.children[i];
    }
    n
}

/// Clamp `sel` into `0..len` and mirror it into `state`, clearing the selection
/// when the list is empty.
fn select_or_none(state: &mut TableState, sel: &mut usize, len: usize) {
    if len == 0 {
        *sel = 0;
        state.select(None);
    } else {
        if *sel >= len {
            *sel = len - 1;
        }
        state.select(Some(*sel));
    }
}

fn make_view(dir: &Node, sort: Sort, metric: Metric, filter: &str) -> Vec<usize> {
    let matcher = crate::util::FilterMatcher::for_filter(filter);
    let mut idx: Vec<usize> = (0..dir.children.len())
        .filter(|&i| matcher.matches(&dir.children[i].name))
        .collect();
    match sort {
        Sort::Size => idx.sort_by(|&a, &b| {
            dir.children[b]
                .size(metric)
                .cmp(&dir.children[a].size(metric))
                .then_with(|| dir.children[a].name.cmp(&dir.children[b].name))
        }),
        Sort::Name => idx.sort_by_cached_key(|&i| dir.children[i].name.to_lowercase()),
        Sort::Files => idx.sort_by(|&a, &b| {
            dir.children[b]
                .file_count
                .cmp(&dir.children[a].file_count)
                .then_with(|| {
                    dir.children[b]
                        .size(metric)
                        .cmp(&dir.children[a].size(metric))
                })
        }),
    }
    idx
}

impl App {
    fn new(
        root: Node,
        display_root: PathBuf,
        metric: Metric,
        hotspots: Vec<Hotspot>,
        aggs: Vec<CatAgg>,
        min_size: u64,
    ) -> Self {
        let hotspot_map = hotspots.iter().map(|h| (h.path.clone(), h.cat)).collect();
        let mut app = App {
            root,
            display_root,
            path: Vec::new(),
            view: Vec::new(),
            selected: 0,
            table_state: TableState::default(),
            sort: Sort::Size,
            metric,
            overlay: None,
            deleting: None,
            show_top: false,
            ext_cache: Vec::new(),
            top_cache: Vec::new(),
            message: String::new(),
            message_error: false,
            pane: Pane::Browser,
            hotspots,
            aggs,
            hotspot_map,
            min_size,
            rec_order: Vec::new(),
            rec_sel: 0,
            rec_state: TableState::default(),
            table_area: Rect::default(),
            rescan_to: None,
            filter: String::new(),
            filter_mode: false,
        };
        app.rebuild_view();
        app.refresh_caches();
        app.rebuild_rec_order();
        app
    }

    fn current_dir(&self) -> &Node {
        node_at(&self.root, &self.path)
    }

    fn current_path(&self) -> PathBuf {
        let mut p = self.display_root.clone();
        let mut n = &self.root;
        for &i in &self.path {
            n = &n.children[i];
            p.push(n.name.as_ref());
        }
        p
    }

    fn rebuild_view(&mut self) {
        let v = {
            let dir = node_at(&self.root, &self.path);
            make_view(dir, self.sort, self.metric, &self.filter)
        };
        self.view = v;
        select_or_none(&mut self.table_state, &mut self.selected, self.view.len());
    }

    fn refresh_caches(&mut self) {
        let base = self.current_path();
        let (ext, top) = {
            let dir = node_at(&self.root, &self.path);
            (
                ext_breakdown(dir, self.metric),
                top_files_in(dir, &base, constants::tui::TOP_FILES_LIMIT, self.metric),
            )
        };
        self.ext_cache = ext;
        self.top_cache = top;
    }

    fn select(&mut self, idx: usize) {
        self.selected = idx;
        select_or_none(&mut self.table_state, &mut self.selected, self.view.len());
    }

    fn move_by(&mut self, delta: isize) {
        if self.view.is_empty() {
            return;
        }
        let i = clamp_index(self.selected, self.view.len(), delta);
        self.select(i);
    }

    fn enter(&mut self) {
        if self.view.is_empty() {
            return;
        }
        let child_idx = self.view[self.selected];
        let is_dir = self.current_dir().children[child_idx].is_dir();
        if !is_dir {
            return;
        }
        self.path.push(child_idx);
        self.selected = 0;
        self.filter.clear();
        self.rebuild_view();
        self.refresh_caches();
    }

    fn leave(&mut self) {
        if let Some(last) = self.path.pop() {
            self.filter.clear();
            self.rebuild_view();
            if let Some(pos) = self.view.iter().position(|&i| i == last) {
                self.select(pos);
            }
            self.refresh_caches();
        }
    }

    /// Re-select the same child after the view order changes.
    fn reselect(&mut self, child_idx: Option<usize>) {
        if let Some(ci) = child_idx {
            if let Some(pos) = self.view.iter().position(|&i| i == ci) {
                self.select(pos);
            }
        }
    }

    fn toggle_metric(&mut self) {
        self.metric = self.metric.toggled();
        let sel_child = self.view.get(self.selected).copied();
        self.rebuild_view();
        self.reselect(sel_child);
        self.refresh_caches();
        self.rebuild_rec_order();
    }

    fn cycle_sort(&mut self) {
        self.sort = self.sort.next();
        let sel_child = self.view.get(self.selected).copied();
        self.rebuild_view();
        self.reselect(sel_child);
    }

    fn export_now(&mut self) {
        let base = self.current_path();
        let stem = sanitize(&self.current_dir().name);
        let json = format!(
            "{}{stem}{}",
            constants::files::EXPORT_JSON_PREFIX,
            constants::files::EXPORT_JSON_EXT
        );
        let csv = format!(
            "{}{stem}{}",
            constants::files::EXPORT_JSON_PREFIX,
            constants::files::EXPORT_CSV_EXT
        );
        let dir = self.current_dir();
        let r1 = export::write_json(dir, &base, Path::new(&json), false);
        let r2 = export::write_csv(dir, &base, Path::new(&csv));
        match (r1, r2) {
            (Ok(()), Ok(())) => self.set_status(&format!("Exported {json} and {csv}"), false),
            _ => self.set_status(constants::messages::EXPORT_FAILED, true),
        }
    }

    fn toggle_pane(&mut self) {
        self.pane = match self.pane {
            Pane::Browser => Pane::Reclaim,
            Pane::Reclaim => Pane::Browser,
        };
    }

    /// Recompute the filtered, sorted reclaim list (called on init and on the
    /// apparent/on-disk toggle).
    fn rebuild_rec_order(&mut self) {
        self.rec_order =
            reclaim::indices_ranked_by_size(&self.hotspots, self.metric, self.min_size);
        select_or_none(&mut self.rec_state, &mut self.rec_sel, self.rec_order.len());
    }

    fn rec_move_by(&mut self, delta: isize) {
        if self.rec_order.is_empty() {
            return;
        }
        self.rec_sel = clamp_index(self.rec_sel, self.rec_order.len(), delta);
        self.rec_state.select(Some(self.rec_sel));
    }

    fn export_reclaim(&mut self) {
        let csv = constants::files::RECLAIM_EXPORT_CSV;
        let r = export::write_reclaim_csv(&self.hotspots, self.metric, Path::new(csv));
        match r {
            Ok(()) => self.set_status(
                &format!(
                    "Exported {} removable clusters to {csv}",
                    self.hotspots.len()
                ),
                false,
            ),
            Err(_) => self.set_status(constants::messages::RECLAIM_EXPORT_FAILED, true),
        }
    }

    fn set_status(&mut self, msg: &str, is_error: bool) {
        self.message = msg.to_string();
        self.message_error = is_error;
    }

    /// Open the directory/drive picker overlay.
    fn open_picker(&mut self) {
        self.overlay = Some(Overlay::Picker(Picker::new(&FsSource)));
    }

    /// Build the guarded delete target for the selected browser entry.
    fn selected_delete_target(&mut self, mode: DeleteMode) -> Option<Confirm> {
        if self.view.is_empty() {
            return None;
        }
        let child_idx = self.view[self.selected];
        let (name, is_dir, apparent, disk, files, dirs) = {
            let child = &self.current_dir().children[child_idx];
            (
                child.name.to_string(),
                child.is_dir(),
                child.apparent_size,
                child.disk_size,
                child.file_count,
                child.dir_count(),
            )
        };
        let target = self.current_path().join(&name);
        if let Err(refusal) = deletion::guard_for(&target, &self.display_root) {
            self.set_status(refusal.reason(), true);
            return None;
        }
        Some(Confirm {
            parent_path: self.path.clone(),
            child_idx,
            name,
            target,
            is_dir,
            apparent,
            disk,
            files,
            dirs,
            mode,
            typed: String::new(),
        })
    }

    /// Let the user choose Trash or permanent deletion before confirming.
    fn begin_delete_choice(&mut self) {
        if let Some(c) = self.selected_delete_target(DeleteMode::Trash) {
            self.overlay = Some(Overlay::DeleteChoice(c));
        }
    }

    /// Open the delete-confirmation overlay for the selected browser entry.
    fn begin_delete(&mut self, mode: DeleteMode) {
        if let Some(c) = self.selected_delete_target(mode) {
            self.overlay = Some(Overlay::Confirm(c));
        }
    }

    /// Start a confirmed deletion on a worker thread so the UI can keep
    /// rendering progress while the filesystem operation runs.
    fn start_delete(&mut self, c: Confirm) {
        let progress = Arc::new(deletion::DeleteProgress::new(c.work_total()));
        let worker_progress = Arc::clone(&progress);
        let worker_target = c.target.clone();
        let mode = c.mode;
        let handle = thread::spawn(move || {
            deletion::run_with_progress(&worker_target, mode, &worker_progress);
        });

        let verb = match mode {
            DeleteMode::Trash => "Moving to Trash",
            DeleteMode::Permanent => "Deleting",
        };
        self.set_status(&format!("{verb}: {}", c.name), false);
        self.deleting = Some(Deleting {
            parent_path: c.parent_path,
            child_idx: c.child_idx,
            name: c.name,
            target: c.target,
            is_dir: c.is_dir,
            apparent: c.apparent,
            disk: c.disk,
            files: c.files,
            dirs: c.dirs,
            mode,
            progress,
            started: Instant::now(),
            handle: Some(handle),
        });
    }

    /// Check whether the worker has finished and apply the result on the UI
    /// thread. This keeps all tree and view mutation single-threaded.
    fn poll_delete(&mut self) {
        let Some(inflight) = &self.deleting else {
            return;
        };
        if !inflight.progress.is_finished() {
            return;
        }

        let mut finished = self.deleting.take().expect("checked above");
        let panicked = if let Some(handle) = finished.handle.take() {
            handle.join().is_err()
        } else {
            false
        };
        let result = finished.progress.take_result().unwrap_or_else(|| {
            if panicked {
                Err(io::Error::other("delete worker panicked"))
            } else {
                Err(io::Error::other("delete worker finished without a result"))
            }
        });

        match result {
            Ok(()) => self.complete_delete(finished),
            Err(error) if should_offer_permanent_fallback(finished.mode, &error) => {
                self.set_status(constants::messages::TRASH_FALLBACK_PROMPT, true);
                self.overlay = Some(Overlay::Confirm(finished.into_permanent_confirm()));
            }
            Err(error) => self.set_status(&delete_failure_message(finished.mode, &error), true),
        }
    }

    /// Fix up the in-memory tree after the worker has deleted the target.
    fn complete_delete(&mut self, d: Deleting) {
        if deletion::remove_subtree(&mut self.root, &d.parent_path, d.child_idx).is_none() {
            self.set_status(
                "deleted on disk; tree update failed, rescan recommended",
                true,
            );
            return;
        }
        let target = d.target.clone();
        self.hotspots.retain(|h| !h.path.starts_with(&target));
        self.hotspot_map.retain(|p, _| !p.starts_with(&target));
        self.aggs = reclaim::summarize(&self.hotspots);
        self.rebuild_rec_order();
        self.rebuild_view();
        self.refresh_caches();
        let verb = match d.mode {
            DeleteMode::Trash => "Moved to Trash",
            DeleteMode::Permanent => "Deleted",
        };
        self.set_status(
            &format!("{verb}: {} (freed {})", d.name, human_size(d.disk)),
            false,
        );
    }

    /// Route a key press while an overlay is open.
    fn overlay_key(&mut self, code: KeyCode) -> bool {
        match self.overlay.take() {
            Some(Overlay::Help) => {
                if !matches!(
                    code,
                    KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter
                ) {
                    self.overlay = Some(Overlay::Help);
                }
            }
            Some(Overlay::DeleteChoice(c)) => {
                if let KeyCode::Esc = code {
                    // cancelled: overlay stays cleared
                } else if let Some(mode) = delete_choice_key(code) {
                    self.overlay = Some(Overlay::Confirm(c.with_mode(mode)));
                } else {
                    self.overlay = Some(Overlay::DeleteChoice(c));
                }
            }
            Some(Overlay::Confirm(c)) => {
                if let KeyCode::Esc = code {
                    // cancelled: overlay stays cleared
                } else if let Some(next) = self.confirm_key(c, code) {
                    self.overlay = Some(Overlay::Confirm(next));
                }
            }
            Some(Overlay::Picker(mut p)) => match code {
                KeyCode::Esc => {}
                KeyCode::Up | KeyCode::Char('k') => {
                    p.move_by(-1);
                    self.overlay = Some(Overlay::Picker(p));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    p.move_by(1);
                    self.overlay = Some(Overlay::Picker(p));
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    p.enter(&FsSource);
                    self.overlay = Some(Overlay::Picker(p));
                }
                KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                    p.up(&FsSource);
                    self.overlay = Some(Overlay::Picker(p));
                }
                KeyCode::Enter => {
                    // Choose the highlighted location; the run loop re-scans it.
                    self.rescan_to = p.target();
                }
                _ => {
                    self.overlay = Some(Overlay::Picker(p));
                }
            },
            None => {}
        }
        false
    }

    /// Handle a key inside the delete-confirm overlay. Returns the (possibly
    /// updated) state to keep it open, or `None` once resolved/cancelled.
    fn confirm_key(&mut self, mut c: Confirm, code: KeyCode) -> Option<Confirm> {
        match c.mode {
            DeleteMode::Trash => match code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.start_delete(c);
                    None
                }
                KeyCode::Char('n') | KeyCode::Char('N') => None,
                _ => Some(c),
            },
            DeleteMode::Permanent => match code {
                KeyCode::Enter => {
                    if c.typed == c.name {
                        self.start_delete(c);
                        None
                    } else {
                        self.set_status(constants::messages::TYPE_EXACT_NAME, true);
                        Some(c)
                    }
                }
                KeyCode::Backspace => {
                    c.typed.pop();
                    Some(c)
                }
                KeyCode::Char(ch) => {
                    c.typed.push(ch);
                    Some(c)
                }
                _ => Some(c),
            },
        }
    }

    /// Handle a mouse event (ignored while an overlay is open).
    fn on_mouse(&mut self, m: MouseEvent) {
        if self.overlay.is_some() || self.deleting.is_some() {
            return;
        }
        match self.pane {
            Pane::Browser => self.browser_mouse(m),
            Pane::Reclaim => self.reclaim_mouse(m),
        }
    }

    fn browser_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::ScrollDown => self.move_by(1),
            MouseEventKind::ScrollUp => self.move_by(-1),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(idx) = self.browser_row_at(m.column, m.row) {
                    // Click selects; clicking the already-selected row opens it.
                    if idx == self.selected {
                        self.enter();
                    } else {
                        self.select(idx);
                    }
                }
            }
            _ => {}
        }
    }

    fn reclaim_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::ScrollDown => self.rec_move_by(1),
            MouseEventKind::ScrollUp => self.rec_move_by(-1),
            _ => {}
        }
    }

    /// The browser view index under a click, if it lands on a data row.
    fn browser_row_at(&self, col: u16, row: u16) -> Option<usize> {
        let area = self.table_area;
        if col < area.x || col >= area.x.saturating_add(area.width) {
            return None;
        }
        crate::util::row_at(
            area.y,
            constants::tui::MOUSE_ROW_CHROME,
            area.height,
            self.table_state.offset(),
            row,
            self.view.len(),
        )
    }

    /// Edit the live name filter while in filter-input mode.
    fn filter_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => {
                self.filter.clear();
                self.filter_mode = false;
                self.rebuild_view();
            }
            KeyCode::Enter => self.filter_mode = false,
            KeyCode::Backspace => {
                self.filter.pop();
                self.rebuild_view();
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.rebuild_view();
            }
            _ => {}
        }
        false
    }

    /// Open the current directory in the OS file manager.
    fn reveal_current(&mut self) {
        let path = self.current_path();
        self.reveal_path(&path);
    }

    fn reveal_path(&mut self, path: &Path) {
        match reveal::open_in_file_manager(path) {
            Ok(()) => self.set_status(&format!("Opened {} externally", path.display()), false),
            Err(error) => self.set_status(&format!("open failed: {error}"), true),
        }
    }

    fn reveal_selected_reclaim(&mut self) {
        let Some(path) = self.selected_reclaim_path() else {
            return;
        };
        self.reveal_path(&path);
    }

    fn selected_reclaim_path(&self) -> Option<PathBuf> {
        let hotspot_index = *self.rec_order.get(self.rec_sel)?;
        Some(self.hotspots[hotspot_index].path.clone())
    }

    /// Handle a key press. Returns true when the app should quit.
    fn on_key(&mut self, code: KeyCode, _mods: KeyModifiers) -> bool {
        if self.deleting.is_some() {
            return false;
        }
        if self.overlay.is_some() {
            return self.overlay_key(code);
        }
        if self.filter_mode {
            return self.filter_key(code);
        }
        // Keys shared by both panes.
        match code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('?') => {
                self.overlay = Some(Overlay::Help);
                return false;
            }
            KeyCode::Char('o') => {
                self.open_picker();
                return false;
            }
            KeyCode::Char('r') => {
                self.toggle_pane();
                return false;
            }
            KeyCode::Char('a') => {
                self.toggle_metric();
                return false;
            }
            _ => {}
        }
        match self.pane {
            Pane::Browser => self.browser_key(code),
            Pane::Reclaim => self.reclaim_key(code),
        }
    }

    fn browser_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => {
                // Esc clears an active filter first; otherwise it quits.
                if self.filter.is_empty() {
                    return true;
                }
                self.filter.clear();
                self.rebuild_view();
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home | KeyCode::Char('g') => self.select(0),
            KeyCode::End | KeyCode::Char('G') => {
                let n = self.view.len();
                if n > 0 {
                    self.select(n - 1);
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => self.enter(),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => self.leave(),
            KeyCode::Char('s') => self.cycle_sort(),
            KeyCode::Char('f') => self.show_top = !self.show_top,
            KeyCode::Char('e') => self.export_now(),
            KeyCode::Char('d') => self.begin_delete_choice(),
            KeyCode::Char('D') => self.begin_delete(DeleteMode::Permanent),
            KeyCode::Char('/') => self.filter_mode = true,
            KeyCode::Char('O') => self.reveal_current(),
            _ => {}
        }
        false
    }

    fn reclaim_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Backspace => {
                self.pane = Pane::Browser;
            }
            KeyCode::Up | KeyCode::Char('k') => self.rec_move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.rec_move_by(1),
            KeyCode::PageUp => self.rec_move_by(-10),
            KeyCode::PageDown => self.rec_move_by(10),
            KeyCode::Home | KeyCode::Char('g') => {
                self.rec_sel = 0;
                if !self.rec_order.is_empty() {
                    self.rec_state.select(Some(0));
                }
            }
            KeyCode::End | KeyCode::Char('G') => {
                if !self.rec_order.is_empty() {
                    self.rec_sel = self.rec_order.len() - 1;
                    self.rec_state.select(Some(self.rec_sel));
                }
            }
            KeyCode::Char('e') => self.export_reclaim(),
            KeyCode::Char('O') => self.reveal_selected_reclaim(),
            _ => {}
        }
        false
    }
}

fn delete_failure_message(mode: DeleteMode, error: &io::Error) -> String {
    let raw = error.to_string();
    if should_offer_permanent_fallback(mode, error) {
        return constants::messages::TRASH_ABORTED_HINT.to_string();
    }
    format!("delete failed: {raw}")
}

fn delete_choice_key(code: KeyCode) -> Option<DeleteMode> {
    match code {
        KeyCode::Enter | KeyCode::Char('t') | KeyCode::Char('T') => Some(DeleteMode::Trash),
        KeyCode::Char('p') | KeyCode::Char('P') | KeyCode::Char('D') => Some(DeleteMode::Permanent),
        _ => None,
    }
}

fn should_offer_permanent_fallback(mode: DeleteMode, error: &io::Error) -> bool {
    mode == DeleteMode::Trash && error.to_string().contains("Some operations were aborted")
}

/// Launch the interactive TUI.
pub fn run(
    root: Node,
    display_root: PathBuf,
    metric: Metric,
    hotspots: Vec<Hotspot>,
    aggs: Vec<CatAgg>,
    min_size: u64,
) -> io::Result<TuiOutcome> {
    let mut terminal = ratatui::init();
    let _ = execute!(io::stdout(), EnableMouseCapture);
    let mut app = App::new(root, display_root, metric, hotspots, aggs, min_size);
    let outcome = run_loop(&mut terminal, &mut app);
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    outcome
}

fn run_loop(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<TuiOutcome> {
    loop {
        app.poll_delete();
        terminal.draw(|f| draw(f, app))?;
        let poll_ms = if app.deleting.is_some() {
            constants::tui::DELETE_EVENT_POLL_MS
        } else {
            constants::tui::EVENT_POLL_MS
        };
        if event::poll(Duration::from_millis(poll_ms))? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if app.on_key(k.code, k.modifiers) {
                        return Ok(TuiOutcome::Quit);
                    }
                    if let Some(p) = app.rescan_to.take() {
                        return Ok(TuiOutcome::Rescan(p));
                    }
                }
                Event::Mouse(m) => app.on_mouse(m),
                _ => {}
            }
        }
    }
}

fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    match app.pane {
        Pane::Browser => draw_browser(f, app, area),
        Pane::Reclaim => draw_reclaim(f, app, area),
    }
    if let Some(deleting) = &app.deleting {
        draw_delete_progress(f, deleting, area);
    } else {
        match &app.overlay {
            Some(Overlay::Help) => draw_help(f, area),
            Some(Overlay::DeleteChoice(c)) => draw_delete_choice(f, c, area),
            Some(Overlay::Confirm(c)) => draw_confirm(f, c, area),
            Some(Overlay::Picker(p)) => draw_picker(f, p, area),
            None => {}
        }
    }
}

fn draw_browser(f: &mut Frame, app: &mut App, area: Rect) {
    let v = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    draw_header(f, app, v[0]);

    let body =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).split(v[1]);

    draw_table(f, app, body[0]);

    let right = Layout::vertical([Constraint::Min(5), Constraint::Length(10)]).split(body[1]);
    draw_side(f, app, right[0]);
    draw_details(f, app, right[1]);

    draw_footer(f, app, v[2]);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let dir = app.current_dir();
    let path = app.current_path();
    let l1 = Line::from(Span::styled(
        path.to_string_lossy().into_owned(),
        theme::path(),
    ));
    let l2 = Line::from(metric_summary_spans(dir, app.metric));
    let p = Paragraph::new(vec![l1, l2]).block(theme::panel(" spacescan "));
    f.render_widget(p, area);
}

/// Folder totals for the header. The active size metric is emphasized (bold and
/// colored) while the other stays muted, which removes the need for a separate
/// `[metric]` indicator. The active sort is shown by a caret on the table
/// column instead.
fn metric_summary_spans(dir: &Node, metric: Metric) -> Vec<Span<'static>> {
    let mut spans = size_pair(
        "apparent",
        dir.apparent_size,
        theme::APPARENT,
        metric == Metric::Apparent,
    );
    spans.push(Span::styled(theme::SEP, theme::label()));
    spans.extend(size_pair(
        "on-disk",
        dir.disk_size,
        theme::ON_DISK,
        metric == Metric::OnDisk,
    ));
    spans.push(Span::styled(theme::SEP, theme::label()));
    spans.push(Span::styled("files ", theme::label()));
    spans.push(Span::raw(dir.file_count.to_string()));
    spans.push(Span::styled(theme::SEP, theme::label()));
    spans.push(Span::styled("dirs ", theme::label()));
    spans.push(Span::raw(dir.dir_count().to_string()));
    spans
}

/// A muted label plus a size value; the value is emphasized when its metric is
/// the active one and dimmed otherwise.
fn size_pair(label: &str, value: u64, color: Color, active: bool) -> Vec<Span<'static>> {
    let value_style = if active {
        Style::new().fg(color).add_modifier(Modifier::BOLD)
    } else {
        theme::label()
    };
    vec![
        Span::styled(format!("{label} "), theme::label()),
        Span::styled(human_size(value), value_style),
    ]
}

/// The colored `[BADGE]` span for a directory that is a removable cluster, or
/// `None` for plain entries.
fn badge_span_for(app: &App, base: &Path, child: &Node) -> Option<Span<'static>> {
    if !child.is_dir() {
        return None;
    }
    let cat = *app.hotspot_map.get(&base.join(child.name.as_ref()))?;
    let m = cat.meta();
    Some(Span::styled(
        format!("[{}] ", m.badge),
        Style::new()
            .fg(theme::hue(m.hue))
            .add_modifier(Modifier::BOLD),
    ))
}

/// Build one table row for a child entry in the browser.
fn browser_row(
    child: &Node,
    badge: Option<Span<'static>>,
    metric: Metric,
    total: u64,
    max: u64,
) -> Row<'static> {
    let sz = child.size(metric);
    let pct = sz as f64 / total as f64 * 100.0;
    let frac = sz as f64 / max as f64;
    let mut usage_spans = theme::meter(frac, constants::tui::BROWSER_BAR_WIDTH, theme::heat(frac));
    usage_spans.push(Span::styled(format!(" {pct:>4.0}%"), theme::label()));
    let usage = Line::from(usage_spans);
    let name_style = if child.is_dir() {
        theme::path()
    } else {
        Style::new().fg(Color::Gray)
    };
    let files = if child.is_dir() {
        child.file_count.to_string()
    } else {
        String::new()
    };
    let name = if child.is_dir() {
        format!("{}/", child.name)
    } else {
        child.name.to_string()
    };
    let mut name_spans: Vec<Span> = Vec::new();
    if let Some(badge) = badge {
        name_spans.push(badge);
    }
    name_spans.push(Span::styled(name, name_style));
    Row::new(vec![
        Cell::from(usage),
        Cell::from(Line::from(human_size(sz)).right_aligned()),
        Cell::from(Line::from(files).right_aligned()),
        Cell::from(Line::from(name_spans)),
    ])
}

fn rows_for(app: &App) -> Vec<Row<'static>> {
    let dir = app.current_dir();
    let base = app.current_path();
    let total = dir.size(app.metric).max(1);
    let max = app
        .view
        .iter()
        .map(|&i| dir.children[i].size(app.metric))
        .max()
        .unwrap_or(0)
        .max(1);
    app.view
        .iter()
        .map(|&i| {
            let c = &dir.children[i];
            let badge = badge_span_for(app, &base, c);
            browser_row(c, badge, app.metric, total, max)
        })
        .collect()
}

/// Title for the browser table, showing the live filter when one is active.
pub fn browser_title_for(n: usize, filter: &str, filter_mode: bool) -> String {
    if filter_mode {
        format!(
            " Contents - filter: {filter}{} ({n}) ",
            constants::tui::FILTER_CURSOR
        )
    } else if !filter.is_empty() {
        format!(" Contents - filter: {filter} ({n}) ")
    } else {
        format!(" Contents ({n}) ")
    }
}

fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    app.table_area = area;
    let rows = rows_for(app);
    let n = rows.len();
    let widths = [
        Constraint::Length(20),
        Constraint::Length(11),
        Constraint::Length(9),
        Constraint::Fill(1),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![
                Cell::from("usage"),
                Cell::from(
                    Line::from(format!("size{}", app.sort.caret(Sort::Size))).right_aligned(),
                ),
                Cell::from(
                    Line::from(format!("files{}", app.sort.caret(Sort::Files))).right_aligned(),
                ),
                Cell::from(format!("name{}", app.sort.caret(Sort::Name))),
            ])
            .style(theme::header()),
        )
        .block(theme::panel(browser_title_for(
            n,
            &app.filter,
            app.filter_mode,
        )))
        .row_highlight_style(theme::selection())
        .highlight_symbol(theme::SELECT_SYMBOL);
    f.render_stateful_widget(table, area, &mut app.table_state);

    if n == 0 {
        let inner = Rect {
            x: area.x + 2,
            y: area.y + 2,
            width: area.width.saturating_sub(4),
            height: 1,
        };
        f.render_widget(
            Paragraph::new(constants::tui::EMPTY_DIRECTORY).style(theme::label()),
            inner,
        );
    }
}

fn draw_side(f: &mut Frame, app: &App, area: Rect) {
    let size_color = theme::metric_color(app.metric);
    if app.show_top {
        let base = app.current_path();
        let items: Vec<ListItem> = app
            .top_cache
            .iter()
            .map(|(p, s)| {
                let rel = p.strip_prefix(&base).unwrap_or(p);
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>10}", human_size(*s)),
                        Style::new().fg(size_color),
                    ),
                    Span::raw("  "),
                    Span::raw(rel.to_string_lossy().into_owned()),
                ]))
            })
            .collect();
        let list = List::new(items).block(theme::panel(" Largest files "));
        f.render_widget(list, area);
    } else {
        let total = app.current_dir().size(app.metric).max(1);
        let items: Vec<ListItem> = app
            .ext_cache
            .iter()
            .take(constants::tui::FILE_TYPES_LIMIT)
            .map(|e| {
                let sz = e.size(app.metric);
                let pct = sz as f64 / total as f64 * 100.0;
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{:>10}", human_size(sz)),
                        Style::new().fg(size_color),
                    ),
                    Span::styled(format!(" {pct:>4.0}%  "), theme::label()),
                    Span::styled(format!("{:>8}", e.count), theme::label()),
                    Span::raw(format!("  .{}", e.ext)),
                ]))
            })
            .collect();
        let list = List::new(items).block(theme::panel(" File types "));
        f.render_widget(list, area);
    }
}

/// Right-bottom panel. It deliberately avoids repeating the folder totals that
/// already sit in the header; instead it focuses on the selected entry (what an
/// action would target) plus the two folder facts the header omits: wasted
/// slack space and the raw item count.
fn draw_details(f: &mut Frame, app: &App, area: Rect) {
    let dir = app.current_dir();
    let mut lines: Vec<Line> = Vec::new();

    if let Some(sel) = selected_info(app) {
        let primary = app.metric.pick(sel.apparent, sel.disk);
        let other = app.metric.toggled().pick(sel.apparent, sel.disk);
        let other_label = app.metric.toggled().label();
        let name_style = if sel.is_dir {
            theme::path()
        } else {
            Style::new()
        };
        lines.push(Line::from(Span::styled("selected", theme::label())));
        lines.push(Line::from(Span::styled(sel.name, name_style)));
        lines.push(Line::from(vec![
            Span::styled("  size   ", theme::label()),
            Span::styled(
                human_size(primary),
                Style::new().fg(theme::metric_color(app.metric)),
            ),
            Span::styled(
                format!("  ({} {other_label})", human_size(other)),
                theme::label(),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  share  ", theme::label()),
            Span::raw(format!("{:.0}% of folder", sel.pct)),
            Span::styled(format!("{}{} files", theme::SEP, sel.files), theme::label()),
        ]));
        lines.push(Line::from(""));
    }

    let slack = dir.disk_size.saturating_sub(dir.apparent_size);
    let slack_pct = if dir.disk_size > 0 {
        slack as f64 / dir.disk_size as f64 * 100.0
    } else {
        0.0
    };
    lines.push(Line::from(vec![
        Span::styled("slack    ", theme::label()),
        Span::styled(
            format!("{} ({slack_pct:.0}%)", human_size(slack)),
            Style::new().fg(theme::DANGER),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("items    ", theme::label()),
        Span::raw(dir.children.len().to_string()),
    ]));

    let p = Paragraph::new(lines).block(theme::panel(" Details "));
    f.render_widget(p, area);
}

/// Snapshot of the currently selected entry, used by the details panel.
struct SelInfo {
    name: String,
    is_dir: bool,
    apparent: u64,
    disk: u64,
    files: u64,
    pct: f64,
}

fn selected_info(app: &App) -> Option<SelInfo> {
    let child_index = *app.view.get(app.selected)?;
    let dir = app.current_dir();
    let child = &dir.children[child_index];
    let total = dir.size(app.metric).max(1);
    Some(SelInfo {
        name: if child.is_dir() {
            format!("{}/", child.name)
        } else {
            child.name.to_string()
        },
        is_dir: child.is_dir(),
        apparent: child.apparent_size,
        disk: child.disk_size,
        files: child.file_count,
        pct: child.size(app.metric) as f64 / total as f64 * 100.0,
    })
}

/// Build the footer line: an optional colored status message, then the key
/// hints. Shared by the browser and reclaim panes.
fn footer_line(message: &str, is_error: bool, hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    if !message.is_empty() {
        let color = if is_error {
            theme::DANGER
        } else {
            theme::APPARENT
        };
        spans.push(Span::styled(message.to_string(), Style::new().fg(color)));
        spans.push(Span::styled(theme::SEP, theme::label()));
    }
    spans.extend(theme::hints(hints));
    Line::from(spans)
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(
        Paragraph::new(footer_line(
            &app.message,
            app.message_error,
            constants::tui::BROWSER_HINTS,
        )),
        area,
    );
}

/// Center a `max_w`×`max_h` modal within `area`, clamped so it always fits.
fn centered_rect(area: Rect, max_w: u16, max_h: u16) -> Rect {
    let w = max_w.min(area.width.saturating_sub(4));
    let h = max_h.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    }
}

/// One `keys → description` row in the help overlay: keys accented, padded to a
/// fixed column so the descriptions line up.
fn help_row(keys: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{keys:<12}"), Style::new().fg(theme::ACCENT)),
        Span::styled(desc.to_string(), theme::label()),
    ])
}

fn draw_help(f: &mut Frame, area: Rect) {
    let rect = centered_rect(
        area,
        constants::tui::HELP_WIDTH,
        constants::tui::HELP_HEIGHT,
    );
    let text = vec![
        help_row("↑ ↓  j k", "move selection"),
        help_row("PgUp PgDn", "jump ten rows"),
        help_row("g  G", "top / bottom"),
        help_row("→ l Enter", "open folder"),
        help_row("← h Bksp", "go to parent"),
        help_row("s", "cycle sort (size / name / files)"),
        help_row("a", "apparent ↔ on-disk size"),
        help_row("f", "files panel ↔ file types"),
        help_row("r", "reclaim view (removable clusters)"),
        help_row("/", "filter current folder by name"),
        help_row("e", "export folder (or reclaim list)"),
        help_row("d", "choose Trash or permanent delete"),
        help_row("D", "permanent delete shortcut"),
        help_row("o", "scan another folder or drive"),
        help_row("O", "open folder in file manager"),
        help_row("? Esc", "close help          q  quit"),
        Line::from(""),
        Line::from(Span::styled(
            "[BADGE] tags mark folders that are safe or easy to remove.",
            theme::label(),
        )),
        Line::from(Span::styled(
            "On-disk size rounds each file up to the cluster size.",
            theme::label(),
        )),
    ];
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(text)
            .block(theme::overlay(" Help "))
            .wrap(Wrap { trim: false }),
        rect,
    );
}

fn draw_delete_choice(f: &mut Frame, c: &Confirm, area: Rect) {
    let rect = centered_rect(
        area,
        constants::tui::CONFIRM_WIDTH,
        constants::tui::DELETE_CHOICE_HEIGHT,
    );
    let lines = vec![
        Line::from(Span::styled(c.name.to_string(), theme::path())),
        Line::from(Span::styled(
            c.target.to_string_lossy().into_owned(),
            theme::label(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(human_size(c.disk), Style::new().fg(theme::ON_DISK)),
            Span::styled(" on disk", theme::label()),
            Span::styled(
                format!("{}{} apparent", theme::SEP, human_size(c.apparent)),
                theme::label(),
            ),
            Span::styled(format!("{}{} files", theme::SEP, c.files), theme::label()),
            Span::styled(format!("{}{} dirs", theme::SEP, c.dirs), theme::label()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("t", Style::new().fg(theme::ACCENT)),
            Span::styled("  Move to Trash", Style::new().fg(theme::ON_DISK)),
            Span::styled("  recoverable", theme::label()),
        ]),
        Line::from(vec![
            Span::styled("p", Style::new().fg(theme::ACCENT)),
            Span::styled("  Delete permanently", Style::new().fg(theme::DANGER)),
            Span::styled("  requires typing the exact name", theme::label()),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            constants::messages::DELETE_CHOICE_CONFIRM,
            theme::label(),
        )),
    ];
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines)
            .block(theme::overlay(" Delete how? "))
            .wrap(Wrap { trim: true }),
        rect,
    );
}

/// The delete-confirmation modal (Trash = single confirm; Permanent = type the
/// name to confirm).
fn draw_confirm(f: &mut Frame, c: &Confirm, area: Rect) {
    let rect = centered_rect(
        area,
        constants::tui::CONFIRM_WIDTH,
        constants::tui::CONFIRM_HEIGHT,
    );
    let (title, severity) = match c.mode {
        DeleteMode::Trash => (" Move to Trash ", Color::Yellow),
        DeleteMode::Permanent => (" Permanent delete ", theme::DANGER),
    };
    let mut lines = vec![
        Line::from(Span::styled(c.name.to_string(), theme::path())),
        Line::from(Span::styled(
            c.target.to_string_lossy().into_owned(),
            theme::label(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(human_size(c.disk), Style::new().fg(theme::ON_DISK)),
            Span::styled(" on disk", theme::label()),
            Span::styled(
                format!("{}{} apparent", theme::SEP, human_size(c.apparent)),
                theme::label(),
            ),
            Span::styled(format!("{}{} files", theme::SEP, c.files), theme::label()),
            Span::styled(format!("{}{} dirs", theme::SEP, c.dirs), theme::label()),
        ]),
        Line::from(""),
    ];
    match c.mode {
        DeleteMode::Trash => {
            lines.push(Line::from(Span::styled(
                constants::messages::RECOVERABLE_DELETE,
                Style::new().fg(theme::APPARENT),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                constants::messages::TRASH_CONFIRM,
                theme::label(),
            )));
        }
        DeleteMode::Permanent => {
            lines.push(Line::from(Span::styled(
                constants::messages::PERMANENT_DELETE_WARNING,
                Style::new().fg(theme::DANGER).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(format!("Type \"{}\" to confirm:", c.name)));
            lines.push(Line::from(Span::styled(
                format!("> {}", c.typed),
                Style::new().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                constants::messages::PERMANENT_CONFIRM,
                theme::label(),
            )));
        }
    }
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::bordered()
                    .border_style(Style::new().fg(severity))
                    .title(Span::styled(
                        title,
                        Style::new().fg(severity).add_modifier(Modifier::BOLD),
                    )),
            )
            .wrap(Wrap { trim: true }),
        rect,
    );
}

fn draw_delete_progress(f: &mut Frame, d: &Deleting, area: Rect) {
    let rect = centered_rect(
        area,
        constants::tui::DELETE_PROGRESS_WIDTH,
        constants::tui::DELETE_PROGRESS_HEIGHT,
    );
    let (title, bar_color, action) = match d.mode {
        DeleteMode::Trash => (" Moving to Trash ", theme::ON_DISK, "Moving"),
        DeleteMode::Permanent => (" Deleting ", theme::DANGER, "Deleting"),
    };
    let elapsed = d.started.elapsed();
    let width = constants::tui::DELETE_PROGRESS_BAR_WIDTH;
    let (bar, detail) = if let Some(frac) = d.progress.fraction() {
        let done = d.progress.done().min(d.progress.total());
        (
            theme::meter(frac, width, bar_color),
            format!(
                "{done} / {} items  {:>3.0}%  elapsed {}",
                d.progress.total(),
                frac * 100.0,
                format_elapsed(elapsed)
            ),
        )
    } else {
        let activity = match d.mode {
            DeleteMode::Trash => "Recycle Bin operation in progress",
            DeleteMode::Permanent => "Filesystem delete in progress",
        };
        (
            indeterminate_meter(elapsed, width, bar_color),
            format!("{activity}  elapsed {}", format_elapsed(elapsed)),
        )
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(format!("{action} "), theme::label()),
            Span::styled(d.name.clone(), theme::path()),
            Span::styled(format!(" ({})", human_size(d.disk)), theme::label()),
        ]),
        Line::from(Span::styled(
            d.target.to_string_lossy().into_owned(),
            theme::label(),
        )),
        Line::from(""),
        Line::from(bar),
        Line::from(Span::styled(detail, theme::label())),
    ];
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines)
            .block(theme::overlay(title))
            .wrap(Wrap { trim: true }),
        rect,
    );
}

fn indeterminate_meter(elapsed: Duration, width: usize, fill: Color) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let pulse = (width / 3).max(4).min(width);
    let pos = ((elapsed.as_millis() / constants::tui::DELETE_EVENT_POLL_MS as u128) as usize)
        % (width + pulse);
    let start = pos.saturating_sub(pulse);
    let end = pos.min(width);
    let mut spans = Vec::new();
    if start > 0 {
        spans.push(Span::styled(
            "░".repeat(start),
            Style::new().fg(theme::MUTED),
        ));
    }
    if end > start {
        spans.push(Span::styled("█".repeat(end - start), Style::new().fg(fill)));
    }
    if end < width {
        spans.push(Span::styled(
            "░".repeat(width - end),
            Style::new().fg(theme::MUTED),
        ));
    }
    spans
}

fn format_elapsed(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

/// The directory/drive picker modal.
fn draw_picker(f: &mut Frame, p: &Picker, area: Rect) {
    let rect = centered_rect(
        area,
        constants::tui::PICKER_WIDTH,
        constants::tui::PICKER_HEIGHT,
    );
    let title = match p.location() {
        Some(loc) => format!(" Scan where?  {} ", loc.display()),
        None => " Scan where?  drives & bookmarks ".to_string(),
    };
    let items: Vec<ListItem> = p
        .entries()
        .iter()
        .map(|e| {
            let (tag, color) = match e.kind {
                EntryKind::Up => ("..", theme::MUTED),
                EntryKind::Drive => ("drive", theme::ON_DISK),
                EntryKind::Bookmark => ("mark", theme::ACCENT),
                EntryKind::Dir => ("dir", theme::MUTED),
            };
            let label_style = if e.kind == EntryKind::Up {
                theme::label()
            } else {
                Style::new()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{tag:<7} "), Style::new().fg(color)),
                Span::styled(e.label.clone(), label_style),
            ]))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(p.selected()));
    f.render_widget(Clear, rect);
    let list = List::new(items)
        .block(theme::overlay(title))
        .highlight_style(theme::selection())
        .highlight_symbol(theme::SELECT_SYMBOL);
    f.render_stateful_widget(list, rect, &mut state);
}

// ---------------------------------------------------------------------------
// Reclaim view: a visual map of removable directory clusters.
// ---------------------------------------------------------------------------

fn draw_reclaim(f: &mut Frame, app: &mut App, area: Rect) {
    let v = Layout::vertical([
        Constraint::Length(6),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    draw_reclaim_header(f, app, v[0]);

    let body =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).split(v[1]);
    draw_reclaim_list(f, app, body[0]);
    draw_reclaim_summary(f, app, body[1]);

    f.render_widget(
        Paragraph::new(footer_line(
            &app.message,
            app.message_error,
            constants::tui::RECLAIM_HINTS,
        )),
        v[2],
    );
}

/// A single stacked, proportional bar of category sizes (the "map" strip).
fn category_strip(aggs: &[CatAgg], metric: Metric, width: usize) -> Line<'static> {
    let total: u64 = aggs.iter().map(|a| a.size(metric)).sum();
    if total == 0 || width == 0 {
        return Line::from("");
    }
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0usize;
    let n = aggs.len();
    for (i, a) in aggs.iter().enumerate() {
        let sz = a.size(metric);
        let w = if i == n - 1 {
            width.saturating_sub(used)
        } else {
            (((sz as f64 / total as f64) * width as f64).round() as usize).min(width - used)
        };
        if w == 0 {
            continue;
        }
        used += w;
        spans.push(Span::styled(
            "█".repeat(w),
            Style::new().fg(theme::hue(a.cat.meta().hue)),
        ));
        if used >= width {
            break;
        }
    }
    Line::from(spans)
}

fn draw_reclaim_header(f: &mut Frame, app: &App, area: Rect) {
    let metric = app.metric;
    let total = reclaim::total_reclaimable(&app.hotspots, metric);
    let (regen, junk, review) = reclaim::safety_totals(&app.hotspots, metric);
    let metric_label = metric.label();

    let l1 = Line::from(vec![
        Span::styled("Removable clusters under ", theme::label()),
        Span::styled(
            app.display_root.to_string_lossy().into_owned(),
            theme::path(),
        ),
    ]);
    let l2 = Line::from(vec![
        Span::styled("Total ", theme::label()),
        Span::styled(human_size(total), Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(theme::SEP, theme::label()),
        Span::styled(
            format!("regenerable {}", human_size(regen)),
            Style::new().fg(theme::safety(reclaim::Safety::Regenerable)),
        ),
        Span::styled(theme::SEP, theme::label()),
        Span::styled(
            format!("junk {}", human_size(junk)),
            Style::new().fg(theme::safety(reclaim::Safety::Junk)),
        ),
        Span::styled(theme::SEP, theme::label()),
        Span::styled(
            format!("review {}", human_size(review)),
            Style::new().fg(theme::safety(reclaim::Safety::Review)),
        ),
        Span::styled(
            format!(
                "{}{} spots · showing {} ≥ {} · by {}",
                theme::SEP,
                app.hotspots.len(),
                app.rec_order.len(),
                human_size(app.min_size),
                metric_label
            ),
            theme::label(),
        ),
    ]);
    let strip = category_strip(&app.aggs, metric, area.width.saturating_sub(2) as usize);
    let p = Paragraph::new(vec![l1, l2, Line::from(""), strip]).block(theme::panel(" Reclaim "));
    f.render_widget(p, area);
}

fn draw_reclaim_list(f: &mut Frame, app: &mut App, area: Rect) {
    let metric = app.metric;
    let max = app
        .rec_order
        .first()
        .map(|&i| app.hotspots[i].size(metric))
        .unwrap_or(1)
        .max(1);
    let rows: Vec<Row> = app
        .rec_order
        .iter()
        .map(|&i| {
            let h = &app.hotspots[i];
            let m = h.cat.meta();
            let sz = h.size(metric);
            let frac = sz as f64 / max as f64;
            let bar = theme::meter(frac, constants::tui::RECLAIM_BAR_WIDTH, theme::hue(m.hue));
            let badge = Span::styled(
                format!("{:<5}", m.badge),
                Style::new()
                    .fg(theme::hue(m.hue))
                    .add_modifier(Modifier::BOLD),
            );
            Row::new(vec![
                Cell::from(Line::from(bar)),
                Cell::from(
                    Line::from(Span::styled(
                        human_size(sz),
                        Style::new().fg(theme::metric_color(metric)),
                    ))
                    .right_aligned(),
                ),
                Cell::from(Line::from(badge)),
                Cell::from(h.path.to_string_lossy().into_owned()),
            ])
        })
        .collect();
    let n = rows.len();
    let widths = [
        Constraint::Length(10),
        Constraint::Length(11),
        Constraint::Length(6),
        Constraint::Fill(1),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![
                Cell::from("usage"),
                Cell::from(Line::from("size").right_aligned()),
                Cell::from("type"),
                Cell::from("path"),
            ])
            .style(theme::header()),
        )
        .block(theme::panel(format!(" Removable spots ({n}) ")))
        .row_highlight_style(theme::selection())
        .highlight_symbol(theme::SELECT_SYMBOL);
    f.render_stateful_widget(table, area, &mut app.rec_state);

    if n == 0 {
        let inner = Rect {
            x: area.x + 2,
            y: area.y + 2,
            width: area.width.saturating_sub(4),
            height: 2,
        };
        f.render_widget(
            Paragraph::new(format!(
                "No removable clusters >= {}. Lower the bar with --min-size.",
                human_size(app.min_size)
            ))
            .style(theme::label())
            .wrap(Wrap { trim: true }),
            inner,
        );
    }
}

fn draw_reclaim_summary(f: &mut Frame, app: &App, area: Rect) {
    let metric = app.metric;
    let maxc = app
        .aggs
        .iter()
        .map(|a| a.size(metric))
        .max()
        .unwrap_or(1)
        .max(1);
    let items: Vec<ListItem> = app
        .aggs
        .iter()
        .map(|a| {
            let m = a.cat.meta();
            let sz = a.size(metric);
            let frac = sz as f64 / maxc as f64;
            let mut spans = vec![
                Span::styled(
                    format!("{:<6}", m.badge),
                    Style::new()
                        .fg(theme::hue(m.hue))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:>10} ", human_size(sz)),
                    Style::new().fg(theme::metric_color(metric)),
                ),
            ];
            spans.extend(theme::meter(
                frac,
                constants::tui::SUMMARY_BAR_WIDTH,
                theme::hue(m.hue),
            ));
            spans.push(Span::styled(format!(" {} spots ", a.count), theme::label()));
            spans.push(Span::styled(
                m.safety.label(),
                Style::new().fg(theme::safety(m.safety)),
            ));
            ListItem::new(Line::from(spans))
        })
        .collect();
    let list = List::new(items).block(theme::panel(" By category "));
    f.render_widget(list, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_sort_uses_case_insensitive_order() {
        let root = Node::dir_with_children(
            "root".to_string(),
            0,
            0,
            0,
            0,
            vec![
                Node::file("zeta.txt".to_string(), 1, 1),
                Node::file("Alpha.txt".to_string(), 1, 1),
                Node::file("beta.txt".to_string(), 1, 1),
            ],
        );

        let view = make_view(&root, Sort::Name, Metric::Apparent, "");
        let names: Vec<&str> = view
            .iter()
            .map(|&index| root.children[index].name.as_ref())
            .collect();

        assert_eq!(names, vec!["Alpha.txt", "beta.txt", "zeta.txt"]);
    }

    #[test]
    fn size_sort_orders_largest_first_with_name_ties() {
        let root = Node::dir_with_children(
            "root".to_string(),
            0,
            0,
            0,
            0,
            vec![
                Node::file("gamma.txt".to_string(), 10, 10),
                Node::file("beta.txt".to_string(), 20, 20),
                Node::file("alpha.txt".to_string(), 20, 20),
            ],
        );

        let view = make_view(&root, Sort::Size, Metric::Apparent, "");
        let names: Vec<&str> = view
            .iter()
            .map(|&index| root.children[index].name.as_ref())
            .collect();

        assert_eq!(names, vec!["alpha.txt", "beta.txt", "gamma.txt"]);
    }

    #[test]
    fn files_sort_orders_most_files_first_with_size_ties() {
        let root = Node::dir_with_children(
            "root".to_string(),
            0,
            0,
            0,
            0,
            vec![
                Node::dir_with_children("small".to_string(), 10, 10, 2, 0, Vec::new()),
                Node::dir_with_children("large".to_string(), 20, 20, 2, 0, Vec::new()),
                Node::dir_with_children("many".to_string(), 15, 15, 3, 0, Vec::new()),
            ],
        );

        let view = make_view(&root, Sort::Files, Metric::Apparent, "");
        let names: Vec<&str> = view
            .iter()
            .map(|&index| root.children[index].name.as_ref())
            .collect();

        assert_eq!(names, vec!["many", "large", "small"]);
    }

    #[test]
    fn trash_abort_error_gets_actionable_message() {
        let error = io::Error::other(r#"Unknown { description: "Some operations were aborted" }"#);

        assert!(should_offer_permanent_fallback(DeleteMode::Trash, &error));
        assert_eq!(
            delete_failure_message(DeleteMode::Trash, &error),
            constants::messages::TRASH_ABORTED_HINT
        );
    }

    #[test]
    fn non_trash_error_keeps_original_message() {
        let error = io::Error::other("permission denied");

        assert!(!should_offer_permanent_fallback(
            DeleteMode::Permanent,
            &error
        ));
        assert_eq!(
            delete_failure_message(DeleteMode::Permanent, &error),
            "delete failed: permission denied"
        );
    }

    #[test]
    fn delete_choice_keys_select_mode() {
        assert_eq!(
            delete_choice_key(KeyCode::Char('t')),
            Some(DeleteMode::Trash)
        );
        assert_eq!(delete_choice_key(KeyCode::Enter), Some(DeleteMode::Trash));
        assert_eq!(
            delete_choice_key(KeyCode::Char('p')),
            Some(DeleteMode::Permanent)
        );
        assert_eq!(
            delete_choice_key(KeyCode::Char('D')),
            Some(DeleteMode::Permanent)
        );
        assert_eq!(delete_choice_key(KeyCode::Esc), None);
    }
}
