//! Serde bridge.

use crate::nonstandard::InfoGauge as InnerInfoGauge;
use parking_lot::{MappedRwLockReadGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use prometheus_client::{
    encoding::text::{Encode, EncodeMetric, Encoder},
    metrics::{family::MetricConstructor, MetricType, TypedMetric},
};
use serde::ser::Serialize;
use std::{collections::HashMap, fmt, hash::Hash, io, sync::Arc};

mod error;
mod str;
mod top;
mod value;

/// A version of [`prometheus_client::metrics::family::Family`] which
/// encodes its labels with [`Serialize`] instead of [`Encode`].
///
/// #### Examples
///
/// Basic usage:
///
/// ```rust
/// # use prometheus_client::{
/// #     encoding::text::encode,
/// #     registry::Registry,
/// # };
/// # use prometools::{nonstandard::NonstandardUnsuffixedCounter, serde::Family};
/// # use serde::Serialize;
/// #
/// #[derive(Clone, Eq, Hash, PartialEq, Serialize)]
/// struct Labels {
///     method: Method,
///     host: String,
/// }
///
/// #[derive(Clone, Eq, Hash, PartialEq, Serialize)]
/// enum Method {
///     #[serde(rename = "GET")]
///     Get,
/// }
///
/// let family = <Family<Labels, NonstandardUnsuffixedCounter>>::default();
/// let mut registry = Registry::with_prefix("http");
///
/// registry.register(
///     "incoming_requests",
///     "Number of requests per method and per host",
///     family.clone(),
/// );
///
/// family
///     .get_or_create(&Labels {
///         method: Method::Get,
///         host: "techworkerscoalition.org".to_string(),
///     })
///     .inc();
///
/// let mut serialized = String::new();
///
/// // SAFETY: We know prometheus-client only writes UTF-8 slices.
/// unsafe {
///     encode(&mut serialized.as_mut_vec(), &registry).unwrap();
/// }
///
/// assert_eq!(
///     serialized,
///     concat!(
///         "# HELP http_incoming_requests Number of requests per method and per host.\n",
///         "# TYPE http_incoming_requests counter\n",
///         "http_incoming_requests{method=\"GET\",host=\"techworkerscoalition.org\"} 1\n",
///         "# EOF\n",
///     ),
/// );
/// ```
#[derive(Debug)]
pub struct Family<S, M, C = fn() -> M> {
    /// Map of labels to metric instances.
    metrics: Arc<RwLock<HashMap<Bridge<S>, M>>>,
    /// Function to construct fresh metric instances.
    constructor: C,
}

impl<S, M, C> Family<S, M, C>
where
    S: Clone + Eq + Hash,
{
    /// Create a metric family using a custom constructor to construct new metrics.
    pub fn new_with_constructor(constructor: C) -> Self {
        Self {
            metrics: Default::default(),
            constructor,
        }
    }
}

impl<S, M> Default for Family<S, M>
where
    S: Clone + Eq + Hash,
    M: Default,
{
    fn default() -> Self {
        Self::new_with_constructor(M::default)
    }
}

impl<S, M, C> Family<S, M, C>
where
    S: Clone + Eq + Hash,
    C: MetricConstructor<M>,
{
    /// Access a metric with the given label set, creating it if one does not yet exist.
    ///
    /// This can deadlock when called while holding a reference to another metric in the
    /// family. Make sure to drop the reference or convert it into an owned value beforehand.
    pub fn get_or_create(&self, label_set: &S) -> MappedRwLockReadGuard<'_, M> {
        let label_set = Bridge::from_ref(label_set);
        if let Ok(m) = RwLockReadGuard::try_map(self.metrics.read(), |map| map.get(label_set)) {
            return m;
        }

        let mut map_write = self.metrics.write();
        map_write
            .entry(label_set.clone())
            .or_insert_with(|| self.constructor.new_metric());

        let map_read = RwLockWriteGuard::downgrade(map_write);
        RwLockReadGuard::map(map_read, |map| {
            // The atomic downgrade ensures no other writer can have remove the metric
            map.get(label_set)
                .expect("metric should exist after creating it")
        })
    }

    /// Remove a label set from the metric family.
    ///
    /// Returns a bool indicating if the label set was present or not.
    pub fn remove(&self, label_set: &S) -> bool {
        let label_set = Bridge::from_ref(label_set);
        self.metrics.write().remove(label_set).is_some()
    }

    /// Clear all label sets from the metric family.
    pub fn clear(&self) {
        self.metrics.write().clear();
    }
}

impl<S, M, C> EncodeMetric for Family<S, M, C>
where
    S: Clone + Eq + Hash + Serialize,
    M: EncodeMetric + TypedMetric,
    C: MetricConstructor<M>,
{
    fn encode(&self, mut encoder: Encoder) -> io::Result<()> {
        let map_read = self.metrics.read();
        for (label_set, m) in map_read.iter() {
            let enc = encoder.with_label_set(label_set);
            m.encode(enc)?;
        }
        Ok(())
    }

    fn metric_type(&self) -> MetricType {
        M::TYPE
    }
}

impl<S, M, C> TypedMetric for Family<S, M, C>
where
    M: TypedMetric,
{
    const TYPE: MetricType = <M as TypedMetric>::TYPE;
}

impl<S, M, C> Clone for Family<S, M, C>
where
    C: Clone,
{
    fn clone(&self) -> Self {
        Self {
            metrics: Arc::clone(&self.metrics),
            constructor: self.constructor.clone(),
        }
    }
}

/// A wrapper around [`crate::nonstandard::InfoGauge`] which
/// encodes its labels with [`Serialize`] instead of [`Encode`].
///
/// #### Examples
///
/// Basic usage:
///
/// ```rust
/// # use prometheus_client::{
/// #     encoding::text::encode,
/// #     registry::Registry,
/// # };
/// # use prometools::serde::InfoGauge;
/// # use serde::Serialize;
/// #
/// #[derive(Serialize)]
/// struct BuildInfo {
///     version: &'static str,
///     mode: Mode,
/// }
///
/// #[derive(Serialize)]
/// #[serde(rename_all = "lowercase")]
/// enum Mode {
///     Debug,
///     Release,
/// }
///
/// let info = InfoGauge::new(BuildInfo {
///     version: "1.2.3",
///     mode: Mode::Debug,
/// });
///
/// let mut registry = Registry::default();
///
/// registry.register(
///     "build_info",
///     "Build information",
///     info,
/// );
///
/// let mut serialized = String::new();
///
/// // SAFETY: We know prometheus-client only writes UTF-8 slices.
/// unsafe {
///     encode(&mut serialized.as_mut_vec(), &registry).unwrap();
/// }
///
/// assert_eq!(
///     serialized,
///     concat!(
///         "# HELP build_info Build information.\n",
///         "# TYPE build_info gauge\n",
///         "build_info{version=\"1.2.3\",mode=\"debug\"} 1\n",
///         "# EOF\n",
///     ),
/// );
/// ```
#[derive(Debug)]
pub struct InfoGauge<S> {
    inner: InnerInfoGauge<Bridge<S>>,
}

impl<S> InfoGauge<S>
where
    S: Serialize,
{
    pub fn new(label_set: S) -> Self {
        Self {
            inner: InnerInfoGauge::new(Bridge(label_set)),
        }
    }
}

impl<S> EncodeMetric for InfoGauge<S>
where
    S: Serialize,
{
    fn encode(&self, encoder: Encoder) -> io::Result<()> {
        self.inner.encode(encoder)
    }

    fn metric_type(&self) -> MetricType {
        Self::TYPE
    }
}

impl<S> TypedMetric for InfoGauge<S>
where
    S: Serialize,
{
    const TYPE: MetricType = <InnerInfoGauge<S> as TypedMetric>::TYPE;
}

#[derive(Clone, Eq, Hash, PartialEq)]
#[repr(transparent)]
struct Bridge<S>(S);

impl<S> Bridge<S> {
    fn from_ref(label_set: &S) -> &Self {
        // SAFETY: `Self` is a transparent newtype wrapper.
        unsafe { &*(label_set as *const S as *const Bridge<S>) }
    }
}

impl<S> Encode for Bridge<S>
where
    S: Serialize,
{
    fn encode(&self, writer: &mut dyn io::Write) -> Result<(), std::io::Error> {
        self.0
            .serialize(top::serializer(str::Writer::new(writer)))?;

        Ok(())
    }
}

impl<S> fmt::Debug for Bridge<S>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
