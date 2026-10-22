//! Runs jobs: a pool of worker threads, one terrio process per job.
//!
//! Results are written under a temporary name and renamed when terrio succeeds, so a
//! cancelled or failed file never leaves half an output behind.

use crate::help::command;
use crate::i18n::{t, tf};
use crate::jobs::{path_size, shell_join, Job, Status};
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
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

pub fn remove_path(p: &Path) {
    if p.is_dir() {
        let _ = std::fs::remove_dir_all(p);
    } else if p.exists() {
        let _ = std::fs::remove_file(p);
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
            let (jobs, next, cancel, exe, tx, wake) =
                (jobs.clone(), next.clone(), cancel.clone(), exe.clone(), tx.clone(), wake.clone());
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
        if skip_existing && dst.exists() {
            finish(Status::Skipped, t("output exists"), 0.0, path_size(dst));
            return;
        }
        if let Some(p) = dst.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if job.direct {
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
    let mut killed = false;
    let code = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.code(),
            Ok(None) => {
                if cancel.load(Ordering::SeqCst) && !killed {
                    let _ = child.kill();
                    killed = true;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            Err(_) => break None,
        }
    };
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
                remove_path(dst);
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
        let size = job.dst.as_ref().map(|d| path_size(d)).unwrap_or(0);
        if job.dst.is_none() {
            message = text.lines().next().unwrap_or("").chars().take(120).collect();
        }
        (Status::Done, size)
    } else {
        message = match text.lines().last() {
            Some(l) => l.chars().take(300).collect(),
            None => tf("exit code {}", &[code.map_or("?".to_string(), |c| c.to_string())]),
        };
        (Status::Failed, 0)
    };
    if status != Status::Done && job.dst.is_some() && !job.direct {
        remove_path(tmp.as_ref().unwrap());
    }
    if !text.is_empty() {
        send(tx, wake, Event::Log(text));
    }
    finish(status, message, seconds, size_out);
}

fn pump<R: Read>(r: &mut R, buf: &Arc<Mutex<Vec<u8>>>) {
    let mut chunk = [0u8; 8192];
    while let Ok(n) = r.read(&mut chunk) {
        if n == 0 {
            break;
        }
        buf.lock().unwrap().extend_from_slice(&chunk[..n]);
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
