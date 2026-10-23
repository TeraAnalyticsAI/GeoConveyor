//! The egui front end. All logic lives in the other modules; this file only draws.
//!
//! A *session* is one independent workspace (its own folders, options, jobs and log) shown as a
//! tab in the top bar, so several batches can run at once.

use crate::help::{find_terrio, run_text, Backend};
use crate::i18n::{self, t, tf};
use crate::jobs::{make_jobs, preview, scan_files, shell_join, Job, Status};
use crate::model::{normalize_order, FieldKind, Form, Globals, SessionSettings, Settings, TabState, Value};
use crate::runner::{self, apply_event, Event, RunHandle};
use crate::specs::{specs, InputType, Kind, DEM_MODES, EXTENSIONS};
use eframe::egui;
use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use crate::style::{self, Icon};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

const APP_NAME: &str = "GeoConveyor";
const LABEL_W: f32 = 190.0;

pub fn run() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1200.0, 1000.0])
        .with_min_inner_size([760.0, 560.0])
        .with_title(APP_NAME)
        // matches the .desktop file / Flatpak id so the shell groups the window with its launcher
        .with_app_id("xyz.teraanalytics.GeoConveyor");
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../packaging/icon-256.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(APP_NAME, options, Box::new(|_cc| Ok(Box::new(App::new()))))
}

// ---------------------------------------------------------------------------
// one session
// ---------------------------------------------------------------------------
struct Session {
    id: u32,
    g: Globals,
    tabs: Vec<TabState>,
    active: String,
    jobs: Vec<Job>,
    started: Vec<Option<Instant>>,
    run: Option<RunHandle>,
    log: Vec<String>,
    t0: Instant,
    summary: String,
}

impl Session {
    fn new(id: u32, be: &mut Backend, saved: Option<&SessionSettings>) -> Session {
        let mut tabs: Vec<TabState> = specs().into_iter().map(TabState::new).collect();
        for t in &mut tabs {
            t.rebuild(be);
            if let Some(s) = saved.and_then(|s| s.tabs.get(t.spec.key)) {
                t.apply(s, be);
            }
        }
        let g = saved.map(|s| s.globals.clone()).unwrap_or_default();
        let active = saved.map(|s| s.active.clone()).filter(|a| tabs.iter().any(|t| t.spec.key == a)).unwrap_or_else(|| tabs[0].spec.key.to_string());
        Session { id, g, tabs, active, jobs: vec![], started: vec![], run: None, log: vec![], t0: Instant::now(), summary: String::new() }
    }

    fn state(&self) -> SessionSettings {
        SessionSettings {
            globals: self.g.clone(),
            active: self.active.clone(),
            tabs: self.tabs.iter().map(|t| (t.spec.key.to_string(), t.state())).collect(),
        }
    }

    fn running(&self) -> bool {
        self.run.is_some()
    }

    fn idx(&self) -> usize {
        self.tabs.iter().position(|t| t.spec.key == self.active).unwrap_or(0)
    }

    fn jobs_for(&self, ti: usize, exe: &str) -> Result<Vec<Job>, String> {
        let tab = &self.tabs[ti];
        tab.validate(&self.g, exe)?;
        let files = if tab.uses_files() {
            let skip = if tab.needs_out() { self.g.out_dir.trim() } else { "" };
            scan_files(self.g.in_dir.trim(), &tab.patterns, self.g.recursive, skip)
        } else {
            vec![]
        };
        make_jobs(tab, &self.g, &files, false)
    }

    /// Scan the folder and fill the job table. Ok(false) when there is nothing to run.
    fn do_scan(&mut self, exe: &str) -> Result<bool, String> {
        let jobs = self.jobs_for(self.idx(), exe)?;
        let total: u64 = jobs.iter().map(|j| j.size_in).sum();
        self.summary = if jobs.is_empty() {
            t("No matching files found.")
        } else {
            tf("{} job(s), {} of input", &[jobs.len().to_string(), human(total)])
        };
        self.started = vec![None; jobs.len()];
        let any = !jobs.is_empty();
        self.jobs = jobs;
        Ok(any)
    }

    fn do_start(&mut self, ctx: &egui::Context, exe: &str) -> Result<(), String> {
        if self.running() || !self.do_scan(exe)? {
            return Ok(());
        }
        self.log.clear();
        self.t0 = Instant::now();
        let ctx2 = ctx.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || ctx2.request_repaint());
        let skip = !self.tabs[self.idx()].overwrite;
        self.run = Some(runner::start(exe.to_string(), self.jobs.clone(), self.g.parallel, skip, wake));
        Ok(())
    }

    fn copy_all_text(&self, exe: &str) -> Result<(String, usize), String> {
        let jobs = self.jobs_for(self.idx(), exe)?;
        let lines: Vec<String> = jobs
            .iter()
            .map(|j| {
                let mut p = vec![exe.to_string()];
                p.extend(j.display_argv());
                shell_join(&p)
            })
            .collect();
        Ok((lines.join("\n"), lines.len()))
    }

    fn poll(&mut self) {
        let mut finished = false;
        if let Some(h) = &self.run {
            while let Ok(ev) = h.rx.try_recv() {
                match &ev {
                    Event::Log(l) => {
                        self.log.extend(l.lines().map(String::from));
                        if self.log.len() > 20000 {
                            let cut = self.log.len() - 20000;
                            self.log.drain(..cut);
                        }
                    }
                    Event::AllDone => finished = true,
                    _ => {}
                }
                apply_event(&mut self.jobs, &mut self.started, &ev);
            }
        }
        if finished {
            self.run = None;
            let failed = self.jobs.iter().filter(|j| j.status == Status::Failed).count();
            self.update_summary();
            let tin: u64 = self.jobs.iter().filter(|j| j.status == Status::Done).map(|j| j.size_in).sum();
            let tout: u64 = self.jobs.iter().filter(|j| j.status == Status::Done && j.dst.is_some()).map(|j| j.size_out).sum();
            if tout > 0 {
                self.summary = format!("{} · {}", self.summary, tf("{} in, {} out", &[human(tin), human(tout)]));
            }
            if failed > 0 {
                self.log.push(format!("\n{}", tf("{} job(s) failed. The Output column has each reason.", &[failed.to_string()])));
            }
        } else if self.running() {
            self.update_summary();
        }
    }

    fn update_summary(&mut self) {
        let count = |s: Status| self.jobs.iter().filter(|j| j.status == s).count();
        let finished = self.jobs.iter().filter(|j| j.status.finished()).count();
        let done = count(Status::Done);
        let elapsed = self.t0.elapsed().as_secs_f64();
        let mut parts = vec![tf("{} done", &[done.to_string()])];
        for (s, key) in [(Status::Failed, "{} failed"), (Status::Skipped, "{} skipped"), (Status::Cancelled, "{} cancelled")] {
            if count(s) > 0 {
                parts.push(tf(key, &[count(s).to_string()]));
            }
        }
        parts.push(tf("{} running", &[count(Status::Running).to_string()]));
        let mut text = format!("{} · {}", parts.join(", "), tf("elapsed {}", &[fmt_time(elapsed)]));
        let remaining = self.jobs.len() - finished;
        if done > 0 && remaining > 0 && self.running() {
            text.push_str(&format!(" · {}", tf("about {} left", &[fmt_time(elapsed / done as f64 * remaining as f64)])));
        }
        self.summary = text;
    }
}

fn human(n: u64) -> String {
    let mut f = n as f64;
    for unit in ["B", "KB", "MB", "GB", "TB"] {
        if f < 1024.0 || unit == "TB" {
            return if unit == "B" { format!("{f:.0} {unit}") } else { format!("{f:.1} {unit}") };
        }
        f /= 1024.0;
    }
    String::new()
}

fn fmt_time(s: f64) -> String {
    let s = s as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn status_color(s: Status) -> Color32 {
    match s {
        Status::Done => Color32::from_rgb(76, 200, 110),
        Status::Failed => Color32::from_rgb(239, 83, 80),
        Status::Running => Color32::from_rgb(66, 165, 245),
        Status::Skipped => Color32::from_rgb(190, 196, 206),
        Status::Cancelled => Color32::from_rgb(255, 167, 38),
        Status::Pending => Color32::from_rgb(246, 196, 80),
    }
}

// ---------------------------------------------------------------------------
// the application
// ---------------------------------------------------------------------------
#[derive(Clone, Debug)]
enum Pick {
    InDir,
    OutDir,
    Exe,
    Color(String),
    Field { tab: String, long: String },
    SaveLog,
}

struct PickReq {
    pick: Pick,
    start: String,
}

/// What a session's UI asks the app to do (the session cannot touch app-level state itself).
#[derive(Default)]
struct Requests {
    pick: Option<PickReq>,
    notice: Option<String>,
    open_settings: bool,
    copy: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Theme {
    System,
    Light,
    Dark,
}

impl Theme {
    fn parse(s: &str) -> Theme {
        match s {
            "light" => Theme::Light,
            "dark" => Theme::Dark,
            _ => Theme::System,
        }
    }
    fn key(&self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
    fn pref(&self) -> egui::ThemePreference {
        match self {
            Theme::System => egui::ThemePreference::System,
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
        }
    }
}

struct App {
    be: Backend,
    terrio: String,
    version: String,
    theme: Theme,
    lang: String,
    order: Vec<String>,
    sessions: Vec<Session>,
    cur: usize,
    session_counter: u32,
    notice: Option<String>,
    picks: Vec<(u32, Pick, Receiver<Option<PathBuf>>)>,
    show_settings: bool,
    show_help: bool,
    show_about: bool,
    confirm_quit: bool,
    allow_close: bool,
    close_session: Option<u32>,
    theme_applied: bool,
}

impl App {
    fn new() -> App {
        let st = Settings::load();
        let lang = if st.language.is_empty() { i18n::detect() } else { st.language.clone() };
        i18n::set_language(&lang);
        let terrio = find_terrio(&st.terrio);
        let mut be = Backend::new(&terrio);
        let keys: Vec<&str> = specs().iter().map(|s| s.key).collect();
        let order = normalize_order(&st.order, &keys);
        let mut sessions = vec![];
        let mut counter = 0;
        for s in &st.sessions {
            counter += 1;
            sessions.push(Session::new(counter, &mut be, Some(s)));
        }
        if sessions.is_empty() {
            counter = 1;
            sessions.push(Session::new(1, &mut be, None));
        }
        let version = read_version(&terrio);
        App {
            cur: st.active_session.min(sessions.len() - 1),
            be, terrio, version, theme: Theme::parse(&st.theme), lang, order, sessions, session_counter: counter,
            notice: None, picks: vec![], show_settings: false, show_help: false, show_about: false,
            confirm_quit: false, allow_close: false, close_session: None, theme_applied: false,
        }
    }

    fn save_settings(&self) {
        Settings {
            terrio: self.terrio.clone(),
            theme: self.theme.key().into(),
            language: self.lang.clone(),
            order: self.order.clone(),
            sessions: self.sessions.iter().map(Session::state).collect(),
            active_session: self.cur,
            ..Default::default()
        }
        .save();
    }

    fn reload(&mut self) {
        self.terrio = self.terrio.trim().to_string();
        self.be.reload(&self.terrio);
        for s in &mut self.sessions {
            for t in &mut s.tabs {
                t.rebuild(&mut self.be);
            }
        }
        self.version = read_version(&self.terrio);
    }

    fn new_session(&mut self) {
        self.session_counter += 1;
        let mut s = Session::new(self.session_counter, &mut self.be, None);
        // start from the current session's folders: the usual next step is another job on them
        let cur = &self.sessions[self.cur];
        s.g.in_dir = cur.g.in_dir.clone();
        s.g.out_dir = cur.g.out_dir.clone();
        self.sessions.push(s);
        self.cur = self.sessions.len() - 1;
    }

    fn any_running(&self) -> bool {
        self.sessions.iter().any(Session::running)
    }

    fn close_session_id(&mut self, id: u32) {
        if let Some(i) = self.sessions.iter().position(|s| s.id == id) {
            if let Some(r) = &self.sessions[i].run {
                r.cancel();
            }
            self.sessions.remove(i);
            if self.sessions.is_empty() {
                self.session_counter += 1;
                self.sessions.push(Session::new(self.session_counter, &mut self.be, None));
            }
            self.cur = self.cur.min(self.sessions.len() - 1);
            if i < self.cur {
                self.cur -= 1;
            }
        }
    }

    fn poll_picks(&mut self) {
        let mut done = vec![];
        for (i, (_, _, rx)) in self.picks.iter().enumerate() {
            if let Ok(r) = rx.try_recv() {
                done.push((i, r));
            }
        }
        for (i, r) in done.into_iter().rev() {
            let (sid, what, _) = self.picks.remove(i);
            if let Some(p) = r {
                self.apply_pick(sid, what, p);
            }
        }
    }

    // File dialogs run on their own thread so the window never freezes.
    fn spawn_pick(&mut self, ctx: &egui::Context, sid: u32, req: PickReq) {
        let (tx, rx) = channel();
        let ctx2 = ctx.clone();
        let home = dirs::home_dir().unwrap_or_default();
        let p = PathBuf::from(req.start.trim());
        let start_dir = if p.is_dir() { p } else { p.parent().map(PathBuf::from).filter(|d| d.is_dir()).unwrap_or(home) };
        let (dir_mode, title) = match &req.pick {
            Pick::InDir => (true, t("Input folder")),
            Pick::OutDir => (true, t("Output folder")),
            Pick::Exe => (false, t("Locate terrio")),
            Pick::Color(_) => (false, t("Color file")),
            Pick::Field { .. } => (false, t("Choose a file")),
            Pick::SaveLog => (false, t("Save log")),
        };
        let save = matches!(req.pick, Pick::SaveLog);
        std::thread::spawn(move || {
            let d = rfd::FileDialog::new().set_title(title).set_directory(start_dir);
            let r = if save { d.set_file_name("terrio-log.txt").save_file() } else if dir_mode { d.pick_folder() } else { d.pick_file() };
            let _ = tx.send(r);
            ctx2.request_repaint();
        });
        self.picks.push((sid, req.pick, rx));
    }

    fn apply_pick(&mut self, sid: u32, what: Pick, p: PathBuf) {
        let s = p.to_string_lossy().into_owned();
        if let Pick::Exe = what {
            self.terrio = s;
            self.reload();
            return;
        }
        let Some(sess) = self.sessions.iter_mut().find(|x| x.id == sid) else { return };
        match what {
            Pick::InDir => sess.g.in_dir = s,
            Pick::OutDir => sess.g.out_dir = s,
            Pick::Color(key) => {
                if let Some(t) = sess.tabs.iter_mut().find(|t| t.spec.key == key) {
                    t.color = s;
                }
            }
            Pick::Field { tab, long } => {
                if let Some(f) = sess.tabs.iter_mut().find(|t| t.spec.key == tab).and_then(|t| t.form_mut()) {
                    f.set(&long, Value::Text(s));
                }
            }
            Pick::SaveLog => {
                let _ = std::fs::write(p, sess.log.join("\n"));
            }
            Pick::Exe => {}
        }
    }
}

fn read_version(exe: &str) -> String {
    if exe.is_empty() {
        return String::new();
    }
    let (ok, v) = run_text(exe, &["--version"]);
    if ok { v.trim().to_string() } else { String::new() }
}

// ---------------------------------------------------------------------------
// small widgets
// ---------------------------------------------------------------------------
/// egui's built-in fonts have no arrow glyphs, so arrows are drawn as triangles.
fn paint_arrow(ui: &egui::Ui, rect: egui::Rect, up: bool, enabled: bool) {
    let c = rect.center();
    let (w, h) = (4.5, 3.0);
    let pts = if up {
        vec![egui::pos2(c.x - w, c.y + h * 0.6), egui::pos2(c.x + w, c.y + h * 0.6), egui::pos2(c.x, c.y - h)]
    } else {
        vec![egui::pos2(c.x - w, c.y - h * 0.6), egui::pos2(c.x + w, c.y - h * 0.6), egui::pos2(c.x, c.y + h)]
    };
    let mut color = ui.visuals().text_color();
    if !enabled {
        color = color.gamma_multiply(0.35);
    }
    ui.painter().add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

/// A small square button with an up or down triangle.
fn arrow_button(ui: &mut egui::Ui, up: bool, enabled: bool) -> egui::Response {
    let r = ui.add_enabled(enabled, egui::Button::new("").min_size(egui::vec2(24.0, 18.0)));
    paint_arrow(ui, r.rect, up, enabled);
    r
}

/// A menu button showing a down triangle.
fn caret_menu<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::Response {
    let r = ui.menu_button("      ", add);
    paint_arrow(ui, r.response.rect, false, true);
    r.response
}

fn edit(ui: &mut egui::Ui, s: &mut String, hint: &str, width: f32) -> egui::Response {
    ui.add(egui::TextEdit::singleline(s).hint_text(hint).desired_width(width))
}

/// An editable text box with a drop-down of suggestions.
fn combo_edit(ui: &mut egui::Ui, s: &mut String, choices: &[String], hint: &str, width: f32, tip: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let r = edit(ui, s, hint, (width - 26.0).max(60.0));
        if !tip.is_empty() {
            r.on_hover_text(tip);
        }
        caret_menu(ui, |ui| {
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for c in choices {
                    let shown = if c.is_empty() { t("(none)") } else { c.clone() };
                    if ui.button(shown).clicked() {
                        *s = c.clone();
                        ui.close();
                    }
                }
            });
        });
    });
}

fn path_row(ui: &mut egui::Ui, label: &str, s: &mut String, hint: &str, browse: &mut bool) {
    ui.horizontal(|ui| {
        ui.add_sized([96.0, 20.0], egui::Label::new(label));
        let w = ui.available_width() - 130.0;
        edit(ui, s, hint, w.max(120.0));
        if style::pill_button(ui, &t("Browse…"), Icon::Folder).clicked() {
            *browse = true;
        }
    });
}

fn show_form(ui: &mut egui::Ui, form: &mut Form, filter: &str, key: &str, tab: &str, req: &mut Requests) {
    let active = !filter.trim().is_empty();
    let pending = form.expand_pending;
    let width = (ui.available_width() - LABEL_W - 40.0).clamp(160.0, 900.0);
    let sections = form.sections.clone();
    let mut first = true;
    for sec in sections.iter() {
        let shown: Vec<usize> = form
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| &f.opt.section == sec && form.is_visible(f) && Form::matches(f, filter))
            .map(|(i, _)| i)
            .collect();
        if shown.is_empty() {
            continue;
        }
        let open_by_default = first;
        first = false;
        let force = active || (pending && shown.iter().any(|&i| form.fields[i].is_set()));
        egui::CollapsingHeader::new(RichText::new(t(sec)).strong())
            .id_salt((key, sec))
            .default_open(open_by_default)
            .open(if force { Some(true) } else { None })
            .show(ui, |ui| {
                egui::Grid::new((key, sec, "grid")).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    for &i in &shown {
                        let f = &mut form.fields[i];
                        let name = if f.opt.short.is_empty() { f.opt.long.clone() } else { format!("{}, {}", f.opt.short, f.opt.long) };
                        let mut label = RichText::new(format!("{}{}", name, if f.opt.required { " *" } else { "" }));
                        if f.opt.required {
                            label = label.strong();
                        }
                        ui.vertical(|ui| {
                            ui.set_min_width(LABEL_W);
                            ui.label(label).on_hover_text(&f.opt.desc);
                        });
                        ui.vertical(|ui| {
                            let hint = if f.opt.default.is_empty() { String::new() } else { tf("default: {}", &[f.opt.default.clone()]) };
                            let tip = f.opt.desc.clone();
                            match f.kind {
                                FieldKind::Flag => {
                                    if let Value::Flag(b) = &mut f.value {
                                        style::toggle(ui, b).on_hover_text(&tip);
                                    }
                                }
                                FieldKind::Repeat => {
                                    if let Value::Text(t_) = &mut f.value {
                                        ui.add(
                                            egui::TextEdit::multiline(t_)
                                                .desired_rows(2)
                                                .desired_width(width)
                                                .hint_text(tf("one per line: {}", &[f.opt.metas[0].clone()])),
                                        )
                                        .on_hover_text(&tip);
                                    }
                                }
                                FieldKind::Multi => {
                                    let n = f.opt.metas.len();
                                    let metas = f.opt.metas.clone();
                                    if let Value::Multi(vs) = &mut f.value {
                                        ui.horizontal(|ui| {
                                            for (k, v) in vs.iter_mut().enumerate() {
                                                edit(ui, v, &metas[k], (width / n as f32 - 6.0).max(50.0)).on_hover_text(&tip);
                                            }
                                        });
                                    }
                                }
                                FieldKind::Single => {
                                    let ph = if hint.is_empty() { f.opt.metas[0].to_lowercase() } else { hint.clone() };
                                    let choices = f.choices.clone();
                                    let picker = f.picker;
                                    let long = f.opt.long.clone();
                                    let start = String::new();
                                    if let Value::Text(t_) = &mut f.value {
                                        if !choices.is_empty() {
                                            let mut all = vec![String::new()];
                                            all.extend(choices);
                                            combo_edit(ui, t_, &all, &ph, width, &tip);
                                        } else if picker {
                                            ui.horizontal(|ui| {
                                                edit(ui, t_, &ph, width - 36.0).on_hover_text(&tip);
                                                if ui.button("…").clicked() {
                                                    req.pick = Some(PickReq { pick: Pick::Field { tab: tab.to_string(), long }, start });
                                                }
                                            });
                                        } else {
                                            edit(ui, t_, &ph, width).on_hover_text(&tip);
                                        }
                                    }
                                }
                            }
                            let d = &f.opt.desc;
                            let short = if d.chars().count() > 150 { format!("{}…", d.chars().take(147).collect::<String>().trim_end()) } else { d.clone() };
                            ui.add(egui::Label::new(RichText::new(short).small().weak()).wrap());
                            ui.add_space(3.0);
                        });
                        ui.end_row();
                    }
                });
            });
    }
    form.expand_pending = false;
}

fn show_tab(ui: &mut egui::Ui, tab: &mut TabState, be: &mut Backend, req: &mut Requests) {
    if tab.no_terrio {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(239, 83, 80), t("terrio was not found. Choose its location in Settings, then press Reload."));
            if ui.button(t("Settings")).clicked() {
                req.open_settings = true;
            }
        });
    } else {
        ui.add(egui::Label::new(RichText::new(&tab.summary).weak()).wrap());
    }
    ui.add_space(4.0);
    let key = tab.spec.key;
    let spec = tab.spec;

    if spec.typed {
        ui.horizontal(|ui| {
            ui.label(t("Input type"));
            let mut pick = tab.input_type;
            for (n, ty) in InputType::ALL.into_iter().enumerate() {
                if n > 0 {
                    ui.weak("|");
                }
                if style::choice(ui, &t(ty.label()), pick == ty).clicked() {
                    pick = ty;
                }
            }
            tab.set_input_type(pick);
        });
        ui.add_space(2.0);
    }

    if tab.uses_files() {
        let ext_choices: Vec<String> = EXTENSIONS.iter().map(|s| s.to_string()).collect();
        egui::Grid::new("run_settings").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(t("File types"));
            edit(ui, &mut tab.patterns, "", 300.0).on_hover_text(t("Semicolon-separated, case-insensitive: *.tif;*.tiff;*.jp2"));
            if tab.mode_choice_visible() {
                ui.label(t("Mode"));
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(if tab.together { t("All files together, one output") } else { t("Each file separately") })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut tab.together, false, t("Each file separately"));
                        ui.selectable_value(&mut tab.together, true, t("All files together, one output"));
                    });
            }
            ui.end_row();
            if tab.needs_out() {
                ui.label(t("Output extension"));
                combo_edit(ui, &mut tab.ext, &ext_choices, "", 300.0,
                    &t("Output file extension. Use /ept.json for an EPT dataset, or leave blank for directory outputs such as MVT."));
                if tab.effective_together() {
                    ui.label(t("Output name"));
                    edit(ui, &mut tab.name, "", 220.0);
                } else {
                    ui.label(t("Name suffix"));
                    edit(ui, &mut tab.suffix, "", 220.0);
                }
                ui.end_row();
                ui.label(t("If output exists"));
                egui::ComboBox::from_id_salt("exists")
                    .selected_text(if tab.overwrite { t("Overwrite") } else { t("Skip") })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut tab.overwrite, false, t("Skip"));
                        ui.selectable_value(&mut tab.overwrite, true, t("Overwrite"));
                    });
                ui.end_row();
            }
        });
    }

    // per-command extras
    if spec.subs {
        ui.horizontal(|ui| {
            ui.label(t("Point filter"));
            let before = tab.sub.clone();
            let subs = tab.subs.clone();
            egui::ComboBox::from_id_salt("sub").selected_text(&tab.sub).height(400.0).show_ui(ui, |ui| {
                for s in &subs {
                    ui.selectable_value(&mut tab.sub, s.clone(), s);
                }
            });
            if tab.sub != before {
                tab.ensure_form(be);
            }
        });
    }
    if spec.kind == Kind::Dem {
        ui.horizontal(|ui| {
            ui.label(t("Mode"));
            egui::ComboBox::from_id_salt("dem_mode").selected_text(&tab.dem_mode).show_ui(ui, |ui| {
                for m in DEM_MODES {
                    ui.selectable_value(&mut tab.dem_mode, m.to_string(), *m);
                }
            });
            if tab.dem_mode == "color-relief" {
                edit(ui, &mut tab.color, &t("Color file: value R G B lines"), 380.0);
                if ui.button("…").clicked() {
                    req.pick = Some(PickReq { pick: Pick::Color(key.to_string()), start: tab.color.clone() });
                }
            }
        });
    }
    if spec.kind == Kind::Crs {
        egui::Grid::new("crs_in").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("CRS");
            edit(ui, &mut tab.crs, &t("EPSG:4326, WKT, PROJ string or a file"), 520.0);
            ui.end_row();
            ui.label(t("Coordinates"));
            edit(ui, &mut tab.coords, &t("to transform (with --to): x y [x y …]"), 520.0);
            ui.end_row();
        });
    }
    if spec.kind == Kind::Calc {
        ui.add(egui::Label::new(
            RichText::new(t("Each file becomes input A. In 'together' mode the files become A, B, C… in name order. Extra fixed inputs can be added under --input (B=path)."))
                .weak(),
        ).wrap());
    }

    ui.add_space(4.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t("Reset options")).clicked() {
                tab.reset_form();
            }
            ui.add(egui::TextEdit::singleline(&mut tab.filter).hint_text(t("Search options…")).desired_width(ui.available_width()));
        });
    });
    ui.add_space(2.0);

    let filter = tab.filter.clone();
    let fkey = format!("{}/{}/{:?}", key, tab.form_key(), tab.input_type);
    match tab.form_mut() {
        Some(f) => show_form(ui, f, &filter, &fkey, key, req),
        None => {
            ui.label(t("No options available (terrio not found?)."));
        }
    }
}

// ---------------------------------------------------------------------------
// one session's whole view
// ---------------------------------------------------------------------------
fn session_ui(ui: &mut egui::Ui, s: &mut Session, be: &mut Backend, order: &[String], req: &mut Requests) {
    let ctx = ui.ctx().clone();
    let running = s.running();
    let exe = be.exe.clone();

    // ---- folders and run options
    egui::Panel::top("paths").show(ui, |ui| {
        ui.add_space(4.0);
        let mut b = false;
        path_row(ui, &t("Input folder"), &mut s.g.in_dir, &t("Folder with the source files"), &mut b);
        if b {
            req.pick = Some(PickReq { pick: Pick::InDir, start: s.g.in_dir.clone() });
        }
        let mut b = false;
        path_row(ui, &t("Output folder"), &mut s.g.out_dir, &t("Folder for the results"), &mut b);
        if b {
            req.pick = Some(PickReq { pick: Pick::OutDir, start: s.g.out_dir.clone() });
        }
        ui.horizontal(|ui| {
            style::check(ui, &mut s.g.recursive, &t("Include subfolders"));
            style::check(ui, &mut s.g.keep_tree, &t("Keep subfolder structure"));
            let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
            ui.label(t("Parallel files"));
            ui.add(egui::DragValue::new(&mut s.g.parallel).range(1..=cpus.max(1)))
                .on_hover_text(t("Files processed at the same time. terrio reads most non-GeoTIFF inputs fully into memory, so lower this for very large files."));
        });
        ui.add_space(4.0);
    });

    // ---- log (bottom-most)
    egui::Panel::bottom("log").resizable(true).default_size(80.0).min_size(60.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.strong(t("Log"));
            if ui.small_button(t("Copy")).clicked() {
                req.copy = Some(s.log.join("\n"));
            }
            if ui.small_button(t("Save…")).clicked() {
                req.pick = Some(PickReq { pick: Pick::SaveLog, start: String::new() });
            }
        });
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both().auto_shrink(false).stick_to_bottom(true).show_rows(ui, row_h, s.log.len(), |ui, range| {
            for i in range {
                ui.add(egui::Label::new(RichText::new(&s.log[i]).monospace()).extend());
            }
        });
    });

    // ---- run controls and job table
    egui::Panel::bottom("run").resizable(true).default_size(250.0).min_size(150.0).show(ui, |ui| {
        ui.add_space(4.0);
        let ti = s.idx();
        let prev = preview(&s.tabs[ti], &s.g, &exe);
        ui.horizontal(|ui| {
            ui.strong(t("Command"));
            if style::pill_button(ui, &t("Copy command"), Icon::Copy).clicked() {
                req.copy = Some(prev.clone());
            }
            if style::pill_button(ui, &t("Copy all commands"), Icon::Copy).on_hover_text(t("Scan the folder and copy one command per file, as a shell script")).clicked() {
                match s.copy_all_text(&exe) {
                    Ok((text, n)) => {
                        req.copy = Some(text);
                        s.summary = tf("Copied {} command(s) to the clipboard.", &[n.to_string()]);
                    }
                    Err(e) => req.notice = Some(e),
                }
            }
        });
        style::command_box(ui, &prev);
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!running, |ui: &mut egui::Ui| style::pill_button(ui, &t("Scan folder"), Icon::None)).clicked() {
                if let Err(e) = s.do_scan(&exe) {
                    req.notice = Some(e);
                }
            }
            if ui.add_enabled(!running, |ui: &mut egui::Ui| style::go_button(ui, &t("Start"))).clicked() {
                match s.do_start(&ctx, &exe) {
                    Ok(()) => {}
                    Err(e) => req.notice = Some(e),
                }
            }
            if ui.add_enabled(running, |ui: &mut egui::Ui| style::pill_button(ui, &t("Cancel"), Icon::None)).clicked() {
                if let Some(r) = &s.run {
                    r.cancel();
                }
                s.summary = t("Cancelling…");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if style::pill_button(ui, &t("Open output folder"), Icon::Folder).clicked() && !s.g.out_dir.trim().is_empty() {
                    let _ = open::that(s.g.out_dir.trim());
                }
            });
        });
        let finished = s.jobs.iter().filter(|j| j.status.finished()).count();
        let total = s.jobs.len().max(1);
        style::progress(ui, finished as f32 / total as f32, &format!("{} / {}", finished, s.jobs.len()));
        ui.label(&s.summary);
        ui.add_space(2.0);
        let now = Instant::now();
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .auto_shrink(false)
            .min_scrolled_height(70.0)
            .column(Column::initial(320.0).at_least(160.0).clip(true))
            .column(Column::auto().at_least(70.0))
            .column(Column::auto().at_least(80.0))
            .column(Column::auto().at_least(60.0))
            .column(Column::remainder().at_least(120.0).clip(true))
            .header(20.0, |mut h| {
                for k in ["File", "Size", "Status", "Time", "Output size / note"] {
                    h.col(|ui| {
                        ui.strong(t(k));
                    });
                }
            })
            .body(|body| {
                body.rows(26.0, s.jobs.len(), |mut row| {
                    let idx = row.index();
                    let j = &s.jobs[idx];
                    row.col(|ui| { ui.label(&j.label); });
                    row.col(|ui| { ui.label(if j.size_in > 0 { human(j.size_in) } else { String::new() }); });
                    row.col(|ui| { style::status_pill(ui, &t(j.status.name()), status_color(j.status)); });
                    row.col(|ui| {
                        let secs = match (j.status, s.started.get(idx).copied().flatten()) {
                            (Status::Running, Some(st)) => now.duration_since(st).as_secs_f64(),
                            _ => j.seconds,
                        };
                        ui.label(if secs > 0.0 { fmt_time(secs) } else { String::new() });
                    });
                    row.col(|ui| {
                        let note = if j.status == Status::Done && j.dst.is_some() {
                            let ratio = if j.size_in > 0 { format!("  ({})", tf("{}% of input", &[format!("{:.0}", j.size_out as f64 / j.size_in as f64 * 100.0)])) } else { String::new() };
                            format!("{}{}", human(j.size_out), ratio)
                        } else {
                            j.message.clone()
                        };
                        ui.label(&note).on_hover_text(&note);
                    });
                });
            });
    });

    // ---- function tabs
    egui::CentralPanel::default().show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            ui.add_enabled_ui(!running, |ui| {
                for key in order {
                    if let Some(tab) = s.tabs.iter().find(|t| t.spec.key == key.as_str()) {
                        let title = t(tab.spec.title);
                        if style::tab_pill(ui, &title, s.active == *key).clicked() {
                            s.active = key.clone();
                        }
                    }
                }
            });
        });
        ui.separator();
        let ti = s.idx();
        ui.add_enabled_ui(!running, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                show_tab(ui, &mut s.tabs[ti], be, req);
            });
        });
        s.tabs[ti].sync(be);
    });
}

// ---------------------------------------------------------------------------
// the app frame: session strip, menu, dialogs
// ---------------------------------------------------------------------------
impl App {
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let mut close: Option<u32> = None;
        let mut select: Option<usize> = None;
        let mut new_tab = false;
        egui::Panel::top("sessions").show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                for (i, s) in self.sessions.iter().enumerate() {
                    let name = tf("Session {}", &[s.id.to_string()]);
                    let label = if s.running() { format!("{name}  ⟳") } else { name };
                    if style::tab_pill(ui, &label, self.cur == i).clicked() {
                        select = Some(i);
                    }
                    if self.sessions.len() > 1 && style::choice(ui, "×", false).on_hover_text(t("Close this tab")).clicked() {
                        close = Some(s.id);
                    }
                    ui.add_space(4.0);
                }
                if style::tab_pill(ui, "+", false).on_hover_text(t("New tab")).clicked() {
                    new_tab = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    caret_menu(ui, |ui| {
                        if ui.button(t("New tab")).clicked() {
                            new_tab = true;
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(t("Settings")).clicked() {
                            self.show_settings = true;
                            ui.close();
                        }
                        if ui.button(t("Help")).clicked() {
                            self.show_help = true;
                            ui.close();
                        }
                        if ui.button(t("About")).clicked() {
                            self.show_about = true;
                            ui.close();
                        }
                    })
                    .on_hover_text(t("Menu"));
                    ui.weak(format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION")));
                });
            });
            ui.add_space(3.0);
        });
        if let Some(i) = select {
            self.cur = i;
        }
        if new_tab {
            self.new_session();
        }
        if let Some(id) = close {
            if self.sessions.iter().any(|s| s.id == id && s.running()) {
                self.close_session = Some(id);
            } else {
                self.close_session_id(id);
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        // closing while a batch runs
        if self.confirm_quit {
            egui::Window::new(APP_NAME).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(t("A batch is running. Cancel it and quit?"));
                ui.horizontal(|ui| {
                    if ui.button(t("Cancel batch and quit")).clicked() {
                        for s in &self.sessions {
                            if let Some(r) = &s.run {
                                r.cancel();
                            }
                        }
                        self.save_settings();
                        self.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button(t("Keep running")).clicked() {
                        self.confirm_quit = false;
                    }
                });
            });
        }
        if let Some(id) = self.close_session {
            egui::Window::new(APP_NAME).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(t("This tab is running a batch. Cancel it and close the tab?"));
                ui.horizontal(|ui| {
                    if ui.button(t("Cancel batch and close")).clicked() {
                        self.close_session_id(id);
                        self.close_session = None;
                    }
                    if ui.button(t("Keep running")).clicked() {
                        self.close_session = None;
                    }
                });
            });
        }
        if let Some(msg) = self.notice.clone() {
            egui::Window::new(APP_NAME).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.set_max_width(520.0);
                ui.label(msg);
                if ui.button("OK").clicked() {
                    self.notice = None;
                }
            });
        }
        self.settings_window(ctx);
        self.help_window(ctx);
        self.about_window(ctx);
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let mut changed = false;
        let mut reload = false;
        let mut browse = false;
        let mut new_lang: Option<String> = None;
        let titles: std::collections::HashMap<&str, &str> = specs().iter().map(|s| (s.key, s.title)).collect();
        egui::Window::new(t("Settings")).open(&mut open).default_width(460.0).collapsible(false).show(ctx, |ui| {
            ui.strong(t("Appearance"));
            ui.horizontal(|ui| {
                for (th, name) in [(Theme::System, "System"), (Theme::Light, "Light"), (Theme::Dark, "Dark")] {
                    if ui.selectable_label(self.theme == th, t(name)).clicked() {
                        self.theme = th;
                        ctx.set_theme(th.pref());
                        changed = true;
                    }
                }
            });
            ui.add_space(8.0);

            ui.strong(t("Language"));
            let packs = i18n::all_packs();
            let current_name = packs.iter().find(|p| p.code == self.lang).map(|p| p.name.clone()).unwrap_or_else(|| "English".into());
            egui::ComboBox::from_id_salt("lang").selected_text(current_name).show_ui(ui, |ui| {
                for p in &packs {
                    if ui.selectable_label(self.lang == p.code, &p.name).clicked() {
                        new_lang = Some(p.code.clone());
                    }
                }
                for (code, name) in i18n::PLANNED {
                    if !packs.iter().any(|p| p.code == *code) {
                        ui.add_enabled(false, egui::Button::new(format!("{} ({})", name, t("coming soon"))).frame(false));
                    }
                }
            });
            ui.add(egui::Label::new(RichText::new(t("Only the program's own text is translated. Option names and descriptions come from terrio and stay as it writes them.")).small().weak()).wrap());
            ui.add_space(8.0);

            ui.strong(t("Order of functions"));
            let mut swap: Option<(usize, usize)> = None;
            let n = self.order.len();
            egui::Grid::new("order").num_columns(3).spacing([6.0, 2.0]).show(ui, |ui| {
                for (i, key) in self.order.iter().enumerate() {
                    if arrow_button(ui, true, i > 0).clicked() {
                        swap = Some((i, i - 1));
                    }
                    if arrow_button(ui, false, i + 1 < n).clicked() {
                        swap = Some((i, i + 1));
                    }
                    ui.label(t(titles.get(key.as_str()).copied().unwrap_or(key.as_str())));
                    ui.end_row();
                }
            });
            if let Some((a, b)) = swap {
                self.order.swap(a, b);
                changed = true;
            }
            if ui.button(t("Reset order")).clicked() {
                self.order = specs().iter().map(|s| s.key.to_string()).collect();
                changed = true;
            }
            ui.add_space(8.0);

            ui.strong("terrio");
            ui.horizontal(|ui| {
                let w = ui.available_width() - 200.0;
                edit(ui, &mut self.terrio, &t("Path to terrio (auto-detected)"), w.max(120.0));
                if ui.button(t("Browse…")).clicked() {
                    browse = true;
                }
                if ui.button(t("Reload")).on_hover_text(t("Re-read terrio's options (after installing or upgrading it)")).clicked() {
                    reload = true;
                }
            });
            if !self.version.is_empty() {
                ui.weak(&self.version);
            }
        });
        if let Some(code) = new_lang {
            self.lang = code.clone();
            i18n::set_language(&code);
            changed = true;
        }
        if reload {
            self.reload();
            changed = true;
        }
        if browse {
            let sid = self.sessions[self.cur].id;
            let start = self.terrio.clone();
            // dialogs need the context; reuse the one passed in
            self.spawn_pick(ctx, sid, PickReq { pick: Pick::Exe, start });
        }
        if changed {
            self.save_settings();
        }
        self.show_settings = open;
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        let mut open = true;
        egui::Window::new(t("Help")).open(&mut open).default_width(560.0).default_height(520.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (h, body) in i18n::HELP {
                    ui.strong(t(h));
                    ui.add(egui::Label::new(t(body)).wrap());
                    ui.add_space(8.0);
                }
            });
        });
        self.show_help = open;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = true;
        egui::Window::new(t("About")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.heading(APP_NAME);
            ui.label(tf("Version {}", &[env!("CARGO_PKG_VERSION").to_string()]));
            ui.add_space(6.0);
            ui.label(t("A Tera Analytics product."));
            ui.label(t("A native batch front end for terrio."));
            if !self.version.is_empty() {
                ui.label(tf("Using {}", &[self.version.clone()]));
            }
            ui.label(t("Built with Rust and egui."));
            ui.add_space(6.0);
            if ui.link("github.com/opengeos/terrio").clicked() {
                let _ = open::that("https://github.com/opengeos/terrio");
            }
        });
        self.show_about = open;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.theme_applied {
            style::install(&ctx);
            ctx.set_theme(self.theme.pref());
            self.theme_applied = true;
        }
        for s in &mut self.sessions {
            s.poll();
        }
        self.poll_picks();
        if self.any_running() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.any_running() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm_quit = true;
            } else {
                self.save_settings();
            }
        }

        self.top_bar(ui);

        let mut req = Requests::default();
        let cur = self.cur;
        {
            let App { be, sessions, order, .. } = self;
            let sid = sessions[cur].id;
            ui.push_id(sid, |ui| session_ui(ui, &mut sessions[cur], be, order, &mut req));
        }
        let sid = self.sessions[self.cur].id;
        if let Some(text) = req.copy.take() {
            ctx.copy_text(text);
        }
        if let Some(n) = req.notice.take() {
            self.notice = Some(n);
        }
        if req.open_settings {
            self.show_settings = true;
        }
        if let Some(p) = req.pick.take() {
            self.spawn_pick(&ctx, sid, p);
        }
        self.dialogs(&ctx);

        let title = format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for s in &self.sessions {
            if let Some(r) = &s.run {
                r.cancel();
            }
        }
        self.save_settings();
    }
}
