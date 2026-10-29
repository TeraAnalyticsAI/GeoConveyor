//! Runs jobs: a pool of worker threads, one terrio process per job.
//!
//! Results are written under a temporary name and renamed when terrio succeeds, so a
//! cancelled or failed file never leaves half an output behind.

use crate::help::{command, LaunchProblem};
use crate::i18n::{t, tf};
use crate::jobs::{output_exists, output_size, remove_output, remove_path, shell_join, Job, Status};
use std::io::Read;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub enum Event {
    Started { i: usize },
    Log(String),
    Finished { i: usize, status: Status, message: String, seconds: f64, size_out: u64 },
    AllDone,
}

pub struct RunHandle {
    pub rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}

impl RunHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// Live terrio processes, so quitting the GUI can terminate them instead of leaving them
/// running after the window closes.
type ChildSlot = Arc<Mutex<Option<Child>>>;

fn live_children() -> &'static Mutex<Vec<ChildSlot>> {
    static LIVE: OnceLock<Mutex<Vec<ChildSlot>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(Vec::new()))
}

fn register_child(child: Child) -> ChildSlot {
    let slot = Arc::new(Mutex::new(Some(child)));
    live_children().lock().unwrap().push(slot.clone());
    slot
}

fn unregister_child(slot: &ChildSlot) {
    live_children().lock().unwrap().retain(|s| !Arc::ptr_eq(s, slot));
}

/// Terminate every terrio process started by this app. Called when the window closes.
pub fn kill_all() {
    let slots: Vec<ChildSlot> = live_children().lock().unwrap().clone();
    for slot in slots {
        if let Ok(mut guard) = slot.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

/// Start the batch on background threads. `wake` is called after every event
/// (the GUI uses it to repaint).
pub fn start(exe: String, jobs: Vec<Job>, parallel: usize, skip_existing: bool, wake: Arc<dyn Fn() + Send + Sync>) -> RunHandle {
    let (tx, rx) = channel::<Event>();
    let cancel = Arc::new(AtomicBool::new(false));
    let handle = RunHandle { rx, cancel: cancel.clone() };
    let jobs = Arc::new(jobs);
    let next = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        let workers = parallel.max(1).min(jobs.len().max(1));
        let mut threads = vec![];
        for _ in 0..workers {
            let (jobs, next, cancel, exe, tx, wake) = (jobs.clone(), next.clone(), cancel.clone(), exe.clone(), tx.clone(), wake.clone());
            threads.push(std::thread::spawn(move || loop {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= jobs.len() {
                    break;
                }
                run_job(&exe, i, &jobs[i], skip_existing, &cancel, &tx, &wake);
            }));
        }
        for t in threads {
            let _ = t.join();
        }
        let _ = tx.send(Event::AllDone);
        wake();
    });
    handle
}

fn send(tx: &Sender<Event>, wake: &Arc<dyn Fn() + Send + Sync>, e: Event) {
    let _ = tx.send(e);
    wake();
}

fn run_job(exe: &str, i: usize, job: &Job, skip_existing: bool, cancel: &Arc<AtomicBool>, tx: &Sender<Event>, wake: &Arc<dyn Fn() + Send + Sync>) {
    let finish = |status: Status, message: String, seconds: f64, size_out: u64| {
        send(tx, wake, Event::Finished { i, status, message, seconds, size_out });
    };
    let mut out = String::new();
    let tmp = job.tmp();
    if let Some(dst) = &job.dst {
        if skip_existing && output_exists(dst) {
            finish(Status::Skipped, t("output exists"), 0.0, output_size(dst));
            return;
        }
        if let Some(p) = dst.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if job.direct {
            if !skip_existing {
                // EPT datasets and splitter tile sets are written straight to their final names.
                // Clear the previous result first so stale tiles cannot mix with the new run.
                remove_output(dst);
                if let Some(p) = dst.parent() {
                    let _ = std::fs::create_dir_all(p);
                }
            }
            out = dst.to_string_lossy().into_owned();
        } else {
            let t = tmp.clone().unwrap();
            remove_path(&t);
            out = t.to_string_lossy().into_owned();
        }
    }
    let args = job.argv(&out);
    let mut display = vec![exe.to_string()];
    display.extend(args.iter().cloned());
    if cfg!(windows) && shell_join(&display).len() > 30000 {
        finish(Status::Failed, t("command line too long for Windows; use fewer files"), 0.0, 0);
        return;
    }

    send(tx, wake, Event::Started { i });
    send(tx, wake, Event::Log(format!("$ {}", shell_join(&display))));
    let t0 = Instant::now();

    let mut child = match command(exe).args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => {
            finish(Status::Failed, tf("Could not start {}: {}", &[exe.to_string(), e.to_string()]), 0.0, 0);
            return;
        }
    };
    // Read both pipes on their own threads so a chatty process can never block.
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let mut readers = vec![];
    if let Some(mut so) = child.stdout.take() {
        let b = buf.clone();
        readers.push(std::thread::spawn(move || pump(&mut so, &b)));
    }
    if let Some(mut se) = child.stderr.take() {
        let b = buf.clone();
        readers.push(std::thread::spawn(move || pump(&mut se, &b)));
    }
    let slot = register_child(child);
    let mut killed = false;
    let code = loop {
        let waiting = {
            let mut guard = slot.lock().unwrap();
            match guard.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(st)) => Some(Ok(st.code())),
                    Ok(None) => {
                        if cancel.load(Ordering::SeqCst) && !killed {
                            let _ = child.kill();
                            killed = true;
                        }
                        None
                    }
                    Err(e) => Some(Err(e)),
                },
                None => Some(Ok(None)),
            }
        };
        match waiting {
            Some(Ok(code)) => break code,
            Some(Err(_)) => break None,
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let child = slot.lock().unwrap().take();
    if let Some(mut child) = child {
        if killed || code.is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
    unregister_child(&slot);
    for r in readers {
        let _ = r.join();
    }
    let seconds = t0.elapsed().as_secs_f64();
    let text = String::from_utf8_lossy(&buf.lock().unwrap()).trim().to_string();
    let mut ok = code == Some(0) && !killed;
    let mut message = String::new();
    let mut text = text;

    if ok {
        if let (Some(dst), false) = (&job.dst, job.direct) {
            let tp = tmp.clone().unwrap();
            if tp.exists() {
                // std::fs::rename replaces an existing file atomically on Unix and Windows.
                // Only a directory destination needs removing first.
                if dst.is_dir() {
                    remove_path(dst);
                }
                if let Err(e) = std::fs::rename(&tp, dst) {
                    ok = false;
                    text = e.to_string();
                }
            } else {
                ok = false;
                text = t("terrio finished but wrote no output");
            }
        }
    }
    let (status, size_out) = if killed && !ok {
        (Status::Cancelled, 0)
    } else if ok {
        let size = job.output_size();
        if job.dst.is_none() {
            message = text.lines().next().unwrap_or("").chars().take(120).collect();
        }
        (Status::Done, size)
    } else {
        message = match (LaunchProblem::from_code(code), text.lines().last()) {
            (Some(p), _) => p.message(),
            (None, Some(l)) => l.chars().take(300).collect(),
            (None, None) => tf("exit code {}", &[code.map_or("?".to_string(), |c| c.to_string())]),
        };
        (Status::Failed, 0)
    };
    if status != Status::Done && job.dst.is_some() {
        if job.direct {
            job.remove_output();
        } else {
            remove_path(tmp.as_ref().unwrap());
        }
    }
    if !text.is_empty() {
        send(tx, wake, Event::Log(text));
    }
    finish(status, message, seconds, size_out);
}

fn pump<R: Read>(r: &mut R, buf: &Arc<Mutex<Vec<u8>>>) {
    // Keep the tail only: a runaway process must not grow the GUI's memory without bound.
    const MAX: usize = 1 << 20;
    let mut chunk = [0u8; 8192];
    while let Ok(n) = r.read(&mut chunk) {
        if n == 0 {
            break;
        }
        let mut b = buf.lock().unwrap();
        b.extend_from_slice(&chunk[..n]);
        if b.len() > MAX {
            let cut = b.len() - MAX;
            b.drain(..cut);
        }
    }
}

/// Apply an event to the job list (shared by the GUI and the tests).
pub fn apply_event(jobs: &mut [Job], started: &mut [Option<Instant>], ev: &Event) {
    match ev {
        Event::Started { i } => {
            jobs[*i].status = Status::Running;
            started[*i] = Some(Instant::now());
        }
        Event::Finished { i, status, message, seconds, size_out } => {
            let j = &mut jobs[*i];
            j.status = *status;
            j.message = message.clone();
            j.seconds = *seconds;
            j.size_out = *size_out;
            started[*i] = None;
        }
        Event::AllDone => {
            for j in jobs.iter_mut() {
                if j.status == Status::Pending {
                    j.status = Status::Cancelled;
                }
            }
        }
        Event::Log(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn kill_all_stops_a_running_child() {
        let job =
            Job { label: "sleep".into(), pre: vec!["30".into()], dst: None, direct: false, size_in: 0, status: Status::Pending, seconds: 0.0, size_out: 0, message: String::new() };
        let h = start("sleep".into(), vec![job], 1, false, Arc::new(|| {}));
        // Wait until the child is registered, so kill_all cannot race the worker thread.
        let t0 = Instant::now();
        while live_children().lock().unwrap().is_empty() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        kill_all();
        let mut done = false;
        while let Ok(ev) = h.rx.recv_timeout(Duration::from_secs(5)) {
            if matches!(ev, Event::AllDone) {
                done = true;
                break;
            }
        }
        assert!(done, "the runner did not finish after kill_all");
    }
}
