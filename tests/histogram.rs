use prometheus_client::metrics::histogram::{exponential_buckets, linear_buckets};
use prometools::histogram::{HistogramSnapshot, TimeHistogram};
use std::thread::sleep;
use std::time::Duration;

// macOS CI runners have notoriously large time skew on sleep() system calls,
// so we skip tests that are sensitive to sleep duration on those runners.
// See (for example) https://stackoverflow.com/q/48285535 and
// https://travis-ci.community/t/sleep-functions-are-not-accurate-on-macos/6122
macro_rules! skip_if_mac_runner {
    () => {{
        if cfg!(target_os = "macos") && std::env::var_os("CI").is_some() {
            eprintln!("skipping test on macOS CI runner");
            return;
        }
    }};
}

#[test]
fn histogram() {
    let histogram = TimeHistogram::new(exponential_buckets(1.0, 2.0, 10));

    histogram.observe(Duration::from_secs(1).as_nanos() as u64);
    histogram.observe(Duration::from_secs_f64(1.5).as_nanos() as u64);
    histogram.observe(Duration::from_secs_f64(2.5).as_nanos() as u64);
    histogram.observe(Duration::from_secs_f64(8.5).as_nanos() as u64);
    histogram.observe(Duration::from_secs_f64(0.5).as_nanos() as u64);

    let snapshot = histogram.snapshot();

    assert_eq!(snapshot.sum(), 14.);
    assert_eq!(snapshot.count(), 5);
    assert_eq!(snapshot.buckets()[0].1, 2);
    assert_eq!(snapshot.buckets()[1].1, 1);
    assert_eq!(snapshot.buckets()[4].1, 1);
}

#[test]
fn timer_stop_and_record() {
    let histogram = TimeHistogram::new(linear_buckets(0.01, 0.01, 12));
    let timer = histogram.start_timer();
    let duration = timer.stop_and_record();

    assert_duration(duration, 0);

    assert_eq!(histogram.snapshot().count(), 1);
}

#[test]
fn timer_stop_and_discard() {
    let histogram = TimeHistogram::new(linear_buckets(0.01, 0.01, 12));
    let timer = histogram.start_timer();
    let duration = timer.stop_and_discard();

    assert_duration(duration, 0);

    assert_eq!(histogram.snapshot().count(), 0)
}

#[test]
fn timer_pause_stop() {
    skip_if_mac_runner!();

    let histogram = TimeHistogram::new(linear_buckets(0.01, 0.01, 12));
    let mut timer = histogram.start_timer();

    sleep(Duration::from_millis(10));
    timer.pause();
    sleep(Duration::from_millis(20));

    let duration = timer.stop_and_record();

    assert_duration(duration, 10);
    assert_timer_bucket(duration, &histogram.snapshot());
}

#[test]
fn timer_pause_resume_stop() {
    skip_if_mac_runner!();

    let histogram = TimeHistogram::new(linear_buckets(0.01, 0.01, 12));
    let mut timer = histogram.start_timer();

    sleep(Duration::from_millis(10));
    timer.pause();
    sleep(Duration::from_millis(20));
    timer.resume();
    sleep(Duration::from_millis(40));

    let duration = timer.stop_and_record();

    assert_duration(duration, 50);
    assert_timer_bucket(duration, &histogram.snapshot());
}

#[test]
fn timer_resume_stop() {
    skip_if_mac_runner!();

    let histogram = TimeHistogram::new(linear_buckets(0.01, 0.01, 12));
    let mut timer = histogram.start_timer();

    sleep(Duration::from_millis(10));
    timer.resume();
    sleep(Duration::from_millis(20));

    let duration = timer.stop_and_record();

    assert_duration(duration, 30);
    assert_timer_bucket(duration, &histogram.snapshot());
}

#[test]
fn timer_pause_pause_stop() {
    skip_if_mac_runner!();

    let histogram = TimeHistogram::new(linear_buckets(0.01, 0.01, 12));
    let mut timer = histogram.start_timer();

    sleep(Duration::from_millis(10));
    timer.pause();
    sleep(Duration::from_millis(20));
    timer.pause();
    sleep(Duration::from_millis(40));

    let duration = timer.stop_and_record();

    assert_duration(duration, 10);
    assert_timer_bucket(duration, &histogram.snapshot());
}

#[test]
fn timer_pause_resume_pause_stop() {
    skip_if_mac_runner!();

    let histogram = TimeHistogram::new(linear_buckets(0.01, 0.01, 12));
    let mut timer = histogram.start_timer();

    sleep(Duration::from_millis(10));
    timer.pause();
    sleep(Duration::from_millis(20));
    timer.resume();
    sleep(Duration::from_millis(40));
    timer.pause();
    sleep(Duration::from_millis(80));

    let duration = timer.stop_and_record();

    assert_duration(duration, 10 + 40);
    assert_timer_bucket(duration, &histogram.snapshot());
}

#[test]
fn timer_pause_resume_pause_resume_stop() {
    skip_if_mac_runner!();

    let histogram = TimeHistogram::new(linear_buckets(0.01, 0.01, 20));
    let mut timer = histogram.start_timer();

    sleep(Duration::from_millis(10));
    timer.pause();
    sleep(Duration::from_millis(20));
    timer.resume();
    sleep(Duration::from_millis(40));
    timer.pause();
    sleep(Duration::from_millis(80));
    timer.resume();
    sleep(Duration::from_millis(120));

    let duration = timer.stop_and_record();

    assert_duration(duration, 10 + 40 + 120);
    assert_timer_bucket(duration, &histogram.snapshot());
}

#[test]
fn timer_resume_drop() {
    skip_if_mac_runner!();

    let histogram = TimeHistogram::new(linear_buckets(0.01, 0.01, 12));
    let mut timer = histogram.start_timer();

    sleep(Duration::from_millis(10));
    timer.pause();
    sleep(Duration::from_millis(20));
    timer.resume();
    sleep(Duration::from_millis(40));
    drop(timer);

    // +1 because sleep is not exact
    let duration = Duration::from_millis(10 + 40 + 1);
    assert_timer_bucket(duration, &histogram.snapshot());
}

#[track_caller]
fn assert_duration(duration: Duration, ms: u128) {
    let duration_ms = duration.as_millis();
    let max_ms = ms + 20;

    assert!(
        duration_ms >= ms,
        "duration {duration_ms} should be at least {ms}"
    );
    assert!(
        duration_ms < max_ms,
        "duration {duration_ms} should be at most {max_ms}"
    );
}

#[track_caller]
fn assert_timer_bucket(duration: Duration, snap: &HistogramSnapshot) {
    let seconds = duration.as_secs_f64();
    let buckets = snap.buckets();
    let bucket_idx = buckets
        .iter()
        .position(|&(bound, _)| seconds <= bound)
        .expect("duration should fit within available buckets");

    assert!(0 < bucket_idx && bucket_idx + 1 < buckets.len());
    let window = [
        buckets[bucket_idx - 1].1,
        buckets[bucket_idx].1,
        buckets[bucket_idx + 1].1,
    ];
    assert_eq!(&window, &[0, 1, 0]);
}
