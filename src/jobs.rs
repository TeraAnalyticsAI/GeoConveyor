//! Turning a tab's settings plus a folder of files into a list of terrio invocations.

use crate::i18n::{t, tf};
use crate::model::{Globals, TabState};
use crate::specs::Kind;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
    Cancelled,
}

impl Status {
    pub fn name(&self) -> &'static str {
        match self {
            Status::Pending => "Pending",
            Status::Running => "Running",
            Status::Done => "Done",
            Status::Failed => "Failed",
            Status::Skipped => "Skipped",
            Status::Cancelled => "Cancelled",
        }
    }

    pub fn finished(&self) -> bool {
        matches!(self, Status::Done | Status::Failed | Status::Skipped | Status::Cancelled)
    }
}

#[derive(Clone, Debug)]
pub struct Job {
    pub label: String,
    /// Arguments up to (not including) the output path.
    pub pre: Vec<String>,
    pub dst: Option<PathBuf>,
    /// Write straight to `dst` (outputs that cannot be renamed into place).
    pub direct: bool,
    pub size_in: u64,
    pub status: Status,
    pub seconds: f64,
    pub size_out: u64,
    pub message: String,
}

impl Job {
    fn new(label: String, pre: Vec<String>, dst: Option<PathBuf>, size_in: u64) -> Job {
        let direct = dst.as_ref().is_some_and(|d| {
            let n = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            n.contains('#') || n == "ept.json"
        });
        Job { label, pre, dst, direct, size_in, status: Status::Pending, seconds: 0.0, size_out: 0, message: String::new() }
    }

    pub fn tmp(&self) -> Option<PathBuf> {
        self.dst.as_ref().map(|d| {
            let n = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            d.with_file_name(format!(".partial-{n}"))
        })
    }

    pub fn argv(&self, out: &str) -> Vec<String> {
        let mut v = self.pre.clone();
        if self.dst.is_some() {
            v.push(out.to_string());
        }
        v
    }

    pub fn display_argv(&self) -> Vec<String> {
        self.argv(&self.dst.as_ref().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default())
    }

    pub fn output_exists(&self) -> bool {
        self.dst.as_ref().is_some_and(|d| output_exists(d))
    }

    pub fn output_size(&self) -> u64 {
        self.dst.as_ref().map(|d| output_size(d)).unwrap_or(0)
    }

    pub fn remove_output(&self) {
        if let Some(d) = &self.dst {
            remove_output(d);
        }
    }
}

// ---------------------------------------------------------------------------
// outputs, quoting, sizes, matching
// ---------------------------------------------------------------------------
pub fn remove_path(p: &Path) {
    if p.is_dir() {
        let _ = std::fs::remove_dir_all(p);
    } else if p.exists() {
        let _ = std::fs::remove_file(p);
    }
}

/// Splitter outputs are a template: the `#` in the file name is replaced by a tile number.
fn split_template(dst: &Path) -> Option<(PathBuf, String, String)> {
    let name = dst.file_name()?.to_string_lossy().into_owned();
    let (prefix, suffix) = name.split_once('#')?;
    Some((dst.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(), prefix.to_string(), suffix.to_string()))
}

fn is_ept(dst: &Path) -> bool {
    dst.file_name().is_some_and(|n| n == "ept.json")
}

/// The files an output path actually produces: one file, an EPT dataset (`ept.json`), or the
/// set of tiles written from a splitter's `#` template.
pub fn output_paths(dst: &Path) -> Vec<PathBuf> {
    if let Some((dir, prefix, suffix)) = split_template(dst) {
        let mut out = vec![];
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.len() > prefix.len() + suffix.len() && name.starts_with(&prefix) && name.ends_with(&suffix) {
                    // terrio replaces '#' with a tile number; don't match unrelated files.
                    let middle = &name[prefix.len()..name.len() - suffix.len()];
                    if middle.chars().all(|c| c.is_ascii_digit()) {
                        out.push(e.path());
                    }
                }
            }
        }
        out
    } else if dst.exists() {
        vec![dst.to_path_buf()]
    } else {
        vec![]
    }
}

pub fn output_exists(dst: &Path) -> bool {
    if is_ept(dst) {
        // A dataset directory that exists but has no ept.json is a partial run, not "nothing".
        return dst.parent().is_some_and(Path::is_dir) || dst.exists();
    }
    !output_paths(dst).is_empty()
}

/// Size of a finished output, including every tile of a splitter and the whole EPT directory.
pub fn output_size(dst: &Path) -> u64 {
    if is_ept(dst) {
        dst.parent().map(path_size).unwrap_or(0)
    } else if split_template(dst).is_some() {
        output_paths(dst).iter().map(|p| path_size(p)).sum()
    } else {
        path_size(dst)
    }
}

/// Remove an output before a run (overwrite) or after a failure, including splitter tiles and
/// EPT datasets that are written directly rather than through a temporary file.
pub fn remove_output(dst: &Path) {
    if let Some((_, _, _)) = split_template(dst) {
        for p in output_paths(dst) {
            remove_path(&p);
        }
    } else if is_ept(dst) {
        if let Some(dir) = dst.parent() {
            remove_path(dir);
        }
    } else {
        remove_path(dst);
    }
}

// ---------------------------------------------------------------------------
// quoting, sizes, matching
// ---------------------------------------------------------------------------
fn quote_posix(a: &str) -> String {
    let safe = !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c));
    if safe {
        a.to_string()
    } else {
        format!("'{}'", a.replace('\'', "'\"'\"'"))
    }
}

fn quote_windows(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '\n', '"']) {
        return a.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for c in a.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                out.push(c);
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

pub fn shell_join(parts: &[String]) -> String {
    let q: fn(&str) -> String = if cfg!(windows) { quote_windows } else { quote_posix };
    parts.iter().map(|p| q(p)).collect::<Vec<_>>().join(" ")
}

pub fn shell_join_for(parts: &[String], windows: bool) -> String {
    let q: fn(&str) -> String = if windows { quote_windows } else { quote_posix };
    parts.iter().map(|p| q(p)).collect::<Vec<_>>().join(" ")
}

pub fn path_size(p: &Path) -> u64 {
    if p.is_dir() {
        walkdir::WalkDir::new(p).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file()).filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()
    } else {
        std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
    }
}

/// Case-insensitive wildcard match supporting `*` and `?`.
pub fn wildcard(pattern: &str, name: &str) -> bool {
    fn rec(p: &[char], n: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|i| rec(&p[1..], &n[i..])),
            Some('?') => !n.is_empty() && rec(&p[1..], &n[1..]),
            Some(c) => n.first() == Some(c) && rec(&p[1..], &n[1..]),
        }
    }
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    rec(&p, &n)
}

/// Resolve like Python's non-strict `Path.resolve()`: works for paths that do not exist yet.
fn norm(p: &Path) -> PathBuf {
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    match (p.parent(), p.file_name()) {
        (Some(par), Some(name)) if !par.as_os_str().is_empty() => norm(par).join(name),
        _ => p.to_path_buf(),
    }
}

pub fn scan_files(in_dir: &str, patterns: &str, recursive: bool, skip_dir: &str) -> Vec<PathBuf> {
    let root = Path::new(in_dir);
    let pats: Vec<String> = patterns.replace(',', ";").split(';').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect();
    let pats = if pats.is_empty() { vec!["*".to_string()] } else { pats };
    let skip = if skip_dir.trim().is_empty() { None } else { Some(norm(Path::new(skip_dir.trim()))) };
    let mut out = vec![];
    let mut walker = walkdir::WalkDir::new(root).sort_by_file_name();
    if !recursive {
        walker = walker.max_depth(1);
    }
    let it = walker.into_iter().filter_entry(|e| !(e.file_type().is_dir() && e.depth() > 0 && skip.as_ref().is_some_and(|s| norm(e.path()) == *s)));
    for e in it.filter_map(Result::ok) {
        if !e.file_type().is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(".partial-") {
            continue;
        }
        if pats.iter().any(|p| wildcard(p, &name)) {
            out.push(e.into_path());
        }
    }
    out
}

pub fn stem_of(name: &str) -> String {
    let low = name.to_lowercase();
    for p in [".copc.laz", ".laz", ".las", ".tiff", ".tif"] {
        if low.ends_with(p) {
            return name[..name.len() - p.len()].to_string();
        }
    }
    Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| name.to_string())
}

fn dst_for(tab: &TabState, src: &Path, root: &Path, out: &Path, keep_tree: bool, used: &mut HashSet<PathBuf>) -> Result<PathBuf, String> {
    let rel = if keep_tree { src.parent().and_then(|p| p.strip_prefix(root).ok()).map(Path::to_path_buf).unwrap_or_default() } else { PathBuf::new() };
    let ext = tab.ext.trim();
    let stem = stem_of(&src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let make = |n: usize| -> PathBuf {
        let tag = if n > 1 { format!("{}_{}", tab.suffix, n) } else { tab.suffix.clone() };
        if ext.ends_with("ept.json") {
            out.join(&rel).join(format!("{stem}{tag}")).join("ept.json")
        } else {
            out.join(&rel).join(format!("{stem}{tag}{ext}"))
        }
    };
    let mut n = 1;
    let mut dst = make(n);
    while used.contains(&dst) {
        n += 1;
        dst = make(n);
    }
    refuse_input_overwrite(&dst, src)?;
    used.insert(dst.clone());
    Ok(dst)
}

/// Refuse to write an output on top of one of the inputs (which would destroy source data).
fn refuse_input_overwrite(dst: &Path, src: &Path) -> Result<(), String> {
    if norm(dst) == norm(src) {
        return Err(tf(
            "The output would overwrite the input {}. Use another output folder or a name suffix.",
            &[src.file_name().unwrap_or_default().to_string_lossy().into_owned()],
        ));
    }
    Ok(())
}

fn rel_label(f: &Path, in_dir: &str) -> String {
    f.strip_prefix(in_dir.trim()).unwrap_or(f).to_string_lossy().into_owned()
}

/// Build the jobs for `files`. With `example`, sizes are not read from disk (used by the preview).
pub fn make_jobs(tab: &TabState, g: &Globals, files: &[PathBuf], example: bool) -> Result<Vec<Job>, String> {
    let exe_args = tab.argv();
    let tokens = match tab.form() {
        Some(f) => f.tokens()?,
        None => vec![],
    };
    let size = |p: &Path| if example { 0 } else { path_size(p) };

    match tab.spec.kind {
        Kind::Formats => {
            let mut pre = exe_args;
            pre.extend(tokens);
            return Ok(vec![Job::new("terrio formats".into(), pre, None, 0)]);
        }
        Kind::Crs => {
            let mut pre = exe_args;
            pre.extend(tokens);
            pre.push(tab.crs.trim().to_string());
            pre.extend(tab.coords.split_whitespace().map(String::from));
            return Ok(vec![Job::new(format!("terrio crs {}", tab.crs.trim()), pre, None, 0)]);
        }
        Kind::Info => {
            return Ok(files
                .iter()
                .map(|f| {
                    let mut pre = exe_args.clone();
                    pre.extend(tokens.clone());
                    pre.push(f.to_string_lossy().into_owned());
                    Job::new(rel_label(f, &g.in_dir), pre, None, size(f))
                })
                .collect());
        }
        _ => {}
    }

    let root = PathBuf::from(if g.in_dir.trim().is_empty() { "." } else { g.in_dir.trim() });
    let out_root = PathBuf::from(if g.out_dir.trim().is_empty() { "output" } else { g.out_dir.trim() });
    let ext = tab.ext.trim();
    let color = tab.color.trim();

    let pre_for = |srcs: &[PathBuf]| -> Vec<String> {
        let mut v = exe_args.clone();
        v.extend(tokens.clone());
        match tab.spec.kind {
            Kind::Dem => {
                v.push(tab.dem_mode.clone());
                v.push(srcs[0].to_string_lossy().into_owned());
                if tab.dem_mode == "color-relief" {
                    v.push(if color.is_empty() { "<color-file>".to_string() } else { color.to_string() });
                }
            }
            Kind::Calc => {
                for (n, s) in srcs.iter().take(26).enumerate() {
                    v.push("--input".into());
                    v.push(format!("{}={}", (b'A' + n as u8) as char, s.to_string_lossy()));
                }
            }
            _ => v.extend(srcs.iter().map(|s| s.to_string_lossy().into_owned())),
        }
        v
    };

    if tab.effective_together() {
        if files.is_empty() {
            return Ok(vec![]);
        }
        if tab.spec.kind == Kind::Calc && tab.form().and_then(|f| f.get("--input")).is_some_and(|f| f.is_set()) {
            return Err(t("In 'together' mode the files are added as --input A, B, C…; remove the --input option, or use 'Each file separately'."));
        }
        let name = tab.name.trim();
        if name.is_empty() {
            return Err(t("Enter an output name."));
        }
        let dst = if ext.ends_with("ept.json") { out_root.join(name).join("ept.json") } else { out_root.join(format!("{name}{ext}")) };
        if let Some(src) = files.iter().find(|f| norm(f) == norm(&dst)) {
            refuse_input_overwrite(&dst, src)?;
        }
        let label = tf("{} files → {}", &[files.len().to_string(), dst.file_name().unwrap_or_default().to_string_lossy().into_owned()]);
        let total = files.iter().map(|f| size(f)).sum();
        return Ok(vec![Job::new(label, pre_for(files), Some(dst), total)]);
    }

    let mut used = HashSet::new();
    let mut jobs = vec![];
    for f in files {
        let dst = dst_for(tab, f, &root, &out_root, g.keep_tree, &mut used)?;
        jobs.push(Job::new(rel_label(f, &g.in_dir), pre_for(std::slice::from_ref(f)), Some(dst), size(f)));
    }
    Ok(jobs)
}

/// The command shown in the preview line, using example file names.
pub fn preview_parts(tab: &TabState, g: &Globals, exe: &str) -> Result<Vec<String>, String> {
    // example names that look like what the tab scans for (input.las for point clouds, …)
    let ex = tab.patterns.split(';').next().unwrap_or("*.tif").trim().replace('*', "input");
    let ex = if ex.contains('.') && !ex.contains('?') { ex } else { "input.tif".to_string() };
    let files: Vec<PathBuf> = if tab.effective_together() { vec![ex.replace("input", "a").into(), ex.replace("input", "b").into()] } else { vec![ex.into()] };
    let files = if matches!(tab.spec.kind, Kind::Crs | Kind::Formats) || tab.form().is_none() { vec![] } else { files };
    let jobs = make_jobs(tab, g, &files, true)?;
    let Some(j) = jobs.first() else { return Ok(vec![]) };
    let name = Path::new(if exe.is_empty() { "terrio" } else { exe }).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "terrio".into());
    let mut parts = vec![name];
    parts.extend(j.display_argv());
    Ok(parts)
}

/// The preview line as a copyable shell command.
pub fn preview(tab: &TabState, g: &Globals, exe: &str) -> String {
    match preview_parts(tab, g, exe) {
        Ok(parts) => shell_join(&parts),
        Err(e) => format!("# {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::help::Opt;
    use crate::model::{Field, Form, Value};
    use crate::specs::specs;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("geoconveyor-unit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn spec(key: &str) -> crate::specs::Spec {
        specs().into_iter().find(|s| s.key == key).unwrap()
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("*.tif", "A.TIF"));
        assert!(wildcard("tile_?.tif", "tile_1.tif"));
        assert!(!wildcard("*.tif", "a.tiff"));
        assert!(wildcard("*", "anything"));
    }

    #[test]
    fn stems() {
        assert_eq!(stem_of("a.copc.laz"), "a");
        assert_eq!(stem_of("scene.v2.TIF"), "scene.v2");
        assert_eq!(stem_of("x.geojson"), "x");
    }

    #[test]
    fn quoting() {
        assert_eq!(shell_join_for(&["a b".into(), "it's".into(), "".into(), "ok/x.tif".into()], false), "'a b' 'it'\"'\"'s' '' ok/x.tif");
        assert_eq!(shell_join_for(&["C:\\my dir\\".into(), "x".into()], true), "\"C:\\my dir\\\\\" x");
    }

    #[test]
    fn scan_respects_patterns_recursion_and_partials() {
        let d = tmp("scan");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        for n in ["a.tif", "B.TIFF", "c.laz", ".partial-x.tif"] {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        std::fs::write(d.join("sub/d.tif"), b"x").unwrap();
        let names = |v: Vec<PathBuf>| v.into_iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        let flat = names(scan_files(d.to_str().unwrap(), "*.tif;*.tiff", false, ""));
        assert_eq!(flat, vec!["B.TIFF", "a.tif"]);
        let deep = names(scan_files(d.to_str().unwrap(), "*.tif", true, ""));
        assert_eq!(deep, vec!["a.tif", "d.tif"]);
        let skipped = names(scan_files(d.to_str().unwrap(), "*.tif", true, d.join("sub").to_str().unwrap()));
        assert_eq!(skipped, vec!["a.tif"]);
    }

    #[test]
    fn output_names_do_not_repeat_and_never_overwrite_the_input() {
        let d = tmp("dst");
        let a = d.join("a.tif");
        std::fs::write(&a, b"x").unwrap();
        let mut tab = TabState::new(spec("convert"));
        tab.ext = ".tif".into();
        tab.suffix = "_out".into();
        let mut used = HashSet::new();
        let first = dst_for(&tab, &a, &d, &d, false, &mut used).unwrap();
        let second = dst_for(&tab, &a, &d, &d, false, &mut used).unwrap();
        assert_ne!(first, second);
        assert_eq!(second.file_name().unwrap(), "a_out_2.tif");

        tab.suffix = String::new();
        let err = dst_for(&tab, &a, &d, &d, false, &mut HashSet::new()).err().unwrap();
        assert!(err.contains("overwrite the input"), "{err}");
    }

    #[test]
    fn together_mode_refuses_to_overwrite_an_input() {
        let d = tmp("together");
        let a = d.join("tile_0.tif");
        std::fs::write(&a, b"x").unwrap();
        let mut tab = TabState::new(spec("mosaic"));
        tab.name = "tile_0".into();
        tab.ext = ".tif".into();
        let g = Globals { in_dir: d.to_string_lossy().into(), out_dir: d.to_string_lossy().into(), ..Globals::default() };
        let err = make_jobs(&tab, &g, std::slice::from_ref(&a), false).err().unwrap();
        assert!(err.contains("overwrite the input"), "{err}");
    }

    #[test]
    fn calc_together_rejects_a_manual_input_option() {
        let d = tmp("calc");
        let a = d.join("a.tif");
        std::fs::write(&a, b"x").unwrap();
        let mut tab = TabState::new(spec("calc"));
        tab.together = true;
        tab.name = "sum".into();
        let opt = Opt { long: "--input".into(), metas: vec!["NAME=PATH".into()], section: "Options".into(), ..Default::default() };
        let mut f = Form { sections: vec!["Options".into()], ..Default::default() };
        f.fields.push(Field::new(opt, vec![]));
        f.set("--input", Value::Text("B=other.tif".into()));
        tab.forms.insert(String::new(), f);
        let g = Globals { in_dir: d.to_string_lossy().into(), out_dir: d.to_string_lossy().into(), ..Globals::default() };
        let err = make_jobs(&tab, &g, &[a], false).err().unwrap();
        assert!(err.contains("--input"), "{err}");
    }

    #[test]
    fn splitter_and_ept_outputs_are_found_sized_and_removed() {
        let d = tmp("direct");
        for (n, body) in [("tile1.copc.laz", b"12345".as_slice()), ("tile2.copc.laz", b"1234567890".as_slice()), ("tile_notes.copc.laz", b"keep".as_slice())] {
            std::fs::write(d.join(n), body).unwrap();
        }
        let template = d.join("tile#.copc.laz");
        assert!(output_exists(&template));
        assert_eq!(output_size(&template), 15);
        remove_output(&template);
        assert!(!output_exists(&template));
        assert!(d.join("tile_notes.copc.laz").exists(), "unrelated files must survive");

        let ept = d.join("points");
        std::fs::create_dir_all(&ept).unwrap();
        std::fs::write(ept.join("ept.json"), b"{}").unwrap();
        std::fs::write(ept.join("data.bin"), b"1234").unwrap();
        let dst = ept.join("ept.json");
        assert!(output_exists(&dst));
        assert_eq!(output_size(&dst), 6);
        remove_output(&dst);
        assert!(!ept.exists());
    }

    #[test]
    fn info_preview_names_the_command_and_input() {
        let mut tab = TabState::new(spec("info"));
        tab.forms.insert(String::new(), Form::default());
        let g = Globals { in_dir: "/in".into(), out_dir: "/out".into(), ..Globals::default() };
        assert_eq!(preview(&tab, &g, "/usr/bin/terrio"), "terrio info input.tif");
    }
}
