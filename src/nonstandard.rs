//! Metric types that don't follow the OpenTelemetry standard exactly.

use prometheus_client::{
    encoding::text::{Encode, EncodeMetric, Encoder},
    metrics::{
        counter::{Atomic, Counter},
        MetricType, TypedMetric,
    },
};
use std::{
    io,
    ops::{Deref, DerefMut},
    sync::atomic::AtomicU64,
};

/// A wrapper of [`prometheus_client::metrics::counter::Counter`] which does
/// not suffix the name with `_total`.
#[repr(transparent)]
pub struct NonstandardUnsuffixedCounter<N = u64, A = AtomicU64>(pub Counter<N, A>);

impl<N, A> Clone for NonstandardUnsuffixedCounter<N, A> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<N, A: Default> Default for NonstandardUnsuffixedCounter<N, A> {
    fn default() -> Self {
        Self(Counter::default())
    }
}

impl<N, A> Deref for NonstandardUnsuffixedCounter<N, A> {
    type Target = Counter<N, A>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<N, A> DerefMut for NonstandardUnsuffixedCounter<N, A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<N, A> TypedMetric for NonstandardUnsuffixedCounter<N, A> {
    const TYPE: MetricType = MetricType::Counter;
}

impl<N, A> EncodeMetric for NonstandardUnsuffixedCounter<N, A>
where
    N: Encode,
    A: Atomic<N>,
{
    fn encode(&self, mut encoder: Encoder) -> Result<(), io::Error> {
        let mut bucket_encoder = encoder.no_suffix()?;
        let mut value_encoder = bucket_encoder.no_bucket()?;
        let mut exemplar_encoder = value_encoder.encode_value(self.get())?;

        exemplar_encoder.no_exemplar()
    }

    fn metric_type(&self) -> MetricType {
        Self::TYPE
    }
}
