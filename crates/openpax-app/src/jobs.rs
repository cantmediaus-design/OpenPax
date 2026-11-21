//! Work that runs on a background thread (downloads, extraction, remixing) with progress
//! reporting and cancellation, polled from the UI thread once per frame.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui;
use openpax_core::{Error, Progress};

enum Msg<T> {
    Progress(String, u64, u64),
    Done(Result<T, Error>),
}

/// What a finished job produced.
pub enum Outcome<T> {
    Done(T),
    Cancelled,
    Failed(String),
}

pub struct Job<T> {
    /// What the job is doing, for the progress dialog.
    pub title: String,
    /// The latest step and how far along it is (`total == 0` when unknown).
    pub step: String,
    pub done: u64,
    pub total: u64,
    rx: Receiver<Msg<T>>,
    cancel: Arc<AtomicBool>,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn(
        ctx: &egui::Context,
        title: impl Into<String>,
        work: impl FnOnce(&mut Progress) -> Result<T, Error> + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = Arc::clone(&cancel);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let progress_ctx = ctx.clone();
            let mut progress = Progress::new(&thread_cancel, move |label, done, total| {
                let _ = progress_tx.send(Msg::Progress(label.to_owned(), done, total));
                progress_ctx.request_repaint();
            });
            let result = work(&mut progress);
            let _ = tx.send(Msg::Done(result));
            ctx.request_repaint();
        });
        Self {
            title: title.into(),
            step: String::new(),
            done: 0,
            total: 0,
            rx,
            cancel,
        }
    }

    /// Takes in progress reports; returns the outcome once the job has finished.
    pub fn poll(&mut self) -> Option<Outcome<T>> {
        loop {
            match self.rx.try_recv() {
                Ok(Msg::Progress(step, done, total)) => {
                    (self.step, self.done, self.total) = (step, done, total);
                }
                Ok(Msg::Done(Ok(value))) => return Some(Outcome::Done(value)),
                Ok(Msg::Done(Err(Error::Cancelled))) => return Some(Outcome::Cancelled),
                Ok(Msg::Done(Err(e))) => return Some(Outcome::Failed(e.to_string())),
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    return Some(Outcome::Failed("The task stopped unexpectedly.".into()));
                }
            }
        }
    }

    /// Asks the job to stop at its next checkpoint.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelling(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Progress as a fraction, if the total is known.
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| (self.done as f64 / self.total as f64) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait<T: Send + 'static>(job: &mut Job<T>) -> Outcome<T> {
        loop {
            if let Some(outcome) = job.poll() {
                return outcome;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn reports_progress_and_result() {
        let ctx = egui::Context::default();
        let mut job = Job::spawn(&ctx, "Test", |p| {
            p.step("half", 1, 2)?;
            Ok(42)
        });
        assert!(matches!(wait(&mut job), Outcome::Done(42)));
        assert_eq!((job.step.as_str(), job.done, job.total), ("half", 1, 2));
    }

    #[test]
    fn cancellation_is_not_a_failure() {
        let ctx = egui::Context::default();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let mut job = Job::spawn(&ctx, "Test", move |p| {
            go_rx.recv().unwrap();
            p.step("never", 0, 0)?;
            Ok(())
        });
        job.cancel();
        go_tx.send(()).unwrap();
        assert!(matches!(wait(&mut job), Outcome::Cancelled));
    }
}
