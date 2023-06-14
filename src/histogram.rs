//! A faster, lock-free histogram for tracking timing data.
//!
//! This is based on the implementation for [`prometheus_client::metrics::histogram::Histogram`],
//! with several changes made to eliminate the need for locks.

use std::time::{Duration, Instant};

use prometheus_client::encoding::text::{Encode, EncodeMetric, Encoder};
use prometheus_client::metrics::exemplar::Exemplar;
use prometheus_client::metrics::{MetricType, TypedMetric};
use std::collections::HashMap;
use std::iter::once;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A faster, lock-free histogram for tracking time.
#[derive(Debug)]
pub struct TimeHistogram {
    inner: Arc<Inner>,
}

/// Timer to measure and record the duration of an event.
///
/// This timer can be stopped and observed at most once, either automatically
/// (when it goes out of scope) or manually. Alternatively, it can be manually
/// stopped and discarded in order to not record its value.
pub struct HistogramTimer {
    histogram: TimeHistogram,
    observed: bool,
    start: Instant,
}

#[derive(Debug)]
struct Inner {
    sum: AtomicU64,
    count: AtomicU64,
    buckets: Vec<(f64, AtomicU64)>,
}

impl HistogramTimer {
    /// Observe, record and return timer duration (in seconds).
    ///
    /// It observes and returns a floating-point number for seconds elapsed since
    /// the timer started, recording that value to the attached histogram.
    pub fn stop_and_record(self) -> Duration {
        let mut timer = self;
        timer.observe(true)
    }

    /// Observe and return timer duration (in seconds).
    ///
    /// It returns a floating-point number of seconds elapsed since the timer started,
    /// without recording to any histogram.
    pub fn stop_and_discard(self) -> Duration {
        let mut timer = self;
        timer.observe(false)
    }

    fn observe(&mut self, record: bool) -> Duration {
        let elapsed = Instant::now().saturating_duration_since(self.start);

        self.observed = true;
        if record {
            self.histogram.observe(elapsed.as_nanos() as u64);
        }

        elapsed
    }
}

impl Drop for HistogramTimer {
    fn drop(&mut self) {
        if !self.observed {
            self.observe(true);
        }
    }
}

impl Clone for TimeHistogram {
    fn clone(&self) -> Self {
        TimeHistogram {
            inner: self.inner.clone(),
        }
    }
}

impl TimeHistogram {
    pub fn new(buckets: impl Iterator<Item = f64>) -> Self {
        Self {
            inner: Arc::new(Inner {
                sum: Default::default(),
                count: Default::default(),
                buckets: buckets
                    .into_iter()
                    .chain(once(f64::MAX))
                    .map(|upper_bound| (upper_bound, AtomicU64::new(0)))
                    .collect(),
            }),
        }
    }

    pub fn start_timer(&self) -> HistogramTimer {
        HistogramTimer {
            histogram: self.clone(),
            observed: false,
            start: Instant::now(),
        }
    }

    pub fn observe(&self, nanos: u64) {
        self.observe_and_bucket(nanos);
    }

    fn observe_and_bucket(&self, v: u64) -> Option<usize> {
        self.inner.sum.fetch_add(v, Ordering::Relaxed);
        self.inner.count.fetch_add(1, Ordering::Relaxed);

        let first_bucket = self
            .inner
            .buckets
            .iter()
            .enumerate()
            .find(|(_i, (upper_bound, _value))| upper_bound >= &(v as f64 * 1E-9));

        match first_bucket {
            Some((i, (_upper_bound, value))) => {
                value.fetch_add(1, Ordering::Relaxed);
                Some(i)
            }
            None => None,
        }
    }

    fn get(&self) -> (f64, u64, Vec<(f64, u64)>) {
        let sum = seconds(self.inner.sum.load(Ordering::Relaxed));
        let count = self.inner.count.load(Ordering::Relaxed);
        let buckets = self
            .inner
            .buckets
            .iter()
            .map(|(k, v)| (*k, v.load(Ordering::Relaxed)))
            .collect();
        (sum, count, buckets)
    }
}

impl TypedMetric for TimeHistogram {
    const TYPE: MetricType = MetricType::Histogram;
}

#[inline(always)]
fn seconds(val: u64) -> f64 {
    (val as f64) * 1E-9
}

fn encode_histogram_with_maybe_exemplars<S: Encode>(
    sum: f64,
    count: u64,
    buckets: &[(f64, u64)],
    exemplars: Option<&HashMap<usize, Exemplar<S, f64>>>,
    mut encoder: Encoder,
) -> Result<(), std::io::Error> {
    encoder
        .encode_suffix("sum")?
        .no_bucket()?
        .encode_value(sum)?
        .no_exemplar()?;
    encoder
        .encode_suffix("count")?
        .no_bucket()?
        .encode_value(count)?
        .no_exemplar()?;

    let mut cummulative = 0;
    for (i, (upper_bound, count)) in buckets.iter().enumerate() {
        cummulative += count;
        let mut bucket_encoder = encoder.encode_suffix("bucket")?;
        let mut value_encoder = bucket_encoder.encode_bucket(*upper_bound)?;
        let mut exemplar_encoder = value_encoder.encode_value(cummulative)?;

        match exemplars.and_then(|es| es.get(&i)) {
            Some(exemplar) => exemplar_encoder.encode_exemplar(exemplar)?,
            None => exemplar_encoder.no_exemplar()?,
        }
    }

    Ok(())
}

impl EncodeMetric for TimeHistogram {
    fn encode(&self, encoder: Encoder) -> Result<(), std::io::Error> {
        let (sum, count, buckets) = self.get();
        // TODO: Would be better to use never type instead of `()`.
        encode_histogram_with_maybe_exemplars::<()>(sum, count, &buckets, None, encoder)
    }

    fn metric_type(&self) -> MetricType {
        Self::TYPE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prometheus_client::metrics::histogram::exponential_buckets;
    use std::time::Duration;

    #[test]
    fn histogram() {
        let histogram = TimeHistogram::new(exponential_buckets(1.0, 2.0, 10));
        histogram.observe(Duration::from_secs(1).as_nanos() as u64);
        histogram.observe(Duration::from_secs_f64(1.5).as_nanos() as u64);
        histogram.observe(Duration::from_secs_f64(2.5).as_nanos() as u64);
        histogram.observe(Duration::from_secs_f64(8.5).as_nanos() as u64);
        histogram.observe(Duration::from_secs_f64(0.5).as_nanos() as u64);

        let (sum, count, buckets) = histogram.get();

        assert_eq!(14., sum);
        assert_eq!(5, count);
        assert_eq!(2, buckets[0].1);
        assert_eq!(1, buckets[1].1);
        assert_eq!(1, buckets[4].1);
    }
}
