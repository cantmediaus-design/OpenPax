use std::sync::atomic::{AtomicBool, Ordering};

use crate::{Error, Result};

/// Receives `(label, done, total)` reports.
type Sink<'a> = Box<dyn FnMut(&str, u64, u64) + 'a>;

/// Progress reporting and cancellation for long-running work (downloads, extraction, remixing).
pub struct Progress<'a> {
    cancel: Option<&'a AtomicBool>,
    sink: Sink<'a>,
}

impl<'a> Progress<'a> {
    /// Reports through `sink(label, done, total)` and stops with [`Error::Cancelled`]
    /// once `cancel` is set.
    pub fn new(cancel: &'a AtomicBool, sink: impl FnMut(&str, u64, u64) + 'a) -> Self {
        Self {
            cancel: Some(cancel),
            sink: Box::new(sink),
        }
    }

    /// Reports nowhere and never cancels.
    pub fn none() -> Progress<'static> {
        Progress {
            cancel: None,
            sink: Box::new(|_, _, _| {}),
        }
    }

    /// Reports a step and returns `Err(Cancelled)` if cancellation was requested.
    pub fn step(&mut self, label: &str, done: u64, total: u64) -> Result<()> {
        if self.cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(Error::Cancelled);
        }
        (self.sink)(label, done, total);
        Ok(())
    }
}
