use std::collections::HashSet;
use std::path::PathBuf;

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;

use crate::config::Config;
use crate::live::{self, Live};
use crate::locate::locate;
use crate::settings::{Op, Settings};
use crate::stats::Stats;
use crate::store::{SessionIndex, SessionInfo, Store, Task, sessions_dir};
use crate::transcript::{Tail, Transcript};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Tasks,
    Dag,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Log,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Row {
    Task { id: String, depth: usize },
    Run { id: String },
    Node { run: String, node: String },
    Info(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reasoning {
    Preview,
    Full,
    Hidden,
}

impl Reasoning {
    fn next(self) -> Self {
        match self {
            Self::Preview => Self::Full,
            Self::Full => Self::Hidden,
            Self::Hidden => Self::Preview,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Full => "full",
            Self::Hidden => "off",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Hit {
    Splitter,
    Tab(Tab),
    Picker,
    ClosePicker,
    Session(usize),
    Row(usize),
    Entry(usize),
    Follow,
    Zoom,
    Settings,
    CloseSettings,
    StatSelect(usize),
    StatBar(usize),
    StatRow(usize),
    StatUp(usize),
    StatDown(usize),
    CostMode,
    CtxDown,
    CtxUp,
    SaveSettings,
    ResetSettings,
    Pin,
    Task(String),
}

pub struct LogLine {
    pub entry: Option<usize>,
    pub task: Option<String>,
    pub line: Line<'static>,
}

pub struct LogView {
    pub task: String,
    pub tail: Option<Tail>,
    pub transcript: Transcript,
    pub lines: Vec<LogLine>,
    pub width: u16,
    pub dirty: bool,
    pub offset: usize,
    pub height: usize,
    pub follow: bool,
    pub unseen: usize,
    pub expanded: HashSet<usize>,
    pub bad: usize,
    pub error: Option<String>,
    pub live: Option<Live>,
}

impl LogView {
    fn new(task: String) -> Self {
        Self {
            task,
            tail: None,
            transcript: Transcript::default(),
            lines: Vec::new(),
            width: 0,
            dirty: true,
            offset: 0,
            height: 0,
            follow: true,
            unseen: 0,
            expanded: HashSet::new(),
            bad: 0,
            error: None,
            live: None,
        }
    }
}

pub struct Picker {
    pub sessions: Vec<SessionInfo>,
    pub selected: usize,
    pub scroll: usize,
    pub reveal: bool,
}

pub struct App {
    pub root: PathBuf,
    pub store: Store,
    pub index: SessionIndex,
    pub session: Option<String>,
    pub session_title: String,
    pub tab: Tab,
    pub rows: Vec<Row>,
    pub selected: usize,
    pub list_scroll: usize,
    pub reveal: bool,
    pub focus: Focus,
    pub zoom: bool,
    pub log: Option<LogView>,
    pub picker: Option<Picker>,
    pub config: Config,
    pub settings: Option<Settings>,
    pub hits: Vec<(Rect, Hit)>,
    pub list_area: Rect,
    pub log_area: Rect,
    pub body_area: Rect,
    pub split: Option<u16>,
    pub dragging: bool,
    pub reasoning: Reasoning,
    pub pin_open: bool,
    pub quit: bool,
    pub dirty: bool,
}

impl App {
    pub fn new(cwd: Option<PathBuf>, session: Option<String>, config: Config) -> anyhow::Result<Self> {
        let start = match cwd {
            Some(c) => c,
            None => std::env::current_dir()?,
        };
        let location = locate(&start);
        let root = location.root;
        let mut store = Store::new(location.tasks);
        store.refresh();
        let mut app = Self {
            index: SessionIndex::new(sessions_dir(&root)),
            root,
            store,
            session: None,
            session_title: String::new(),
            tab: Tab::Tasks,
            rows: Vec::new(),
            selected: 0,
            list_scroll: 0,
            reveal: true,
            focus: Focus::List,
            zoom: false,
            log: None,
            picker: None,
            config,
            settings: None,
            hits: Vec::new(),
            list_area: Rect::default(),
            log_area: Rect::default(),
            body_area: Rect::default(),
            split: None,
            dragging: false,
            reasoning: Reasoning::Preview,
            pin_open: false,
            quit: false,
            dirty: true,
        };
        let initial = session
            .or_else(|| std::env::var("PI_SESSION_ID").ok().filter(|s| !s.is_empty()))
            .or_else(|| app.index.list(&app.store).into_iter().next().map(|s| s.id));
        match initial {
            Some(id) => app.set_session(id),
            None => {
                app.rebuild_rows();
                app.open_picker();
            }
        }
        Ok(app)
    }

    pub fn set_session(&mut self, id: String) {
        self.session_title = self
            .index
            .list(&self.store)
            .into_iter()
            .find(|s| s.id == id)
            .map(|s| s.title)
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| id.chars().take(13).collect());
        self.tab = if self.store.runs_for(&id).is_empty() {
            Tab::Tasks
        } else {
            Tab::Dag
        };
        self.session = Some(id);
        self.log = None;
        self.selected = 0;
        self.list_scroll = 0;
        self.rebuild_rows();
        self.auto_select();
        self.dirty = true;
    }

    pub fn rebuild_rows(&mut self) {
        let Some(session) = self.session.clone() else {
            self.rows = vec![Row::Info(
                "no session selected - click the session name to choose".into(),
            )];
            self.selected = 0;
            return;
        };
        let mut rows: Vec<Row> = match self.tab {
            Tab::Tasks => self
                .store
                .session_tree(&session)
                .into_iter()
                .map(|(depth, t)| Row::Task {
                    id: t.id.clone(),
                    depth,
                })
                .collect(),
            Tab::Dag => {
                let mut rows = Vec::new();
                for r in self.store.runs_for(&session) {
                    rows.push(Row::Run { id: r.id.clone() });
                    for n in &r.nodes {
                        rows.push(Row::Node {
                            run: r.id.clone(),
                            node: n.id.clone(),
                        });
                    }
                }
                rows
            }
        };
        if rows.is_empty() {
            let msg = match self.tab {
                Tab::Tasks => "no subagent tasks in this session yet",
                Tab::Dag => "no DAG runs in this session",
            };
            rows.push(Row::Info(msg.into()));
        }
        let prev = self.rows.get(self.selected).cloned();
        let log_task = self.log.as_ref().map(|l| l.task.clone());
        self.selected = prev
            .and_then(|p| rows.iter().position(|r| *r == p))
            .or_else(|| log_task.and_then(|t| rows.iter().position(|r| self.row_task(r) == Some(t.clone()))))
            .unwrap_or(self.selected)
            .min(rows.len() - 1);
        self.rows = rows;
    }

    pub fn row_task(&self, row: &Row) -> Option<String> {
        match row {
            Row::Task { id, .. } => Some(id.clone()),
            Row::Node { run, node } => self
                .store
                .run(run)?
                .nodes
                .iter()
                .find(|n| &n.id == node)?
                .task_id
                .clone(),
            _ => None,
        }
    }

    pub fn select(&mut self, i: usize) {
        self.selected = i.min(self.rows.len().saturating_sub(1));
        self.reveal = true;
        self.dirty = true;
        if let Some(task) = self.rows.get(self.selected).and_then(|r| self.row_task(r)) {
            self.open_log(task);
        }
    }

    fn open_log(&mut self, task: String) {
        if self.log.as_ref().is_some_and(|l| l.task == task) {
            return;
        }
        self.log = Some(LogView::new(task));
        self.poll_log();
    }

    fn auto_select(&mut self) {
        if self.log.is_some() {
            return;
        }
        let tasks: Vec<(usize, String)> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| Some((i, self.row_task(r)?)))
            .collect();
        let pick = tasks
            .iter()
            .rev()
            .find(|(_, id)| self.store.task(id).is_some_and(|t| t.is_active()))
            .or(tasks.last())
            .map(|(i, _)| *i);
        if let Some(i) = pick {
            self.select(i);
        }
    }

    fn poll_log(&mut self) -> bool {
        let Some(log) = self.log.as_mut() else { return false };
        let Some(path) = self.store.transcript_path(&log.task) else {
            let msg = format!("no transcript on disk for {} (not started, or pruned)", log.task);
            let changed = log.error.as_deref() != Some(msg.as_str());
            log.error = Some(msg);
            return changed;
        };
        if log.tail.as_ref().is_none_or(|t| t.path() != path.as_path()) {
            log.tail = Some(Tail::new(path));
            log.transcript = Transcript::default();
            log.expanded.clear();
            log.bad = 0;
            log.live = None;
            log.dirty = true;
        }
        let Some(tail) = log.tail.as_mut() else { return false };
        match tail.poll() {
            Ok(batch) => {
                let mut changed = log.error.take().is_some() || batch.bad > 0;
                if batch.reset {
                    log.transcript = Transcript::default();
                    log.expanded.clear();
                    log.live = None;
                    changed = true;
                }
                log.bad += batch.bad;
                let before = log.transcript.entries.len();
                for v in &batch.values {
                    log.transcript.push(v);
                }
                if !log.follow {
                    log.unseen += log.transcript.entries.len() - before;
                }
                changed |= !batch.values.is_empty();
                changed |= live::sync(&mut log.live, &log.transcript, &self.root);
                log.dirty |= changed;
                changed
            }
            Err(e) => {
                let msg = format!("cannot read transcript: {e}");
                if log.error.as_deref() == Some(msg.as_str()) {
                    return false;
                }
                log.error = Some(msg);
                log.dirty = true;
                true
            }
        }
    }

    pub fn refresh(&mut self) -> bool {
        let mut changed = self.store.refresh();
        if changed {
            self.rebuild_rows();
            self.auto_select();
            if let Some(log) = self.log.as_mut()
                && log.transcript.entries.iter().any(|e| !e.links.is_empty())
            {
                log.dirty = true;
            }
        }
        changed |= self.poll_log();
        let live = self
            .log
            .as_ref()
            .and_then(|l| self.store.task(&l.task))
            .is_some_and(|t| t.is_active());
        changed || live
    }

    pub fn on_key(&mut self, k: KeyEvent) {
        if k.kind != KeyEventKind::Press {
            return;
        }
        self.dirty = true;
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('c' | 'd')) {
            self.quit = true;
            return;
        }
        if self.settings.is_some() {
            crate::settings::on_key(self, k);
            return;
        }
        if self.picker.is_some() {
            self.picker_key(k.code);
            return;
        }
        let on_log = self.focus == Focus::Log || self.zoom;
        let page = self.log.as_ref().map_or(10, |l| l.height.max(2) - 1) as isize;
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('s') => self.open_picker(),
            KeyCode::Char('c') => self.open_settings(),
            KeyCode::Char('t') => self.switch_tab(if self.tab == Tab::Tasks { Tab::Dag } else { Tab::Tasks }),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = if self.focus == Focus::List {
                    Focus::Log
                } else {
                    Focus::List
                }
            }
            KeyCode::Enter => self.focus = Focus::Log,
            KeyCode::Char('z') => self.zoom = !self.zoom,
            KeyCode::Esc => {
                self.zoom = false;
                self.focus = Focus::List;
            }
            KeyCode::Char('f' | 'G') | KeyCode::End => self.follow_bottom(),
            KeyCode::Char('g') | KeyCode::Home => self.scroll_log(isize::MIN / 2),
            KeyCode::Char('x') => self.toggle_all(),
            KeyCode::Char('p') => self.pin_open = !self.pin_open,
            KeyCode::Char('r') => {
                self.reasoning = self.reasoning.next();
                if let Some(log) = self.log.as_mut() {
                    log.dirty = true;
                }
            }
            KeyCode::Char('+') => self.split = Some(self.list_area.height.saturating_add(1)),
            KeyCode::Char('-') => self.split = Some(self.list_area.height.saturating_sub(1).max(1)),
            KeyCode::Char('=') => self.split = None,
            KeyCode::Up | KeyCode::Char('k') if on_log => self.scroll_log(-1),
            KeyCode::Down | KeyCode::Char('j') if on_log => self.scroll_log(1),
            KeyCode::PageUp if on_log => self.scroll_log(-page),
            KeyCode::PageDown if on_log => self.scroll_log(page),
            KeyCode::Up | KeyCode::Char('k') => self.move_sel(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_sel(1),
            KeyCode::PageUp => self.move_sel(-10),
            KeyCode::PageDown => self.move_sel(10),
            _ => {}
        }
    }

    /// Selects the row of a linked subagent task, switching to the Tasks tab when needed.
    fn jump_to(&mut self, id: String) {
        let find = |app: &Self| {
            app.rows
                .iter()
                .position(|r| app.row_task(r).as_deref() == Some(id.as_str()))
        };
        let mut row = find(self);
        if row.is_none() && self.tab != Tab::Tasks {
            self.switch_tab(Tab::Tasks);
            row = find(self);
        }
        match row {
            Some(i) => self.select(i),
            None => self.open_log(id),
        }
    }

    fn move_sel(&mut self, delta: isize) {
        let max = self.rows.len().saturating_sub(1) as isize;
        self.select((self.selected as isize + delta).clamp(0, max) as usize);
    }

    fn switch_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.list_scroll = 0;
            self.rebuild_rows();
            self.reveal = true;
        }
    }

    pub fn scroll_log(&mut self, delta: isize) {
        let Some(log) = self.log.as_mut() else { return };
        let max = log.lines.len().saturating_sub(log.height);
        log.offset = (log.offset as isize).saturating_add(delta).clamp(0, max as isize) as usize;
        log.follow = log.offset >= max;
        if log.follow {
            log.unseen = 0;
        }
    }

    fn follow_bottom(&mut self) {
        if let Some(log) = self.log.as_mut() {
            log.follow = true;
            log.unseen = 0;
        }
    }

    fn toggle_all(&mut self) {
        let Some(log) = self.log.as_mut() else { return };
        if log.expanded.is_empty() {
            let all = log
                .transcript
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.collapsible());
            log.expanded = all.map(|(i, _)| i).collect();
        } else {
            log.expanded.clear();
        }
        log.dirty = true;
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        let pos = Position::new(m.column, m.row);
        let delta = match m.kind {
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                self.split = Some(m.row.saturating_sub(self.body_area.y));
                self.dirty = true;
                return;
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging => {
                self.dragging = false;
                self.dirty = true;
                return;
            }
            MouseEventKind::ScrollUp => -3,
            MouseEventKind::ScrollDown => 3,
            MouseEventKind::Down(MouseButton::Left) => {
                self.dirty = true;
                let hit = self
                    .hits
                    .iter()
                    .rev()
                    .find(|(r, _)| r.contains(pos))
                    .map(|(_, h)| h.clone());
                match hit {
                    Some(h) => self.activate(h),
                    None if self.picker.is_some() || self.settings.is_some() => {}
                    None if self.log_area.contains(pos) => self.focus = Focus::Log,
                    None if self.list_area.contains(pos) => self.focus = Focus::List,
                    None => {}
                }
                return;
            }
            _ => return,
        };
        if self.settings.is_some() {
            return;
        }
        self.dirty = true;
        if let Some(p) = self.picker.as_mut() {
            p.scroll = (p.scroll as isize + delta).max(0) as usize;
            p.reveal = false;
        } else if self.log_area.contains(pos) {
            self.scroll_log(delta);
        } else if self.list_area.contains(pos) {
            self.list_scroll = (self.list_scroll as isize + delta).max(0) as usize;
            self.reveal = false;
        }
    }

    fn activate(&mut self, hit: Hit) {
        match hit {
            Hit::Splitter => self.dragging = true,
            Hit::Tab(t) => self.switch_tab(t),
            Hit::Picker if self.picker.is_some() => self.picker = None,
            Hit::Picker => self.open_picker(),
            Hit::ClosePicker => self.picker = None,
            Hit::Session(i) => self.choose(i),
            Hit::Row(i) => {
                self.focus = Focus::List;
                self.select(i);
                self.reveal = false;
            }
            Hit::Entry(i) => {
                if let Some(log) = self.log.as_mut() {
                    if !log.expanded.remove(&i) {
                        log.expanded.insert(i);
                    }
                    log.dirty = true;
                    log.follow = false;
                }
                self.focus = Focus::Log;
            }
            Hit::Follow => self.follow_bottom(),
            Hit::Zoom => self.zoom = !self.zoom,
            Hit::Settings if self.settings.is_some() => self.settings = None,
            Hit::Settings => self.open_settings(),
            Hit::CloseSettings => self.settings = None,
            Hit::StatSelect(i) => self.settings_op(Op::Select(i)),
            Hit::StatBar(i) => self.settings_op(Op::Bar(i)),
            Hit::StatRow(i) => self.settings_op(Op::Row(i)),
            Hit::StatUp(i) => self.settings_op(Op::Up(i)),
            Hit::StatDown(i) => self.settings_op(Op::Down(i)),
            Hit::CostMode => self.settings_op(Op::Cost),
            Hit::CtxDown => self.settings_op(Op::CtxDown),
            Hit::CtxUp => self.settings_op(Op::CtxUp),
            Hit::ResetSettings => self.settings_op(Op::Reset),
            Hit::SaveSettings => self.save_settings(),
            Hit::Pin => self.pin_open = !self.pin_open,
            Hit::Task(id) => self.jump_to(id),
        }
    }

    pub fn current_stats(&self, task: &Task) -> Stats {
        match self.log.as_ref().filter(|l| l.task == task.id) {
            Some(log) => task.stats.merge(&log.transcript.usage.stats()),
            None => task.stats.clone(),
        }
    }

    /// Model of the task whose log is open; the settings context-limit row edits this one.
    pub fn log_model(&self) -> Option<String> {
        let task = self.store.task(&self.log.as_ref()?.task)?;
        Some(task.model.clone()).filter(|m| !m.is_empty())
    }

    pub fn open_settings(&mut self) {
        self.picker = None;
        self.settings = Some(Settings::default());
        self.dirty = true;
    }

    pub fn settings_op(&mut self, op: Op) {
        let model = self.log_model();
        let Some(s) = self.settings.as_mut() else { return };
        if crate::settings::apply(s, &mut self.config, model.as_deref(), op) {
            self.stats_changed();
        }
    }

    pub fn stats_changed(&mut self) {
        self.dirty = true;
        if let Some(log) = self.log.as_mut() {
            log.dirty = true;
        }
    }

    pub fn save_settings(&mut self) {
        let status = match self.config.save() {
            Ok(path) => format!("saved to {}", path.display()),
            Err(e) => format!("save failed: {e:#}"),
        };
        if let Some(s) = self.settings.as_mut() {
            s.status = Some(status);
        }
        self.dirty = true;
    }

    pub fn open_picker(&mut self) {
        self.settings = None;
        let sessions = self.index.list(&self.store);
        let selected = sessions
            .iter()
            .position(|s| Some(&s.id) == self.session.as_ref())
            .unwrap_or(0);
        self.picker = Some(Picker {
            sessions,
            selected,
            scroll: 0,
            reveal: true,
        });
        self.dirty = true;
    }

    fn picker_key(&mut self, code: KeyCode) {
        let Some(p) = self.picker.as_mut() else { return };
        let max = p.sessions.len().saturating_sub(1);
        match code {
            KeyCode::Up | KeyCode::Char('k') => p.selected = p.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => p.selected = (p.selected + 1).min(max),
            KeyCode::PageUp => p.selected = p.selected.saturating_sub(10),
            KeyCode::PageDown => p.selected = (p.selected + 10).min(max),
            KeyCode::Enter => {
                let i = p.selected;
                self.choose(i);
                return;
            }
            KeyCode::Esc | KeyCode::Char('q' | 's') => {
                self.picker = None;
                return;
            }
            _ => return,
        }
        p.reveal = true;
    }

    fn choose(&mut self, i: usize) {
        let id = self
            .picker
            .as_ref()
            .and_then(|p| p.sessions.get(i))
            .map(|s| s.id.clone());
        self.picker = None;
        if let Some(id) = id {
            self.set_session(id);
        }
    }
}
