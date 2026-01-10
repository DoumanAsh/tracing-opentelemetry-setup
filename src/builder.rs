//! Opentelemetry setup module

use core::{fmt, time};
use std::borrow::Cow;

use opentelemetry_sdk::error::OTelSdkError;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;

#[cfg(feature = "grpc")]
fn create_metadata_map(headers: &[(String, String)]) -> tonic::metadata::MetadataMap {
    use tonic::metadata::{MetadataMap, MetadataKey};

    let mut result = MetadataMap::with_capacity(headers.len());

    for (key, value) in headers.iter() {
        let meta_key = match MetadataKey::from_bytes(key.as_bytes()) {
            Ok(meta) => meta,
            Err(error) => panic!("Header '{key}' is not valid ASCII value: {error}"),
        };
        match value.parse() {
            Ok(value) => {
                result.append(meta_key, value);
            }
            Err(error) => panic!("Header '{key}' has invalid value: {error}"),
        }
    }

    result
}

#[cfg(not(feature = "grpc"))]
#[cold]
#[inline(never)]
fn missing_grpc_feature() -> ! {
    panic!("Attempt to use 'grpc' when corresponding feature is not enabled")
}

#[cfg(not(feature = "http"))]
#[cold]
#[inline(never)]
fn missing_http_feature() -> ! {
    panic!("Attempt to use 'http' when corresponding feature is not enabled")
}

///Opentelemetry attributes that can be put to be exported along side all records
#[derive(Clone)]
#[repr(transparent)]
pub struct Attributes(opentelemetry_sdk::Resource);

impl Attributes {
    #[inline]
    ///Starts Attributes builder
    pub fn builder() -> AttributesBuilder {
        AttributesBuilder::new()
    }
}

///[Attributes] builder
pub struct AttributesBuilder {
    inner: opentelemetry_sdk::resource::ResourceBuilder
}

impl AttributesBuilder {
    #[inline]
    ///Creates new builder
    pub fn new() -> Self {
        Self {
            inner: opentelemetry_sdk::resource::Resource::builder()
        }
    }

    #[inline]
    ///Specifies `key` attribute with provided `value`
    ///
    ///`value` is always `opentelemetry::Value` and there is no guarantee about its stability
    pub fn with_attr(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<opentelemetry::Value>) -> Self {
        self.inner = self.inner.with_attribute(opentelemetry::KeyValue::new(key.into(), value.into()));
        self
    }

    #[inline]
    ///Finalize builder
    pub fn finish(self) -> Attributes {
        Attributes(self.inner.build())
    }
}

#[derive(Default)]
///[Otlp] Shutdown error
pub struct ShutdownError {
    logs: Option<OTelSdkError>,
    trace: Option<OTelSdkError>,
    #[cfg(any(feature = "metrics", feature = "tracing-metrics"))]
    metrics: Option<OTelSdkError>
}

impl fmt::Debug for ShutdownError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut fmt = fmt.debug_struct("OtlpShutdownError");

        if let Some(logs) = self.logs.as_ref() {
            fmt.field("logs", logs);
        }

        if let Some(trace) = self.trace.as_ref() {
            fmt.field("trace", trace);
        }

        #[cfg(any(feature = "metrics", feature = "tracing-metrics"))]
        if let Some(metrics) = self.metrics.as_ref() {
            fmt.field("metrics", metrics);
        }

        fmt.finish()
    }
}

impl fmt::Display for ShutdownError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str("Failed to shutdown Otlp:")?;

        if let Some(logs) = self.logs.as_ref() {
            fmt.write_fmt(format_args!(" logs={logs}"))?
        }

        if let Some(trace) = self.trace.as_ref() {
            fmt.write_fmt(format_args!(" trace={trace}"))?
        }

        #[cfg(any(feature = "metrics", feature = "tracing-metrics"))]
        if let Some(metrics) = self.metrics.as_ref() {
            fmt.write_fmt(format_args!(" metrics={metrics}"))?
        }

        Ok(())
    }
}

impl std::error::Error for ShutdownError {}

///Opentelemetry integration wrapper
///
///It contains references to all exporters which allows it to shutdown on demand or on `Drop`
pub struct Otlp {
    logs: Option<SdkLoggerProvider>,
    trace: Option<SdkTracerProvider>,
    #[cfg(any(feature = "metrics", feature = "tracing-metrics"))]
    metrics: Option<opentelemetry_sdk::metrics::SdkMeterProvider>
}

impl Otlp {
    #[inline]
    const fn new() -> Self {
        Self {
            logs: None,
            trace: None,
            #[cfg(any(feature = "metrics", feature = "tracing-metrics"))]
            metrics: None,
        }
    }

    #[inline]
    ///Starts building Opentelemetry integration
    pub const fn builder(destination: Destination<'_>) -> Builder<'_> {
        Builder::new(destination)
    }

    ///Performs shutdown, limiting it to `limit` for individual components
    ///
    ///If `limit` is zero, then default timeout of `10` seconds is used
    pub fn shutdown(&mut self, mut limit: time::Duration) -> Result<(), ShutdownError> {
        if limit.is_zero() {
            limit = time::Duration::from_secs(10);
        }

        let mut is_error = false;
        let mut errors = ShutdownError::default();
        if let Some(logs) = self.logs.take() {
            if let Err(error) = logs.shutdown_with_timeout(limit) {
                is_error = true;
                errors.logs = Some(error);
            }
        }

        if let Some(trace) = self.trace.take() {
            if let Err(error) = trace.shutdown_with_timeout(limit) {
                is_error = true;
                errors.trace = Some(error);
            }
        }

        #[cfg(any(feature = "metrics", feature = "tracing-metrics"))]
        if let Some(metrics) = self.metrics.take() {
            if let Err(error) =  metrics.shutdown_with_timeout(limit) {
                is_error = true;
                errors.metrics = Some(error);
            }
        }

        if is_error {
            Err(errors)
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "metrics")]
    ///Initializes [metrics](https://crates.io/crates/metrics) global recorder if metrics SDK is set up
    ///
    ///Requires `metrics` feature
    ///
    ///This function can only run once, subsequent calls will have no effect
    pub fn init_metrics_recorder(&self, name: &'static str) {
        use crate::opentelemetry::metrics::MeterProvider;

        if let Some(metrics) = self.metrics.as_ref() {
            let meter = metrics.meter(name);
            let metrics = metrics_opentelemetry::OpenTelemetryMetrics::new(meter);
            let recorder = metrics_opentelemetry::OpenTelemetryRecorder::new(metrics);
            let _ = crate::metrics::set_global_recorder(recorder);
        }
    }

    ///Finishes initializing `tracing_subscriber::registry::Registry` with specified `name` used for tracer
    ///
    ///Cannot be called more than once as `tracing` allows only single global instance
    ///
    ///If feature `tracing-metrics` is enabled, then it shall record metrics via tracing events.
    ///For details refer to its [docs](https://docs.rs/tracing-opentelemetry/latest/tracing_opentelemetry/struct.MetricsLayer.html)
    pub fn init_tracing_subscriber<R: Sync + Send + tracing::Subscriber + tracing_subscriber::layer::SubscriberExt + tracing_subscriber::util::SubscriberInitExt + for<'a> tracing_subscriber::registry::LookupSpan<'a>>(&self, name: impl Into<Cow<'static, str>>, registry: R) {
        use opentelemetry::trace::TracerProvider;
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        #[cfg(feature = "tracing-metrics")]
        macro_rules! init_metrics {
            ($registry:expr) => {
                if let Some(metrics) = self.metrics.as_ref() {
                    let metrics = tracing_opentelemetry::MetricsLayer::new(metrics.clone());
                    $registry.with(metrics).init();
                } else {
                    $registry.init()
                }
            };
        }

        #[cfg(not(feature = "tracing-metrics"))]
        macro_rules! init_metrics {
            ($registry:expr) => {
                $registry.init()
            }
        }

        if let Some(trace) = self.trace.as_ref() {
            let layer = tracing_opentelemetry::OpenTelemetryLayer::new(trace.tracer(name));
            let registry = registry.with(layer);
            if let Some(logs) = self.logs.as_ref() {
                let layer = opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(logs);
                let registry = registry.with(layer);
                init_metrics!(registry)
            } else {
                init_metrics!(registry)
            }
        } else if let Some(logs) = self.logs.as_ref() {
            let layer = opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(logs);
            let registry = registry.with(layer);
            init_metrics!(registry)
        } else {
            init_metrics!(registry)
        }
    }
}

impl Drop for Otlp {
    #[inline(always)]
    fn drop(&mut self) {
        let _ = self.shutdown(time::Duration::ZERO);
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
///Possible communication protocol
pub enum Protocol {
    ///GRPC
    Grpc,
    ///HTTP
    HttpBinary,
    ///HTTP
    HttpJson,
}

impl Protocol {
    #[allow(unused)]
    #[inline]
    const fn into_otel(self) -> opentelemetry_otlp::Protocol {
        match self {
            Self::Grpc => opentelemetry_otlp::Protocol::Grpc,
            Self::HttpJson => opentelemetry_otlp::Protocol::HttpJson,
            Self::HttpBinary => opentelemetry_otlp::Protocol::HttpBinary,
        }

    }
}

///Describes destination configuration
pub struct Destination<'a> {
    ///protocol to use
    pub protocol: Protocol,
    ///destination URL
    ///
    ///When `Http*` protocol is used, assumes `<url>/metrics` | `<url>/logs` | `<url>/traces` to be available
    pub url: Cow<'a, str>,
}

///Opentelemetry integration builder
pub struct Builder<'a> {
    destination: Destination<'a>,
    otlp: Otlp,
    headers: Vec<(String, String)>,
    timeout: time::Duration,
    compression: bool,
}

///Trace configuration
pub struct TraceSettings {
    ///Sample ratio to apply to all traces (unless parent overrides it)
    pub sample_rate: f64,
}

#[cfg(feature = "metrics")]
///Metrics settings
pub struct MetricsSettings {
    temporality: opentelemetry_sdk::metrics::Temporality,
}

#[cfg(feature = "metrics")]
impl MetricsSettings {
    #[inline]
    ///Creates new instance with following defaults:
    ///
    ///- temporality is Cumulative
    pub const fn new() -> Self {
        Self {
            temporality: opentelemetry_sdk::metrics::Temporality::Cumulative
        }
    }

    #[inline]
    ///Metrics are measured in cycles
    pub const fn with_delta(mut self) -> Self {
        self.temporality = opentelemetry_sdk::metrics::Temporality::Delta;
        self
    }

    #[inline]
    ///Optimizes delta measured metrics for low memory usage
    pub const fn with_low_memory(mut self) -> Self {
        self.temporality = opentelemetry_sdk::metrics::Temporality::LowMemory;
        self
    }
}

impl<'a> Builder<'a> {
    #[inline]
    ///Starts building Opentelemetry integration
    pub const fn new(destination: Destination<'a>) -> Self {
        Self {
            destination,
            otlp: Otlp::new(),
            headers: Vec::new(),
            timeout: time::Duration::from_secs(5),
            compression: true,
        }
    }

    #[inline]
    ///Specify whether to use compression by all OTLP exporters
    ///
    ///Defaults to `true`
    ///
    ///Has no effect if relevant `*-compression` are enabled
    pub fn with_compression(mut self, compression: bool) -> Self {
        self.compression = compression;
        self
    }

    #[inline]
    ///Specify common timeout to be used by all OTLP exporters
    ///
    ///Defaults to 5 seconds
    pub fn with_timeout(mut self, timeout: time::Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[inline]
    ///Specify common header to be included for all OTLP destinations
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    ///Enables `logs` exporter with provided `attrs` annotating logs
    ///
    ///Panics if called more than once
    pub fn with_logs(self, _attrs: Option<&Attributes>) -> Self {
        if self.otlp.logs.is_some() {
            panic!("Logs is already initialized")
        }

        let _exporter = match self.destination.protocol {
            #[cfg(feature = "grpc")]
            Protocol::Grpc => {
                use opentelemetry_otlp::{WithTonicConfig, WithExportConfig};
                let mut builder = opentelemetry_otlp::LogExporter::builder().with_tonic().with_endpoint(self.destination.url.clone().into_owned());

                if cfg!(feature = "grpc-compression") && self.compression {
                    builder = builder.with_compression(opentelemetry_otlp::Compression::Gzip)
                }

                if !self.headers.is_empty() {
                    let headers = create_metadata_map(&self.headers);
                    builder = builder.with_metadata(headers);
                }


                builder.with_timeout(self.timeout).build().expect("Failed to initialize logs grpc exporter")
            },
            #[cfg(not(feature = "grpc"))]
            Protocol::Grpc => missing_grpc_feature(),
            #[cfg(feature = "http")]
            http => {
                use opentelemetry_otlp::{WithHttpConfig, WithExportConfig};
                let url = format!("{}/logs", self.destination.url.trim_end_matches('/'));
                let mut builder = opentelemetry_otlp::LogExporter::builder().with_http().with_protocol(http.into_otel()).with_endpoint(url);

                if cfg!(feature = "http-compression") && self.compression {
                    builder = builder.with_compression(opentelemetry_otlp::Compression::Gzip)
                }

                if !self.headers.is_empty() {
                    let headers = self.headers.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
                    builder = builder.with_headers(headers);
                }
                builder.with_timeout(self.timeout).build().expect("Failed to initialize logs http exporter")
            },
            #[cfg(not(feature = "http"))]
            _ => missing_http_feature(),
        };

        #[cfg(any(feature = "grpc", feature = "http"))]
        {
            let mut this = self;
            let mut builder = SdkLoggerProvider::builder();
            if let Some(attrs) = _attrs {
                builder = builder.with_resource(attrs.0.clone());
            }

            this.otlp.logs = Some(builder.with_batch_exporter(_exporter).build());
            return this;
        }
    }

    ///Enables `trace` exporter with provided `attrs` annotating traces
    ///
    ///Panics if called more than once
    pub fn with_trace(self, _attrs: Option<&Attributes>, _settings: TraceSettings) -> Self {
        if self.otlp.trace.is_some() {
            panic!("Trace is already initialized")
        }

        let _exporter = match self.destination.protocol {
            #[cfg(feature = "grpc")]
            Protocol::Grpc => {
                use opentelemetry_otlp::{WithTonicConfig, WithExportConfig};
                let mut builder = opentelemetry_otlp::SpanExporter::builder().with_tonic().with_endpoint(self.destination.url.clone().into_owned());

                if cfg!(feature = "grpc-compression") && self.compression {
                    builder = builder.with_compression(opentelemetry_otlp::Compression::Gzip)
                }

                if !self.headers.is_empty() {
                    let headers = create_metadata_map(&self.headers);
                    builder = builder.with_metadata(headers);
                }


                builder.with_timeout(self.timeout).build().expect("Failed to initialize trace grpc exporter")
            },
            #[cfg(not(feature = "grpc"))]
            Protocol::Grpc => missing_grpc_feature(),
            #[cfg(feature = "http")]
            http => {
                use opentelemetry_otlp::{WithHttpConfig, WithExportConfig};
                let url = format!("{}/traces", self.destination.url.trim_end_matches('/'));
                let mut builder = opentelemetry_otlp::SpanExporter::builder().with_http().with_protocol(http.into_otel()).with_endpoint(url);

                if cfg!(feature = "http-compression") && self.compression {
                    builder = builder.with_compression(opentelemetry_otlp::Compression::Gzip)
                }

                if !self.headers.is_empty() {
                    let headers = self.headers.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
                    builder = builder.with_headers(headers);
                }
                builder.with_timeout(self.timeout).build().expect("Failed to initialize trace http exporter")
            },
            #[cfg(not(feature = "http"))]
            _ => missing_http_feature(),
        };

        #[cfg(any(feature = "grpc", feature = "http"))]
        {
            let mut this = self;
            let sample_rate = _settings.sample_rate.clamp(0.0, 1.0);
            let sampler = opentelemetry_sdk::trace::Sampler::ParentBased(Box::new(opentelemetry_sdk::trace::Sampler::TraceIdRatioBased(sample_rate)));
            let mut builder = SdkTracerProvider::builder().with_sampler(sampler).with_id_generator(opentelemetry_sdk::trace::RandomIdGenerator::default());
            if let Some(attrs) = _attrs {
                builder = builder.with_resource(attrs.0.clone());
            }

            this.otlp.trace = Some(builder.with_batch_exporter(_exporter).build());
            return this;
        }
    }

    #[cfg(feature = "metrics")]
    ///Enables `metrics` exporter with provided `attrs` annotating metrics
    ///
    ///Panics if called more than once
    pub fn with_metrics(self, _attrs: Option<&Attributes>, _settings: MetricsSettings) -> Self {
        if self.otlp.metrics.is_some() {
            panic!("Trace is already initialized")
        }

        let _exporter = match self.destination.protocol {
            #[cfg(feature = "grpc")]
            Protocol::Grpc => {
                use opentelemetry_otlp::{WithTonicConfig, WithExportConfig};
                let mut builder = opentelemetry_otlp::MetricExporter::builder().with_tonic().with_endpoint(self.destination.url.clone().into_owned()).with_temporality(_settings.temporality);

                if cfg!(feature = "grpc-compression") && self.compression {
                    builder = builder.with_compression(opentelemetry_otlp::Compression::Gzip)
                }

                if !self.headers.is_empty() {
                    let headers = create_metadata_map(&self.headers);
                    builder = builder.with_metadata(headers);
                }


                builder.with_timeout(self.timeout).build().expect("Failed to initialize metrics grpc exporter")
            },
            #[cfg(not(feature = "grpc"))]
            Protocol::Grpc => missing_grpc_feature(),
            #[cfg(feature = "http")]
            http => {
                use opentelemetry_otlp::{WithHttpConfig, WithExportConfig};
                let url = format!("{}/metrics", self.destination.url.trim_end_matches('/'));
                let mut builder = opentelemetry_otlp::MetricExporter::builder().with_http().with_protocol(http.into_otel()).with_endpoint(url).with_temporality(_settings.temporality);

                if cfg!(feature = "http-compression") && self.compression {
                    builder = builder.with_compression(opentelemetry_otlp::Compression::Gzip)
                }

                if !self.headers.is_empty() {
                    let headers = self.headers.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
                    builder = builder.with_headers(headers);
                }
                builder.with_timeout(self.timeout).build().expect("Failed to initialize metrics http exporter")
            },
            #[cfg(not(feature = "http"))]
            _ => missing_http_feature(),
        };

        #[cfg(any(feature = "grpc", feature = "http"))]
        {
            let mut this = self;
            let mut builder = opentelemetry_sdk::metrics::SdkMeterProvider::builder();
            if let Some(attrs) = _attrs {
                builder = builder.with_resource(attrs.0.clone());
            }

            this.otlp.metrics = Some(builder.with_periodic_exporter(_exporter).build());
            return this;
        }
    }

    #[inline]
    ///Finalizes building otlp integration
    pub fn finish(self) -> Otlp {
        self.otlp
    }
}
