//! The state behind the UI, free of any widget code so it can be tested headlessly.

use crate::help::{Backend, Format, Help, Opt};
use crate::i18n::{t, tf};
use crate::specs::{self, InputType, Kind, Spec, Together, DEM_MODES};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Flag(bool),
    Text(String),
    Multi(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Flag,
    Repeat,
    Multi,
    Single,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub opt: Opt,
    pub kind: FieldKind,
    pub choices: Vec<String>,
    pub picker: bool,
    pub value: Value,
}

impl Field {
    pub fn new(opt: Opt, choices: Vec<String>) -> Field {
        let kind = if opt.is_flag() {
            FieldKind::Flag
        } else if opt.repeatable {
            FieldKind::Repeat
        } else if opt.metas.len() > 1 {
            FieldKind::Multi
        } else {
            FieldKind::Single
        };
        let picker = specs::OPEN_PICKERS.contains(&opt.long.as_str());
        let mut f = Field { opt, kind, choices, picker, value: Value::Flag(false) };
        f.clear();
        f
    }

    pub fn clear(&mut self) {
        self.value = match self.kind {
            FieldKind::Flag => Value::Flag(false),
            FieldKind::Multi => Value::Multi(vec![String::new(); self.opt.metas.len()]),
            _ => Value::Text(String::new()),
        };
    }

    pub fn tokens(&self) -> Result<Vec<String>, String> {
        let long = self.opt.long.clone();
        match (&self.kind, &self.value) {
            (FieldKind::Flag, Value::Flag(true)) => Ok(vec![long]),
            (FieldKind::Repeat, Value::Text(t)) => {
                let mut out = vec![];
                for line in t.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    out.push(long.clone());
                    out.push(line.to_string());
                }
                Ok(out)
            }
            (FieldKind::Multi, Value::Multi(vs)) => {
                let vals: Vec<String> = vs.iter().map(|v| v.trim().to_string()).collect();
                if vals.iter().all(|v| v.is_empty()) {
                    return Ok(vec![]);
                }
                if vals.iter().any(|v| v.is_empty()) {
                    return Err(format!("{} needs {} values ({}).", long, vals.len(), self.opt.metas.join(" ")));
                }
                let mut out = vec![long];
                out.extend(vals);
                Ok(out)
            }
            (FieldKind::Single, Value::Text(t)) if !t.trim().is_empty() => Ok(vec![long, t.trim().to_string()]),
            _ => Ok(vec![]),
        }
    }

    pub fn is_set(&self) -> bool {
        match self.tokens() {
            Ok(t) => !t.is_empty(),
            Err(_) => true,
        }
    }

    pub fn set(&mut self, v: &Value) {
        let ok = matches!(
            (&self.kind, v),
            (FieldKind::Flag, Value::Flag(_))
                | (FieldKind::Multi, Value::Multi(_))
                | (FieldKind::Single | FieldKind::Repeat, Value::Text(_))
        );
        if !ok {
            return;
        }
        if let (FieldKind::Multi, Value::Multi(vs)) = (&self.kind, v) {
            let mut vs = vs.clone();
            vs.resize(self.opt.metas.len(), String::new());
            self.value = Value::Multi(vs);
        } else {
            self.value = v.clone();
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Form {
    pub summary: String,
    pub fields: Vec<Field>,
    pub sections: Vec<String>,
    /// Set after loading saved values: sections with a value open on the next frame.
    pub expand_pending: bool,
    /// Which input type's options are active (only Convert distinguishes them).
    pub ty: InputType,
    pub cmd_key: String,
}

impl Form {
    pub fn build(hp: &Help, cmd_key: &str, formats: &[String], defaults: &[(&str, &str)], skip: &[&str]) -> Form {
        let mut fields = vec![];
        let mut sections: Vec<String> = vec![];
        for o in &hp.options {
            if skip.contains(&o.long.as_str()) {
                continue;
            }
            if !sections.contains(&o.section) {
                sections.push(o.section.clone());
            }
            let mut choices: Vec<String> = specs::choices_for(cmd_key, &o.long).iter().map(|s| s.to_string()).collect();
            if o.long == "--format" && !formats.is_empty() {
                choices = formats.to_vec();
            }
            fields.push(Field::new(o.clone(), choices));
        }
        let mut f = Form { summary: hp.summary.clone(), fields, sections, expand_pending: false, ty: InputType::Image, cmd_key: cmd_key.to_string() };
        f.reset(defaults);
        f
    }

    pub fn reset(&mut self, defaults: &[(&str, &str)]) {
        for f in &mut self.fields {
            f.clear();
            if let Some((_, v)) = defaults.iter().find(|(k, _)| *k == f.opt.long) {
                f.set(&Value::Text((*v).to_string()));
            }
        }
    }

    /// Options of the other input types are hidden, and so are left out of the command.
    pub fn is_visible(&self, f: &Field) -> bool {
        specs::relevant(&self.cmd_key, &f.opt.section, &f.opt.long, self.ty)
    }

    pub fn tokens(&self) -> Result<Vec<String>, String> {
        let mut out = vec![];
        for f in self.fields.iter().filter(|f| self.is_visible(f)) {
            out.extend(f.tokens()?);
        }
        Ok(out)
    }

    pub fn missing(&self) -> Vec<String> {
        self.fields.iter().filter(|f| self.is_visible(f) && f.opt.required && !f.is_set()).map(|f| f.opt.long.clone()).collect()
    }

    pub fn get(&self, long: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.opt.long == long)
    }

    pub fn get_mut(&mut self, long: &str) -> Option<&mut Field> {
        self.fields.iter_mut().find(|f| f.opt.long == long)
    }

    /// Set a field by flag name; used by saved state and by tests.
    pub fn set(&mut self, long: &str, v: Value) -> bool {
        match self.get_mut(long) {
            Some(f) => {
                f.set(&v);
                true
            }
            None => false,
        }
    }

    pub fn state(&self) -> BTreeMap<String, Value> {
        self.fields.iter().map(|f| (f.opt.long.clone(), f.value.clone())).collect()
    }

    pub fn set_state(&mut self, st: &BTreeMap<String, Value>) {
        for (k, v) in st {
            self.set(k, v.clone());
        }
        self.expand_pending = true;
    }

    pub fn matches(f: &Field, filter: &str) -> bool {
        let t = filter.trim().to_lowercase();
        t.is_empty() || f.opt.long.to_lowercase().contains(&t) || f.opt.desc.to_lowercase().contains(&t)
    }
}

// ---------------------------------------------------------------------------
// One tab per command
// ---------------------------------------------------------------------------
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TabSettings {
    pub together: bool,
    pub patterns: String,
    pub ext: String,
    pub suffix: String,
    pub name: String,
    pub overwrite: bool,
    pub sub: String,
    pub dem: String,
    pub color: String,
    pub crs: String,
    pub coords: String,
    pub input_type: InputType,
    pub forms: BTreeMap<String, BTreeMap<String, Value>>,
}

pub struct TabState {
    pub spec: Spec,
    pub together: bool,
    pub patterns: String,
    pub ext: String,
    pub suffix: String,
    pub name: String,
    pub overwrite: bool,
    pub sub: String,
    pub subs: Vec<String>,
    pub dem_mode: String,
    pub color: String,
    pub crs: String,
    pub coords: String,
    pub filter: String,
    pub input_type: InputType,
    pub forms: HashMap<String, Form>,
    pub summary: String,
    pub no_terrio: bool,
    auto_suffix: String,
    last_format: String,
    last_dem: String,
}

impl TabState {
    pub fn new(spec: Spec) -> TabState {
        TabState {
            spec,
            together: false,
            patterns: spec.patterns.into(),
            ext: spec.ext.into(),
            suffix: spec.suffix.into(),
            name: spec.out_name.into(),
            overwrite: false,
            sub: String::new(),
            subs: vec![],
            dem_mode: DEM_MODES[0].into(),
            color: String::new(),
            crs: String::new(),
            coords: String::new(),
            filter: String::new(),
            input_type: InputType::Image,
            forms: HashMap::new(),
            summary: String::new(),
            no_terrio: false,
            auto_suffix: spec.suffix.into(),
            last_format: spec.defaults.iter().find(|(k, _)| *k == "--format").map(|(_, v)| v.to_string()).unwrap_or_default(),
            last_dem: DEM_MODES[0].into(),
        }
    }

    pub fn uses_files(&self) -> bool {
        !matches!(self.spec.kind, Kind::Crs | Kind::Formats)
    }

    pub fn needs_out(&self) -> bool {
        !matches!(self.spec.kind, Kind::Info | Kind::Crs | Kind::Formats)
    }

    pub fn mode_choice_visible(&self) -> bool {
        self.spec.together == Together::Optional && !self.spec.subs
    }

    pub fn effective_together(&self) -> bool {
        match self.spec.together {
            Together::Only => true,
            _ if self.spec.subs => self.sub == "merge",
            Together::Optional => self.together,
            Together::No => false,
        }
    }

    pub fn argv(&self) -> Vec<String> {
        let mut v = vec![self.spec.key.to_string()];
        if self.spec.subs && !self.sub.is_empty() {
            v.push(self.sub.clone());
        }
        v
    }

    pub fn cmd_key(&self) -> String {
        self.argv().join(" ")
    }

    pub fn form_key(&self) -> String {
        if self.spec.subs { self.sub.clone() } else { String::new() }
    }

    pub fn form(&self) -> Option<&Form> {
        self.forms.get(&self.form_key())
    }

    pub fn form_mut(&mut self) -> Option<&mut Form> {
        let k = self.form_key();
        self.forms.get_mut(&k)
    }

    fn format_names(&self, formats: &[Format]) -> Vec<String> {
        formats
            .iter()
            .filter(|f| f.write && (self.spec.fmt_kinds.is_empty() || self.spec.fmt_kinds.contains(&f.kind.as_str())))
            .map(|f| f.name.clone())
            .collect()
    }

    /// (Re)read --help and make sure the current form exists.
    pub fn rebuild(&mut self, be: &mut Backend) {
        let saved = self.state();
        self.forms.clear();
        self.summary.clear();
        let exe = be.exe.clone();
        let top = be.help.get(&exe, &[self.spec.key.to_string()]).cloned();
        self.no_terrio = top.is_none();
        if let Some(h) = &top {
            self.summary = h.summary.clone();
        }
        if self.spec.subs {
            if let Some(h) = &top {
                self.subs = h.subcommands.iter().map(|(n, _)| n.clone()).collect();
                if !self.subs.contains(&self.sub) {
                    self.sub = self.subs.first().cloned().unwrap_or_default();
                }
            }
        }
        self.ensure_form(be);
        self.apply(&saved, be);
    }

    pub fn ensure_form(&mut self, be: &mut Backend) {
        let key = self.form_key();
        if self.forms.contains_key(&key) || (self.spec.subs && key.is_empty()) {
            return;
        }
        let exe = be.exe.clone();
        let argv = self.argv();
        let formats = self.format_names(&be.formats);
        if let Some(hp) = be.help.get(&exe, &argv) {
            let f = Form::build(hp, &self.cmd_key(), &formats, self.spec.defaults, specs::skipped(self.spec.key));
            if self.spec.subs {
                self.summary = f.summary.clone();
            }
            let mut f = f;
            f.ty = self.input_type;
            self.forms.insert(key, f);
        }
    }

    /// Call once per frame after the widgets ran: keeps derived settings in step.
    pub fn sync(&mut self, be: &Backend) {
        let fmt = match self.form().and_then(|f| f.get("--format")).map(|f| f.value.clone()) {
            Some(Value::Text(t)) => Some(t),
            _ => None,
        };
        if let Some(t) = fmt {
            if t != self.last_format {
                if let Some(fm) = be.formats.iter().find(|x| x.name.eq_ignore_ascii_case(t.trim())) {
                    if let Some(e) = fm.extensions.first() {
                        self.ext = format!(".{e}");
                    }
                }
                self.last_format = t;
            }
        }
        if self.spec.kind == Kind::Dem && self.dem_mode != self.last_dem {
            if self.suffix == self.auto_suffix {
                self.auto_suffix = format!("_{}", self.dem_mode.to_lowercase());
                self.suffix = self.auto_suffix.clone();
            }
            self.last_dem = self.dem_mode.clone();
        }
    }

    pub fn state(&self) -> TabSettings {
        TabSettings {
            together: self.together,
            patterns: self.patterns.clone(),
            ext: self.ext.clone(),
            suffix: self.suffix.clone(),
            name: self.name.clone(),
            overwrite: self.overwrite,
            sub: self.sub.clone(),
            dem: self.dem_mode.clone(),
            color: self.color.clone(),
            crs: self.crs.clone(),
            coords: self.coords.clone(),
            input_type: self.input_type,
            forms: self.forms.iter().map(|(k, f)| (k.clone(), f.state())).collect(),
        }
    }

    /// Apply saved values. Builds the forms they refer to when terrio is available.
    pub fn apply(&mut self, st: &TabSettings, be: &mut Backend) {
        if st.patterns.is_empty() && st.ext.is_empty() && st.forms.is_empty() && st.name.is_empty() {
            return; // an empty record (never saved)
        }
        self.together = st.together;
        self.patterns = st.patterns.clone();
        self.ext = st.ext.clone();
        self.suffix = st.suffix.clone();
        self.name = st.name.clone();
        self.overwrite = st.overwrite;
        if DEM_MODES.contains(&st.dem.as_str()) {
            self.dem_mode = st.dem.clone();
            self.last_dem = st.dem.clone();
            self.auto_suffix = format!("_{}", st.dem.to_lowercase());
        }
        self.color = st.color.clone();
        self.crs = st.crs.clone();
        self.coords = st.coords.clone();
        if self.spec.typed {
            self.input_type = st.input_type;
            for f in self.forms.values_mut() {
                f.ty = st.input_type;
            }
        }
        if self.spec.subs && self.subs.contains(&st.sub) {
            self.sub = st.sub.clone();
        }
        let keep = self.sub.clone();
        for (key, vals) in &st.forms {
            if self.spec.subs {
                if !self.subs.contains(key) {
                    continue;
                }
                self.sub = key.clone();
            }
            self.ensure_form(be);
            if let Some(f) = self.forms.get_mut(key) {
                f.set_state(vals);
            }
        }
        self.sub = keep;
        self.ensure_form(be);
        if let Some(Value::Text(t)) = self.form().and_then(|f| f.get("--format")).map(|f| f.value.clone()) {
            self.last_format = t; // loading must not overwrite the saved extension
        }
    }

    /// Switch Convert between image, vector and point-cloud inputs: loads that type's starting
    /// file patterns, extension and option values. Values typed for other types are kept.
    pub fn set_input_type(&mut self, ty: InputType) {
        if self.input_type == ty {
            return;
        }
        self.input_type = ty;
        for f in self.forms.values_mut() {
            f.ty = ty;
        }
        if let Some((patterns, ext, defaults)) = ty.starting_point() {
            self.patterns = patterns.into();
            self.ext = ext.into();
            if let Some(form) = self.form_mut() {
                for k in specs::TYPE_MANAGED {
                    if let Some(f) = form.get_mut(k) {
                        f.clear();
                    }
                }
                for (k, v) in defaults {
                    form.set(k, Value::Text((*v).to_string()));
                }
            }
            // keep the extension logic from overwriting what we just chose
            if let Some(Value::Text(t)) = self.form().and_then(|f| f.get("--format")).map(|f| f.value.clone()) {
                self.last_format = t;
            }
        }
    }

    pub fn reset_form(&mut self) {
        let d = self.spec.defaults;
        if let Some(f) = self.form_mut() {
            f.reset(d);
        }
    }

    pub fn validate(&self, g: &Globals, exe: &str) -> Result<(), String> {
        if exe.is_empty() {
            return Err(t("terrio executable not found. Install it (Linux/macOS: `curl -LsSf https://terrio.opengeos.org/install.sh | sh`, or `pip install terrio`) or choose it in Settings."));
        }
        let f = self.form().ok_or_else(|| t("This command's options could not be read from terrio."))?;
        f.tokens()?;
        let miss = f.missing();
        if !miss.is_empty() {
            return Err(tf("Required: {}", &[miss.join(", ")]));
        }
        if self.uses_files() && !std::path::Path::new(g.in_dir.trim()).is_dir() {
            return Err(t("Choose an input folder."));
        }
        if self.needs_out() && g.out_dir.trim().is_empty() {
            return Err(t("Choose an output folder."));
        }
        if self.spec.kind == Kind::Dem && self.dem_mode == "color-relief" && self.color.trim().is_empty() {
            return Err(t("Color relief needs a color file."));
        }
        if self.spec.kind == Kind::Crs && self.crs.trim().is_empty() {
            return Err(t("Enter a CRS."));
        }
        Ok(())
    }
}

/// The folders and run options of one session.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Globals {
    pub in_dir: String,
    pub out_dir: String,
    pub recursive: bool,
    pub keep_tree: bool,
    pub parallel: usize,
}

impl Default for Globals {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
        Globals { in_dir: String::new(), out_dir: String::new(), recursive: true, keep_tree: true, parallel: (cpus / 2).max(1) }
    }
}

/// Everything saved about one session ("tab" in the top bar).
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionSettings {
    pub globals: Globals,
    pub active: String,
    pub tabs: BTreeMap<String, TabSettings>,
}

/// Settings from versions that had a single session and no Settings dialog.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LegacyGlobals {
    pub in_dir: String,
    pub out_dir: String,
    pub terrio: String,
    pub recursive: Option<bool>,
    pub keep_tree: Option<bool>,
    pub parallel: Option<usize>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub terrio: String,
    /// "system", "light" or "dark"
    pub theme: String,
    /// language code; empty = detect from the environment on first run
    pub language: String,
    /// function tabs in display order (spec keys)
    pub order: Vec<String>,
    pub sessions: Vec<SessionSettings>,
    pub active_session: usize,
    // ---- older files
    pub globals: Option<LegacyGlobals>,
    pub tabs: BTreeMap<String, TabSettings>,
}

impl Settings {
    pub fn path() -> Option<std::path::PathBuf> {
        dirs::config_dir().map(|d| d.join("geoconveyor").join("settings.json"))
    }

    pub fn load() -> Settings {
        migrate_old_config();
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| Self::from_json(&s)).unwrap_or_default()
    }

    /// Parse a settings file, carrying an older single-session file forward.
    pub fn from_json(json: &str) -> Settings {
        let mut s: Settings = serde_json::from_str(json).unwrap_or_default();
        if s.sessions.is_empty() && (s.globals.is_some() || !s.tabs.is_empty()) {
            let lg = s.globals.take().unwrap_or_default();
            let d = Globals::default();
            if s.terrio.is_empty() {
                s.terrio = lg.terrio.clone();
            }
            s.sessions.push(SessionSettings {
                globals: Globals {
                    in_dir: lg.in_dir,
                    out_dir: lg.out_dir,
                    recursive: lg.recursive.unwrap_or(d.recursive),
                    keep_tree: lg.keep_tree.unwrap_or(d.keep_tree),
                    parallel: lg.parallel.unwrap_or(d.parallel),
                },
                active: String::new(),
                tabs: std::mem::take(&mut s.tabs),
            });
        }
        s
    }

    pub fn save(&self) {
        if let Some(p) = Self::path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if let Ok(s) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(p, s);
            }
        }
    }
}

/// `order` with unknown keys dropped and missing ones appended, so a saved order stays valid
/// when functions are added.
pub fn normalize_order(order: &[String], all: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = order.iter().filter(|k| all.contains(&k.as_str())).cloned().collect();
    out.dedup();
    let mut seen = std::collections::HashSet::new();
    out.retain(|k| seen.insert(k.clone()));
    for k in all {
        if !out.iter().any(|o| o == k) {
            out.push((*k).to_string());
        }
    }
    out
}

/// Earlier builds were called terrio-gui and kept their files in a folder of that name.
/// Copy them to the new folder once, leaving the old ones untouched.
fn migrate_old_config() {
    let Some(base) = dirs::config_dir() else { return };
    let (old, new) = (base.join("terrio-gui"), base.join("geoconveyor"));
    if !old.is_dir() || new.exists() {
        return;
    }
    fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
        if std::fs::create_dir_all(to).is_err() {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(from) {
            for e in rd.flatten() {
                let (p, q) = (e.path(), to.join(e.file_name()));
                if p.is_dir() {
                    copy_dir(&p, &q);
                } else {
                    let _ = std::fs::copy(&p, &q);
                }
            }
        }
    }
    copy_dir(&old, &new);
}
