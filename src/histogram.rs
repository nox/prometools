//! Extensions for [`prometheus_client::metrics::histogram::Histogram`].

use prometheus_client::metrics::histogram::Histogram;
use std::time::{Duration, Instant};

/// Extension trait for [`prometheus_client::metrics::histogram::Histogram`].
pub trait HistogramExt {
    /// Returns a timer whose value will be recorded in this histogram.
    fn start_timer(&self) -> HistogramTimer;
}

impl HistogramExt for Histogram {
    fn start_timer(&self) -> HistogramTimer {
        HistogramTimer {
            histogram: self.clone(),
            observed: false,
            start: Instant::now(),
        }
    }
}

/// Timer to measure and record the duration of an event.
///
/// This timer can be stopped and observed at most once, either automatically
/// (when it goes out of scope) or manually. Alternatively, it can be manually
/// stopped and discarded in order to not record its value.
pub struct HistogramTimer {
    histogram: Histogram,
    observed: bool,
    start: Instant,
}

impl HistogramTimer {
    /// Observe, record and return timer duration (in seconds).
    ///
    /// It observes and returns a floating-point number for seconds elapsed since
    /// the timer started, recording that value to the attached histogram.
    pub fn stop_and_record(self) -> f64 {
        let mut timer = self;
        timer.observe(true)
    }

    /// Observe and return timer duration (in seconds).
    ///
    /// It returns a floating-point number of seconds elapsed since the timer started,
    /// without recording to any histogram.
    pub fn stop_and_discard(self) -> f64 {
        let mut timer = self;
        timer.observe(false)
    }

    fn observe(&mut self, record: bool) -> f64 {
        let v = duration_to_seconds(Instant::now().saturating_duration_since(self.start));
        self.observed = true;
        if record {
            self.histogram.observe(v);
        }
        v
    }
}

impl Drop for HistogramTimer {
    fn drop(&mut self) {
        if !self.observed {
            self.observe(true);
        }
    }
}

#[inline]
fn duration_to_seconds(d: Duration) -> f64 {
    let nanos = f64::from(d.subsec_nanos()) / 1e9;
    d.as_secs() as f64 + nanos
}
