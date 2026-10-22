//! End-to-end checks of the engine (help parsing -> forms -> jobs -> processes) against a
//! real `terrio` on the PATH, using the small files in tests/data. Skipped when terrio
//! is not installed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use geoconveyor::help::{find_terrio, Backend};
use geoconveyor::jobs::{make_jobs, preview, scan_files, Status};
use geoconveyor::model::{Globals, TabState, Value};
use geoconveyor::runner::{apply_event, start, Event};
use geoconveyor::specs::{specs, InputType};

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn out_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("geoconveyor-tests").join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

struct Case<'a> {
    key: &'a str,
    dir: &'a str,
    patterns: Option<&'a str>,
    opts: Vec<(&'a str, Value)>,
    together: bool,
    sub: Option<&'a str>,
    ext: Option<&'a str>,
    suffix: Option<&'a str>,
    name: Option<&'a str>,
    dem: Option<&'a str>,
    color: Option<String>,
    crs: Option<&'a str>,
    coords: Option<&'a str>,
    input_type: Option<InputType>,
}

fn case<'a>(key: &'a str, dir: &'a str, patterns: &'a str) -> Case<'a> {
    Case { key, dir, patterns: Some(patterns), opts: vec![], together: false, sub: None, ext: None, suffix: None,
           name: None, dem: None, color: None, crs: None, coords: None, input_type: None }
}

fn t(s: &str) -> Value {
    Value::Text(s.into())
}

struct Outcome {
    jobs: Vec<geoconveyor::jobs::Job>,
    log: String,
    outputs: Vec<String>,
}

fn run(be: &mut Backend, name: &str, c: Case) -> Result<Outcome, String> {
    let spec = specs().into_iter().find(|s| s.key == c.key).unwrap();
    let mut tab = TabState::new(spec);
    tab.rebuild(be);
    if let Some(ty) = c.input_type {
        tab.set_input_type(ty);
    }
    if let Some(s) = c.sub {
        tab.sub = s.into();
        tab.ensure_form(be);
    }
    for (k, v) in c.opts {
        assert!(tab.form_mut().unwrap().set(k, v), "no such option {k}");
    }
    if let Some(p) = c.patterns { tab.patterns = p.into(); }
    tab.together = c.together;
    if let Some(e) = c.ext { tab.ext = e.into(); }
    if let Some(e) = c.suffix { tab.suffix = e.into(); }
    if let Some(e) = c.name { tab.name = e.into(); }
    if let Some(d) = c.dem { tab.dem_mode = d.into(); }
    if let Some(d) = c.color { tab.color = d; }
    if let Some(d) = c.crs { tab.crs = d.into(); }
    if let Some(d) = c.coords { tab.coords = d.into(); }
    tab.sync(be);

    let out = out_dir(name);
    let g = Globals {
        in_dir: if c.dir.is_empty() { String::new() } else { data().join(c.dir).to_string_lossy().into() },
        out_dir: out.to_string_lossy().into(),
        recursive: true, keep_tree: true, parallel: 2,
    };
    tab.validate(&g, &be.exe)?;
    let files = if tab.uses_files() {
        scan_files(&g.in_dir, &tab.patterns, true, if tab.needs_out() { &g.out_dir } else { "" })
    } else { vec![] };
    let mut jobs = make_jobs(&tab, &g, &files, false)?;
    let handle = start(be.exe.clone(), jobs.clone(), 2, !tab.overwrite, Arc::new(|| {}));
    let mut started = vec![None::<Instant>; jobs.len()];
    let mut log = String::new();
    while let Ok(ev) = handle.rx.recv() {
        if let Event::Log(l) = &ev { log.push_str(l); log.push('\n'); }
        apply_event(&mut jobs, &mut started, &ev);
        if matches!(ev, Event::AllDone) { break; }
    }
    let mut outputs: Vec<String> = walkdir::WalkDir::new(&out).into_iter().filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(&out).unwrap().to_string_lossy().into()).collect();
    outputs.sort();
    Ok(Outcome { jobs, log, outputs })
}

fn backend() -> Option<Backend> {
    let exe = find_terrio("");
    if exe.is_empty() { eprintln!("terrio not installed: skipping"); return None; }
    Some(Backend::new(&exe))
}

fn all_done(o: &Outcome) -> bool {
    !o.jobs.is_empty() && o.jobs.iter().all(|j| j.status == Status::Done)
}

fn no_partials(o: &Outcome) -> bool {
    o.outputs.iter().all(|p| !p.contains(".partial-"))
}

#[test]
fn every_command_builds_and_runs() {
    let Some(mut be) = backend() else { return };
    let colors = data().join("colors.txt").to_string_lossy().into_owned();
    let mut results: HashMap<&str, Outcome> = HashMap::new();

    let mut cases: Vec<(&str, Case)> = vec![];
    {
        let mut c = case("info", "rast", "*.tif"); c.opts = vec![("--json", Value::Flag(true))];
        cases.push(("info", c));
    }
    {   // defaults make this a COG with DEFLATE
        let mut c = case("convert", "rast", "*.tif"); c.opts = vec![];
        cases.push(("convert", c));
    }
    {
        let mut c = case("translate", "rast", "tile_*.tif");
        c.opts = vec![("--outsize", Value::Multi(vec!["50%".into(), "50%".into()])), ("--resampling", t("average"))];
        cases.push(("translate", c));
    }
    {
        let mut c = case("warp", "rast", "tile_*.tif");
        c.opts = vec![("--t-srs", t("EPSG:4326")), ("--resampling", t("bilinear"))];
        cases.push(("warp_each", c));
    }
    {
        let mut c = case("warp", "rast", "tile_*.tif"); c.together = true; c.name = Some("warped_all");
        c.opts = vec![("--t-srs", t("EPSG:4326"))];
        cases.push(("warp_together", c));
    }
    {
        let mut c = case("mosaic", "dem", "*.tif"); c.name = Some("dem_mosaic");
        c.opts = vec![("--resolution", t("highest"))];
        cases.push(("mosaic", c));
    }
    {
        let mut c = case("mosaic", "dem", "*.tif"); c.name = Some("dem_virtual"); c.ext = Some(".vrt");
        cases.push(("mosaic_vrt", c));
    }
    {
        let mut c = case("rasterize", "vec", "*.geojson");
        c.opts = vec![("--tr", Value::Multi(vec!["0.0001".into(), "0.0001".into()])), ("--burn", t("1"))];
        cases.push(("rasterize", c));
    }
    cases.push(("polygonize", { let mut c = case("polygonize", "rast", "cover.tif"); c.opts = vec![("--8", Value::Flag(true))]; c }));
    cases.push(("dem_hillshade", { let mut c = case("dem", "dem", "*.tif"); c.opts = vec![("--multidirectional", Value::Flag(true))]; c.dem = Some("hillshade"); c.suffix = Some("_hillshade"); c }));
    cases.push(("dem_slope", { let mut c = case("dem", "dem", "dem_a.tif"); c.opts = vec![("--percent", Value::Flag(true))]; c.dem = Some("slope"); c.suffix = Some("_slope"); c }));
    cases.push(("dem_color", { let mut c = case("dem", "dem", "dem_a.tif"); c.opts = vec![("--alpha", Value::Flag(true))]; c.dem = Some("color-relief"); c.color = Some(colors.clone()); c.suffix = Some("_color"); c }));
    cases.push(("calc_each", { let mut c = case("calc", "dem", "*.tif"); c.opts = vec![("--calc", t("A * 2")), ("--type", t("Float32"))]; c }));
    cases.push(("calc_together", { let mut c = case("calc", "dem", "*.tif"); c.opts = vec![("--calc", t("A + B")), ("--type", t("Float32"))]; c.together = true; c.name = Some("sum"); c }));
    cases.push(("proximity", { let mut c = case("proximity", "rast", "cover.tif"); c.opts = vec![("--values", t("1")), ("--distunits", t("GEO"))]; c }));
    cases.push(("sieve", { let mut c = case("sieve", "rast", "cover.tif"); c.opts = vec![("--st", t("5"))]; c }));
    cases.push(("grid", { let mut c = case("grid", "pc", "*.las"); c.opts = vec![("--tr", t("5")), ("--stat", t("max"))]; c }));
    cases.push(("filter_smrf", { let mut c = case("filter", "pc", "*.las"); c.sub = Some("smrf"); c }));
    cases.push(("filter_range", { let mut c = case("filter", "pc", "*.las"); c.sub = Some("range"); c.opts = vec![("--limits", t("Classification[2:3]"))]; c }));
    cases.push(("filter_expression", { let mut c = case("filter", "pc", "cloud_a.las"); c.sub = Some("expression"); c.opts = vec![("--expression", t("Z > 10"))]; c.ext = Some(".copc.laz"); c }));
    cases.push(("filter_merge", { let mut c = case("filter", "pc", "*.las"); c.sub = Some("merge"); c.name = Some("both"); c.ext = Some(".copc.laz"); c }));
    cases.push(("filter_splitter", { let mut c = case("filter", "pc", "cloud_a.las"); c.sub = Some("splitter"); c.opts = vec![("--length", t("50"))]; c.suffix = Some("_tile#"); c.ext = Some(".copc.laz"); c }));
    cases.push(("crs", { let mut c = case("crs", "", ""); c.opts = vec![("--to", t("EPSG:32616"))]; c.crs = Some("EPSG:4326"); c.coords = Some("-86.9 20.8"); c }));
    cases.push(("formats", case("formats", "", "")));

    for (name, c) in cases {
        let o = run(&mut be, name, c).unwrap_or_else(|e| panic!("[{name}] blocked: {e}"));
        assert!(all_done(&o), "[{name}] jobs: {:?}\nlog: {}", o.jobs.iter().map(|j| (&j.label, j.status.name(), &j.message)).collect::<Vec<_>>(), o.log);
        assert!(no_partials(&o), "[{name}] leftovers: {:?}", o.outputs);
        results.insert(name, o);
    }

    // spot checks on what came out
    assert!(results["info"].log.contains("\"driver\""));
    assert_eq!(results["convert"].jobs.len(), 4);
    assert!(results["convert"].outputs.contains(&"tile_0_converted.tif".to_string()));
    assert_eq!(results["warp_together"].outputs, vec!["warped_all.tif"]);
    assert_eq!(results["mosaic_vrt"].outputs, vec!["dem_virtual.vrt"]);
    assert_eq!(results["filter_merge"].outputs, vec!["both.copc.laz"]);
    assert_eq!(results["filter_splitter"].outputs.len(), 9, "{:?}", results["filter_splitter"].outputs);
    assert!(results["filter_splitter"].outputs.iter().all(|o| o.starts_with("cloud_a_tile") && o.ends_with(".copc.laz")));
    assert!(results["crs"].log.contains("510406"));
    assert!(results["formats"].log.lines().count() > 10);
    assert!(results["dem_color"].outputs.contains(&"dem_a_color.tif".to_string()));
}

#[test]
fn corrupt_file_fails_with_terrios_message_and_leaves_nothing() {
    let Some(mut be) = backend() else { return };
    let mut c = case("convert", "", "corrupt.tif");
    c.dir = ".";
    let o = run(&mut be, "corrupt", Case { dir: "", ..c }).err();
    // dir "" means no input folder: validation blocks it
    assert!(o.is_some());

    let c = case("convert", ".", "corrupt.tif");
    let o = run(&mut be, "corrupt2", c).unwrap();
    assert_eq!(o.jobs.len(), 1);
    assert_eq!(o.jobs[0].status, Status::Failed);
    assert!(!o.jobs[0].message.is_empty());
    assert!(o.outputs.is_empty(), "{:?}", o.outputs);
}

#[test]
fn missing_required_option_is_blocked() {
    let Some(mut be) = backend() else { return };
    let e = run(&mut be, "calc_blocked", case("calc", "dem", "*.tif")).err().unwrap();
    assert!(e.contains("--calc"), "{e}");
}

#[test]
fn skip_vs_overwrite_and_input_protection() {
    let Some(mut be) = backend() else { return };
    let o = run(&mut be, "skip", case("sieve", "rast", "cover.tif")).unwrap();
    assert!(all_done(&o));
    // second run into the same folder: skip
    let spec = specs().into_iter().find(|s| s.key == "sieve").unwrap();
    let mut tab = TabState::new(spec);
    tab.rebuild(&mut be);
    tab.patterns = "cover.tif".into();
    let g = Globals { in_dir: data().join("rast").to_string_lossy().into(), out_dir: out_dir("skip").parent().unwrap().join("skip").to_string_lossy().into(), ..Globals::default() };
    // out_dir() wiped it; recreate the output, then run again
    let first = make_jobs(&tab, &g, &scan_files(&g.in_dir, &tab.patterns, true, ""), false).unwrap();
    std::fs::create_dir_all(first[0].dst.as_ref().unwrap().parent().unwrap()).unwrap();
    std::fs::write(first[0].dst.as_ref().unwrap(), b"x").unwrap();
    let mut jobs = first.clone();
    let h = start(be.exe.clone(), jobs.clone(), 1, true, Arc::new(|| {}));
    let mut st = vec![None; jobs.len()];
    while let Ok(ev) = h.rx.recv() { apply_event(&mut jobs, &mut st, &ev); if matches!(ev, Event::AllDone) { break; } }
    assert_eq!(jobs[0].status, Status::Skipped);
    assert_eq!(std::fs::read(first[0].dst.as_ref().unwrap()).unwrap(), b"x");

    // writing next to the input with an empty suffix would overwrite it: refused
    let mut tab2 = TabState::new(specs().into_iter().find(|s| s.key == "sieve").unwrap());
    tab2.rebuild(&mut be);
    tab2.suffix = String::new();
    let g2 = Globals { in_dir: data().join("rast").to_string_lossy().into(), out_dir: data().join("rast").to_string_lossy().into(), ..Globals::default() };
    let err = make_jobs(&tab2, &g2, &[data().join("rast/cover.tif")], false).err().unwrap();
    assert!(err.contains("overwrite the input"), "{err}");
}

#[test]
fn cancel_stops_pending_jobs_and_cleans_up() {
    let Some(mut be) = backend() else { return };
    let spec = specs().into_iter().find(|s| s.key == "convert").unwrap();
    let mut tab = TabState::new(spec);
    tab.rebuild(&mut be);
    let out = out_dir("cancel");
    let g = Globals { in_dir: data().join("rast").to_string_lossy().into(), out_dir: out.to_string_lossy().into(), ..Globals::default() };
    let files = scan_files(&g.in_dir, "*.tif", true, "");
    let mut jobs = make_jobs(&tab, &g, &files, false).unwrap();
    let h = start(be.exe.clone(), jobs.clone(), 1, false, Arc::new(|| {}));
    h.cancel(); // before the first job starts, or right after it
    let mut st = vec![None; jobs.len()];
    while let Ok(ev) = h.rx.recv() { apply_event(&mut jobs, &mut st, &ev); if matches!(ev, Event::AllDone) { break; } }
    assert!(jobs.iter().all(|j| j.status.finished()), "{:?}", jobs.iter().map(|j| j.status.name()).collect::<Vec<_>>());
    assert!(jobs.iter().any(|j| j.status == Status::Cancelled));
    let leftovers: Vec<_> = walkdir::WalkDir::new(&out).into_iter().filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(".partial-")).collect();
    assert!(leftovers.is_empty());
}

#[test]
fn field_counts_match_the_help_and_every_filter_builds() {
    let Some(mut be) = backend() else { return };
    for spec in specs() {
        let mut tab = TabState::new(spec);
        tab.rebuild(&mut be);
        if spec.subs {
            assert_eq!(tab.subs.len(), 20, "filter subcommands: {:?}", tab.subs);
            for s in tab.subs.clone() {
                tab.sub = s.clone();
                tab.ensure_form(&mut be);
                assert!(tab.form().is_some(), "no form for filter {s}");
            }
        } else {
            let hp = be.help.get(&be.exe.clone(), &[spec.key.to_string()]).unwrap().clone();
            let skipped = geoconveyor::specs::skipped(spec.key).len();
            assert_eq!(tab.form().unwrap().fields.len(), hp.options.len() - skipped, "{}", spec.key);
        }
    }
}

#[test]
fn preview_and_settings_roundtrip() {
    let Some(mut be) = backend() else { return };
    let spec = specs().into_iter().find(|s| s.key == "convert").unwrap();
    let mut tab = TabState::new(spec);
    tab.rebuild(&mut be);
    let g = Globals { in_dir: "/in".into(), out_dir: "/out".into(), ..Globals::default() };
    let p = preview(&tab, &g, &be.exe);
    assert!(p.starts_with("terrio convert"), "{p}");
    assert!(p.contains("--format COG") && p.contains("--compress DEFLATE") && p.contains("--tile-size 512"), "{p}");
    assert!(p.ends_with("/out/input_converted.tif"), "{p}");

    // picking a format changes the extension; saved state restores everything
    tab.form_mut().unwrap().set("--format", t("AAIGrid"));
    tab.sync(&be);
    assert_eq!(tab.ext, ".asc");
    tab.form_mut().unwrap().set("--co", t("A=1\nB=2"));
    let saved = tab.state();
    let json = serde_json::to_string(&saved).unwrap();
    let back: geoconveyor::model::TabSettings = serde_json::from_str(&json).unwrap();
    let mut tab2 = TabState::new(spec);
    tab2.rebuild(&mut be);
    tab2.apply(&back, &mut be);
    assert_eq!(tab2.ext, ".asc");
    let p2 = preview(&tab2, &g, &be.exe);
    assert!(p2.contains("--co A=1 --co B=2"), "{p2}");
    assert_eq!(p, preview(&{ let mut t3 = TabState::new(spec); t3.rebuild(&mut be); t3 }, &g, &be.exe));
}

#[test]
fn convert_input_type_shows_only_its_options_and_runs() {
    let Some(mut be) = backend() else { return };
    let spec = specs().into_iter().find(|s| s.key == "convert").unwrap();
    let mut tab = TabState::new(spec);
    tab.rebuild(&mut be);
    let g = Globals { in_dir: "/in".into(), out_dir: "/out".into(), ..Globals::default() };

    // Image (default): raster options in the command, vector/point-cloud ones not
    tab.form_mut().unwrap().set("--where", t("pop > 1"));
    tab.form_mut().unwrap().set("--bounds", Value::Multi(vec!["0".into(), "0".into(), "1".into(), "1".into()]));
    let p = preview(&tab, &g, &be.exe);
    assert!(p.contains("--compress DEFLATE") && !p.contains("--where") && !p.contains("--bounds"), "{p}");
    let visible = |tab: &TabState| -> Vec<String> {
        let f = tab.form().unwrap();
        f.fields.iter().filter(|x| f.is_visible(x)).map(|x| x.opt.long.clone()).collect()
    };
    let img = visible(&tab);
    assert!(img.contains(&"--tile-size".to_string()) && !img.contains(&"--where".to_string()));

    // Vector: its own options and patterns, no raster-only ones; typed values return when switching back
    tab.set_input_type(InputType::Vector);
    assert_eq!(tab.ext, ".gpkg");
    assert!(tab.patterns.contains("*.geojson"));
    let vec_opts = visible(&tab);
    assert!(vec_opts.contains(&"--where".to_string()) && vec_opts.contains(&"--layer".to_string()));
    assert!(!vec_opts.contains(&"--compress".to_string()) && !vec_opts.contains(&"--bounds".to_string()));
    let p = preview(&tab, &g, &be.exe);
    assert!(p.contains("--format GPKG") && p.contains("--where") && !p.contains("--compress") && !p.contains("--bounds"), "{p}");

    // Point cloud
    tab.set_input_type(InputType::PointCloud);
    assert_eq!(tab.ext, ".copc.laz");
    let pc = visible(&tab);
    assert!(pc.contains(&"--bounds".to_string()) && !pc.contains(&"--where".to_string()) && !pc.contains(&"--tile-size".to_string()));

    // All: everything visible, everything that is set goes into the command
    tab.set_input_type(InputType::All);
    assert_eq!(visible(&tab).len(), tab.form().unwrap().fields.len());
    let p = preview(&tab, &g, &be.exe);
    assert!(p.contains("--where") && p.contains("--bounds"), "{p}");
}

#[test]
fn convert_runs_for_vector_and_point_cloud_inputs() {
    let Some(mut be) = backend() else { return };
    let o = run(&mut be, "convert_vector", Case { opts: vec![], ..case_typed("vec", InputType::Vector) }).unwrap();
    assert!(all_done(&o), "{:?} {}", o.jobs.iter().map(|j| (&j.label, &j.message)).collect::<Vec<_>>(), o.log);
    assert_eq!(o.outputs, vec!["polys_converted.gpkg"]);
    let o = run(&mut be, "convert_cloud", case_typed("pc", InputType::PointCloud)).unwrap();
    assert!(all_done(&o), "{:?} {}", o.jobs.iter().map(|j| (&j.label, &j.message)).collect::<Vec<_>>(), o.log);
    assert!(o.outputs.iter().all(|p| p.ends_with("_converted.copc.laz")) && o.outputs.len() == 2, "{:?}", o.outputs);
}

fn case_typed(dir: &str, ty: InputType) -> Case<'_> {
    let mut c = case("convert", dir, "");
    c.patterns = None;
    c.input_type = Some(ty);
    c
}

#[test]
fn settings_order_and_migration() {
    use geoconveyor::model::{normalize_order, Settings};
    let all = ["info", "convert", "warp"];
    assert_eq!(normalize_order(&["warp".into(), "bogus".into(), "info".into(), "warp".into()], &all), vec!["warp", "info", "convert"]);
    assert_eq!(normalize_order(&[], &all), vec!["info", "convert", "warp"]);

    // an old single-session file becomes one session; terrio's path is carried to the top level
    let old = r#"{"globals":{"in_dir":"/a","out_dir":"/b","terrio":"/usr/bin/terrio","recursive":false,"keep_tree":true,"parallel":3},"tab":1,"tabs":{"convert":{"patterns":"*.tif","ext":".tif","suffix":"_x","name":"n","forms":{}}}}"#;
    let s = Settings::from_json(old);
    assert_eq!(s.terrio, "/usr/bin/terrio");
    assert_eq!(s.sessions.len(), 1);
    let g = &s.sessions[0].globals;
    assert_eq!((g.in_dir.as_str(), g.out_dir.as_str(), g.recursive, g.parallel), ("/a", "/b", false, 3));
    assert_eq!(s.sessions[0].tabs["convert"].suffix, "_x");
    // a settings file the program cannot parse must not crash it
    assert!(Settings::from_json("not json").sessions.is_empty());
}
