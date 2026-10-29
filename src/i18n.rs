//! Language packs.
//!
//! The English text in the source is the key: `t("Start")`. A pack is a JSON file:
//!
//! ```text
//! { "code": "es", "name": "Español", "strings": { "Start": "Iniciar", "Cancel": "Cancelar" } }
//! ```
//!
//! English, Spanish, French and German are built in. More packs can be dropped into
//! `<config dir>/geoconveyor/lang/` (e.g. `pt.json`) and appear in Settings after a restart,
//! with no rebuild. A missing key falls back to the English text.
//!
//! Only the GUI's own text is translated. Option names and descriptions come from
//! terrio's `--help` and stay as terrio writes them.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Pack {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub strings: HashMap<String, String>,
}

/// Packs compiled into the program.
const BUILT_IN: &[&str] = &[include_str!("../lang/en.json"), include_str!("../lang/es.json"), include_str!("../lang/fr.json"), include_str!("../lang/de.json")];

/// Languages planned but not shipped yet: shown greyed out in Settings.
/// (Chinese and Japanese also need a CJK font loaded before they can be drawn.)
pub const PLANNED: &[(&str, &str)] = &[("pt", "Português"), ("it", "Italiano"), ("zh", "Chinese"), ("ja", "Japanese")];

/// The Help window: (heading, text).
pub const HELP: &[(&str, &str)] = &[
    ("Getting started", "Choose an input folder and an output folder at the top, pick a function tab, set its options and press Start. Every matching file in the input folder is processed and written to the output folder."),
    ("Options", "Each tab's options are read from your own terrio (terrio <command> --help), so every flag it has is here. Use Search options to find one by name or description, and Reload in Settings after upgrading terrio."),
    ("Convert and input types", "On the Convert tab, choose the input type (Image, Vector or Point cloud) to see only the options that apply to it. 'All options' shows everything. Options that are hidden are not added to the command."),
    ("Tabs", "The menu at the top right opens a new tab, so you can start another job while the first one is still running. Each tab has its own folders, options, progress and log."),
    ("Parallel files", "terrio runs once per file. 'Parallel files' sets how many run at the same time; lower it for very large files."),
    ("Commands for scripts", "Copy command shows the exact command for one example file; Copy all commands scans the folder and copies one command per file."),
    ("Safe output", "Each result is written under a temporary name and renamed when terrio succeeds, so a failed or cancelled file never leaves a half-written output."),
    ("Settings", "Choose light or dark mode, the language, the order of the function tabs and where terrio is installed."),
];

/// Text that reaches `t()` through a variable, so it cannot be found by scanning for `t("...")`.
/// (Tab titles, input-type labels and job statuses are added by the test from their own lists.)
pub const DYNAMIC_KEYS: &[&str] = &[
    "System",
    "Light",
    "Dark",
    "File",
    "Size",
    "Status",
    "Time",
    "Output size / note",
    "{} failed",
    "{} skipped",
    "{} cancelled",
    "Options",
    "Vector options",
    "Point cloud options",
];

static CURRENT: RwLock<Option<Arc<Pack>>> = RwLock::new(None);

pub fn built_in() -> Vec<Pack> {
    BUILT_IN.iter().map(|s| serde_json::from_str::<Pack>(s).expect("built-in language pack")).collect()
}

fn external_dir() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("geoconveyor").join("lang"))
}

fn load_packs() -> Vec<Pack> {
    let mut packs = built_in();
    if let Some(dir) = external_dir() {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.filter_map(Result::ok) {
                if e.path().extension().is_some_and(|x| x == "json") {
                    if let Some(p) = std::fs::read_to_string(e.path()).ok().and_then(|s| serde_json::from_str::<Pack>(&s).ok()) {
                        packs.retain(|x| x.code != p.code);
                        packs.push(p);
                    }
                }
            }
        }
    }
    packs
}

fn packs_cache() -> &'static RwLock<Arc<Vec<Pack>>> {
    static PACKS: OnceLock<RwLock<Arc<Vec<Pack>>>> = OnceLock::new();
    PACKS.get_or_init(|| RwLock::new(Arc::new(load_packs())))
}

/// Built-in packs plus any found in the user's lang folder (which win on the same code).
/// Parsed once: the Settings window reads this every frame.
pub fn all_packs() -> Arc<Vec<Pack>> {
    packs_cache().read().unwrap().clone()
}

/// Re-read the language folder (e.g. after the user drops in a new pack).
pub fn refresh_packs() {
    *packs_cache().write().unwrap() = Arc::new(load_packs());
}

/// Switch language. Unknown codes fall back to English.
pub fn set_language(code: &str) {
    let pack = all_packs().iter().find(|p| p.code == code).cloned();
    let pack = pack.filter(|p| p.code != "en").map(Arc::new);
    *CURRENT.write().unwrap() = pack;
}

pub fn current_code() -> String {
    CURRENT.read().unwrap().as_ref().map(|p| p.code.clone()).unwrap_or_else(|| "en".into())
}

/// Pick a language from the environment (`LANG=es_MX.UTF-8` → `es`) when it is available.
pub fn detect() -> String {
    let raw = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.is_empty()).unwrap_or_default();
    let code: String = raw.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_lowercase();
    if all_packs().iter().any(|p| p.code == code) {
        code
    } else {
        "en".into()
    }
}

pub fn t(key: &str) -> String {
    if let Some(p) = CURRENT.read().unwrap().as_ref() {
        if let Some(s) = p.strings.get(key) {
            if !s.is_empty() {
                return s.clone();
            }
        }
    }
    key.to_string()
}

/// Translate, then fill `{}` placeholders in order.
pub fn tf(key: &str, args: &[String]) -> String {
    let mut out = String::new();
    let tpl = t(key);
    let mut it = args.iter();
    let mut rest = tpl.as_str();
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        out.push_str(it.next().map(String::as_str).unwrap_or(""));
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::Status;
    use crate::specs::{specs, InputType};
    use std::collections::BTreeSet;

    /// Every string passed to `t("…")` / `tf("…", …)` in the sources, plus the dynamic ones.
    fn used_keys() -> BTreeSet<String> {
        let re = regex::Regex::new(r#"\btf?\(\s*"((?:[^"\\]|\\.)*)""#).unwrap();
        let mut keys = BTreeSet::new();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for e in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
            let text = std::fs::read_to_string(e.path()).unwrap();
            // skip this file's own tests and docs
            let text = text.split("#[cfg(test)]").next().unwrap();
            let text: String = text.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            for c in re.captures_iter(&text) {
                let k = c[1].replace("\\\"", "\"").replace("\\'", "'").replace("\\\\", "\\");
                if !k.is_empty() && k != "…" {
                    keys.insert(k);
                }
            }
        }
        for k in DYNAMIC_KEYS {
            keys.insert(k.to_string());
        }
        for (h, b) in HELP {
            keys.insert(h.to_string());
            keys.insert(b.to_string());
        }
        for s in specs() {
            keys.insert(s.title.to_string());
        }
        for ty in InputType::ALL {
            keys.insert(ty.label().to_string());
        }
        for st in [Status::Pending, Status::Running, Status::Done, Status::Failed, Status::Skipped, Status::Cancelled] {
            keys.insert(st.name().to_string());
        }
        keys
    }

    #[test]
    fn every_built_in_pack_translates_every_string() {
        let keys = used_keys();
        assert!(keys.len() > 80, "found only {} keys", keys.len());
        for p in built_in() {
            let missing: Vec<&String> = keys.iter().filter(|k| p.strings.get(*k).is_none_or(|v| v.trim().is_empty())).collect();
            if p.code == "en" {
                // English is the template: every key present, identical text
                assert!(missing.is_empty(), "en.json is missing: {missing:?}");
                assert!(p.strings.iter().all(|(k, v)| k == v));
            } else {
                assert!(missing.is_empty(), "{} is missing {} string(s): {:?}", p.code, missing.len(), missing);
            }
            let stale: Vec<&String> = p.strings.keys().filter(|k| !keys.contains(*k)).collect();
            assert!(stale.is_empty(), "{} has strings no longer used: {:?}", p.code, stale);
        }
    }

    #[test]
    fn placeholders_survive_translation() {
        let en = built_in().into_iter().find(|p| p.code == "en").unwrap();
        for p in built_in() {
            for (k, v) in &p.strings {
                assert_eq!(k.matches("{}").count(), v.matches("{}").count(), "{}: placeholder count differs in {k:?}", p.code);
            }
        }
        assert!(!en.strings.is_empty());
    }

    #[test]
    fn tf_fills_placeholders_in_order() {
        assert_eq!(tf("{} files → {}", &["3".into(), "out.tif".into()]), "3 files → out.tif");
        assert_eq!(tf("no placeholders", &["x".into()]), "no placeholders");
    }
}
