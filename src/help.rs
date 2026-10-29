//! Reading terrio's own `--help` (clap output) so the GUI offers every flag the
//! installed version has.

use crate::i18n::t;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Opt {
    pub long: String,
    pub short: String,
    pub metas: Vec<String>,
    pub desc: String,
    pub default: String,
    pub section: String,
    pub required: bool,
    pub repeatable: bool,
}

impl Opt {
    pub fn is_flag(&self) -> bool {
        self.metas.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Help {
    pub summary: String,
    pub options: Vec<Opt>,
    pub notes: String,
    pub subcommands: Vec<(String, String)>,
}

struct Res {
    opt: Regex,
    section: Regex,
    sub: Regex,
    meta: Regex,
    default: Regex,
    strip: Regex,
    req: Regex,
}

fn res() -> &'static Res {
    static R: OnceLock<Res> = OnceLock::new();
    R.get_or_init(|| Res {
        opt: Regex::new(r"^\s{2,6}(?:(-[A-Za-z]), )?(--[\w-]+)((?: <[^>]+>)*)(?:\s{2,}(\S.*))?$").unwrap(),
        section: Regex::new(r"^([A-Z][A-Za-z ]+):\s*$").unwrap(),
        sub: Regex::new(r"^  (\w[\w-]*)\s+(.*)$").unwrap(),
        meta: Regex::new(r"<([^>]+)>").unwrap(),
        default: Regex::new(r"\s*\[default: ([^\]]*)\]").unwrap(),
        strip: Regex::new(r"\s*\[(?:default|alias|possible values): [^\]]*\]").unwrap(),
        req: Regex::new(r"(--[\w-]+) <").unwrap(),
    })
}

pub fn parse_help(text: &str) -> Help {
    let r = res();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    let mut summary: Vec<&str> = Vec::new();
    while i < lines.len() && !lines[i].starts_with("Usage:") {
        summary.push(lines[i]);
        i += 1;
    }
    let mut usage = lines.get(i).copied().unwrap_or("").replace("[OPTIONS]", "");
    // clap wraps long usage lines; join the indented continuation lines before looking for
    // required options, otherwise a required flag on the second line is missed.
    let mut u = i + 1;
    while u < lines.len() && lines[u].starts_with(' ') && !lines[u].trim().is_empty() {
        usage.push(' ');
        usage.push_str(lines[u].trim());
        u += 1;
    }
    let mut section = String::new();
    let mut cur: Option<usize> = None;
    let mut opts: Vec<Opt> = Vec::new();
    let mut subs: Vec<(String, String)> = Vec::new();
    let mut notes: Vec<&str> = Vec::new();

    for ln in lines.iter().skip(i + 1) {
        if let Some(c) = r.section.captures(ln) {
            section = c[1].to_string();
            cur = None;
            continue;
        }
        if section == "Commands" {
            if let Some(c) = r.sub.captures(ln) {
                if &c[1] != "help" {
                    subs.push((c[1].to_string(), c[2].to_string()));
                }
            }
            continue;
        }
        if section.ends_with("ptions") {
            if let Some(c) = r.opt.captures(ln) {
                let metas = r.meta.captures_iter(c.get(3).map_or("", |m| m.as_str())).map(|m| m[1].to_string()).collect();
                opts.push(Opt {
                    long: c[2].to_string(),
                    short: c.get(1).map_or("", |m| m.as_str()).to_string(),
                    metas,
                    desc: c.get(4).map_or("", |m| m.as_str()).trim().to_string(),
                    section: section.clone(),
                    ..Default::default()
                });
                cur = Some(opts.len() - 1);
                continue;
            }
            if let Some(ci) = cur {
                if ln.starts_with("       ") && !ln.trim().is_empty() {
                    let o = &mut opts[ci];
                    o.desc = format!("{} {}", o.desc, ln.trim()).trim().to_string();
                    continue;
                }
            }
            if ln.trim().is_empty() {
                continue;
            }
        }
        if !ln.trim().is_empty() && !ln.starts_with(' ') {
            notes.push(ln);
            cur = None;
        }
    }

    let required: Vec<String> = r.req.captures_iter(&usage).map(|c| c[1].to_string()).collect();
    for o in &mut opts {
        if let Some(c) = r.default.captures(&o.desc) {
            o.default = c[1].to_string();
        }
        o.desc = r.strip.replace_all(&o.desc, "").trim().to_string();
        o.required = required.contains(&o.long);
        let low = o.desc.to_lowercase();
        o.repeatable = low.contains("repeat") || o.metas.iter().any(|m| m.contains('=')) || o.long == "--layer";
    }
    opts.retain(|o| o.long != "--help" && o.long != "--version");
    Help { summary: summary.iter().filter(|s| !s.trim().is_empty()).cloned().collect::<Vec<_>>().join("\n"), options: opts, notes: notes.join("\n"), subcommands: subs }
}

// ---------------------------------------------------------------------------
// Running terrio for help / formats
// ---------------------------------------------------------------------------
/// True inside a Flatpak sandbox, where the host's terrio is only reachable through
/// `flatpak-spawn --host`.
pub fn in_flatpak() -> bool {
    std::env::var_os("FLATPAK_ID").is_some() && Path::new("/.flatpak-info").exists()
}

pub fn command(exe: &str) -> Command {
    #[cfg(unix)]
    if in_flatpak() {
        let mut c = Command::new("flatpak-spawn");
        c.arg("--host").arg(exe);
        return c;
    }
    let c = Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = c;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        return c;
    }
    #[cfg(not(windows))]
    c
}

/// Run `exe args…`, returning (success, stdout [+ stderr on failure]).
pub fn run_text(exe: &str, args: &[&str]) -> (bool, String) {
    match command(exe).args(args).output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            if !o.status.success() {
                s.push_str(&String::from_utf8_lossy(&o.stderr));
            }
            (o.status.success(), s)
        }
        Err(e) => (false, e.to_string()),
    }
}

#[derive(Default)]
pub struct HelpCache {
    map: HashMap<Vec<String>, Option<Help>>,
}

impl HelpCache {
    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn get(&mut self, exe: &str, argv: &[String]) -> Option<&Help> {
        let key = argv.to_vec();
        if !self.map.contains_key(&key) {
            let h = if exe.is_empty() {
                None
            } else {
                let mut a: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
                a.push("--help");
                let (ok, text) = run_text(exe, &a);
                if ok && !text.is_empty() {
                    Some(parse_help(&text))
                } else {
                    None
                }
            };
            self.map.insert(key.clone(), h);
        }
        self.map[&key].as_ref()
    }
}

/// terrio's location, its formats, and the cached `--help` of every command.
#[derive(Default)]
pub struct Backend {
    pub exe: String,
    pub formats: Vec<Format>,
    pub help: HelpCache,
    /// terrio exists but Windows could not start it (e.g. VCRUNTIME140.dll missing).
    pub problem: Option<LaunchProblem>,
}

/// Microsoft's permanent link to the latest Visual C++ Redistributable (x64).
pub const VC_REDIST_URL: &str = "https://aka.ms/vs/17/release/vc_redist.x64.exe";

/// Why Windows refused to start terrio.exe, read from the process exit code.
/// terrio's Windows builds link the Visual C++ runtime as a DLL, which many PCs lack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchProblem {
    /// STATUS_DLL_NOT_FOUND / STATUS_ENTRYPOINT_NOT_FOUND: a DLL is missing or too old.
    MissingDll,
    /// STATUS_INVALID_IMAGE_FORMAT: 32/64-bit or ARM build mismatch.
    WrongBuild,
}

impl LaunchProblem {
    pub fn from_code(code: Option<i32>) -> Option<LaunchProblem> {
        match code? as u32 {
            0xC000_0135 | 0xC000_0139 => Some(LaunchProblem::MissingDll),
            0xC000_007B => Some(LaunchProblem::WrongBuild),
            _ => None,
        }
    }

    pub fn message(self) -> String {
        match self {
            LaunchProblem::MissingDll => t("terrio could not start because a Windows component it needs is missing (usually VCRUNTIME140.dll). Install the Microsoft Visual C++ Redistributable (x64), then press Reload."),
            LaunchProblem::WrongBuild => t("terrio could not start: this terrio build does not match this PC (32/64-bit or ARM). Reinstall terrio, then press Reload."),
        }
    }
}

impl Backend {
    pub fn new(exe: &str) -> Backend {
        let mut b = Backend::default();
        b.reload(exe);
        b
    }

    pub fn reload(&mut self, exe: &str) {
        self.exe = exe.to_string();
        self.help.clear();
        self.problem = if exe.is_empty() { None } else { command(exe).arg("--version").output().ok().and_then(|o| LaunchProblem::from_code(o.status.code())) };
        self.formats = if self.problem.is_some() { vec![] } else { get_formats(exe) };
    }
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Format {
    pub name: String,
    #[serde(default)]
    pub long_name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default, alias = "readable", alias = "can_read")]
    pub read: bool,
    #[serde(default, alias = "writable", alias = "can_write")]
    pub write: bool,
}

#[derive(serde::Deserialize)]
struct FormatsDoc {
    formats: Vec<Format>,
}

pub fn get_formats(exe: &str) -> Vec<Format> {
    if exe.is_empty() {
        return vec![];
    }
    let (ok, text) = run_text(exe, &["formats", "--json"]);
    if !ok {
        return vec![];
    }
    serde_json::from_str(&text).or_else(|_| serde_json::from_str::<FormatsDoc>(&text).map(|d| d.formats)).unwrap_or_default()
}

pub fn find_terrio(hint: &str) -> String {
    if in_flatpak() {
        // ask the host's login shell, so ~/.local/bin and friends are on PATH
        let out = Command::new("flatpak-spawn").args(["--host", "sh", "-lc", "command -v terrio"]).output();
        if let Ok(o) = out {
            let p = String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").trim().to_string();
            if o.status.success() && !p.is_empty() {
                return p;
            }
        }
        if !hint.is_empty() {
            return hint.to_string();
        }
        return String::new();
    }
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    if !hint.is_empty() {
        cands.push(hint.into());
    }
    for n in ["terrio", "terrio.exe"] {
        if let Ok(p) = which::which(n) {
            cands.push(p);
        }
    }
    if let Some(h) = dirs::home_dir() {
        for d in [".local/bin", ".cargo/bin", "AppData/Roaming/Python/Scripts"] {
            for n in ["terrio", "terrio.exe"] {
                cands.push(h.join(d).join(n));
            }
        }
    }
    for c in cands {
        if Path::new(&c).is_file() {
            return c.to_string_lossy().into_owned();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Convert between formats\n\nUsage: terrio convert [OPTIONS] <INPUT> <OUTPUT>\n\nOptions:\n  -f, --format <FORMAT>  Output format [default: COG]\n      --co <KEY=VALUE>   Creation option (repeatable)\n      --tr <X> <Y>       Target resolution\n      --cutline <FILE>   Clip\n                         to a polygon\n  -h, --help             Print help\n";

    #[test]
    fn parses_flags_defaults_and_continuations() {
        let h = parse_help(SAMPLE);
        assert_eq!(h.summary, "Convert between formats");
        let by = |n: &str| h.options.iter().find(|o| o.long == n).unwrap();
        assert_eq!(by("--format").default, "COG");
        assert_eq!(by("--format").short, "-f");
        assert!(by("--co").repeatable);
        assert_eq!(by("--tr").metas.len(), 2);
        assert_eq!(by("--cutline").desc, "Clip to a polygon");
        assert!(h.options.iter().all(|o| o.long != "--help"));
    }

    #[test]
    fn windows_loader_errors_are_recognised() {
        assert_eq!(LaunchProblem::from_code(Some(0xC000_0135_u32 as i32)), Some(LaunchProblem::MissingDll));
        assert_eq!(LaunchProblem::from_code(Some(0xC000_007B_u32 as i32)), Some(LaunchProblem::WrongBuild));
        assert_eq!(LaunchProblem::from_code(Some(1)), None);
        assert_eq!(LaunchProblem::from_code(None), None);
    }

    #[test]
    fn required_comes_from_usage_line() {
        let t = "x\n\nUsage: terrio calc [OPTIONS] --calc <EXPR> <OUT>\n\nOptions:\n      --calc <EXPR>  Expression\n      --type <T>  Type\n";
        let h = parse_help(t);
        assert!(h.options.iter().find(|o| o.long == "--calc").unwrap().required);
        assert!(!h.options.iter().find(|o| o.long == "--type").unwrap().required);
    }

    #[test]
    fn wrapped_usage_lines_are_joined() {
        let t = "x\n\nUsage: terrio calc [OPTIONS]\n         --calc <EXPR>\n         <OUTPUT>\n\nOptions:\n      --calc <EXPR>  Expression\n      --type <T>  Type\n";
        let h = parse_help(t);
        assert!(h.options.iter().find(|o| o.long == "--calc").unwrap().required);
        assert!(!h.options.iter().find(|o| o.long == "--type").unwrap().required);
    }

    #[test]
    fn parses_every_captured_help_fixture() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/help");
        let mut parsed = 0;
        for e in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
            let text = std::fs::read_to_string(e.path()).unwrap();
            if !text.contains("Usage:") {
                continue; // filter_subs.txt lists subcommands, not a clap help page
            }
            let h = parse_help(&text);
            assert!(!h.summary.trim().is_empty(), "{:?} has no summary", e.path());
            for o in &h.options {
                assert!(o.long.starts_with("--"), "{:?} parsed a bad option {:?}", e.path(), o.long);
            }
            parsed += 1;
        }
        assert!(parsed >= 35, "only {parsed} help fixtures were parsed");
    }

    #[test]
    fn formats_json_fixture_and_aliases() {
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/formats.json")).unwrap();
        let formats: Vec<Format> = serde_json::from_str(&text).unwrap();
        assert!(formats.len() >= 40, "{} formats", formats.len());
        assert_eq!(formats[0].name, "GTiff");
        assert!(formats.iter().any(|f| f.name == "COG" && f.write));
        assert!(formats.iter().filter(|f| f.write).count() > 30);

        let alias = r#"[{"name":"X","writable":true,"readable":false}]"#;
        let f: Vec<Format> = serde_json::from_str(alias).unwrap();
        assert!(f[0].write && !f[0].read);
    }
}
