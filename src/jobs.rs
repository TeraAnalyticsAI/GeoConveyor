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
}

// ---------------------------------------------------------------------------
// quoting, sizes, matching
// ---------------------------------------------------------------------------
fn quote_posix(a: &str) -> String {
    let safe = !a.is_empty() && a.chars().all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c));
    if safe { a.to_string() } else { format!("'{}'", a.replace('\'', "'\"'\"'")) }
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
        walkdir::WalkDir::new(p)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum()
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
    let pats: Vec<String> = patterns
        .replace(',', ";")
        .split(';')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    let pats = if pats.is_empty() { vec!["*".to_string()] } else { pats };
    let skip = if skip_dir.trim().is_empty() { None } else { Some(norm(Path::new(skip_dir.trim()))) };
    let mut out = vec![];
    let mut walker = walkdir::WalkDir::new(root).sort_by_file_name();
    if !recursive {
        walker = walker.max_depth(1);
    }
    let it = walker.into_iter().filter_entry(|e| {
        !(e.file_type().is_dir() && e.depth() > 0 && skip.as_ref().is_some_and(|s| norm(e.path()) == *s))
    });
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
    let rel = if keep_tree {
        src.parent().and_then(|p| p.strip_prefix(root).ok()).map(Path::to_path_buf).unwrap_or_default()
    } else {
        PathBuf::new()
    };
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
    if norm(&dst) == norm(src) {
        return Err(tf(
            "The output would overwrite the input {}. Use another output folder or a name suffix.",
            &[src.file_name().unwrap_or_default().to_string_lossy().into_owned()],
        ));
    }
    used.insert(dst.clone());
    Ok(dst)
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
        let name = tab.name.trim();
        if name.is_empty() {
            return Err(t("Enter an output name."));
        }
        let dst = if ext.ends_with("ept.json") { out_root.join(name).join("ept.json") } else { out_root.join(format!("{name}{ext}")) };
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
pub fn preview(tab: &TabState, g: &Globals, exe: &str) -> String {
    // example names that look like what the tab scans for (input.las for point clouds, …)
    let ex = tab.patterns.split(';').next().unwrap_or("*.tif").trim().replace('*', "input");
    let ex = if ex.contains('.') && !ex.contains('?') { ex } else { "input.tif".to_string() };
    let files: Vec<PathBuf> = if tab.effective_together() {
        vec![ex.replace("input", "a").into(), ex.replace("input", "b").into()]
    } else {
        vec![ex.into()]
    };
    let files = if matches!(tab.spec.kind, Kind::Crs | Kind::Formats) || tab.form().is_none() { vec![] } else { files };
    match make_jobs(tab, g, &files, true) {
        Ok(jobs) => match jobs.first() {
            Some(j) => {
                let name = Path::new(if exe.is_empty() { "terrio" } else { exe })
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "terrio".into());
                let mut parts = vec![name];
                parts.extend(j.display_argv());
                shell_join(&parts)
            }
            None => String::new(),
        },
        Err(e) => format!("# {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
