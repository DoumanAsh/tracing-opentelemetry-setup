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

#[cfg(feature = "datadog")]
#[cold]
#[inline(never)]
fn unsupported_datadog_feature() -> ! {
    panic!("Attempt to use 'datadog' while it doesn't support logs functionality")
}

#[cfg(not(feature = "datadog"))]
#[cold]
#[inline(never)]
fn missing_datadog_feature() -> ! {
    panic!("Attempt to use 'datadog' when corresponding feature is not enabled")
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
    ///Datadog agent exporter
    DatadogAgent,
}

impl Protocol {
    #[allow(unused)]
    #[inline]
    const fn into_otel(self) -> opentelemetry_otlp::Protocol {
        match self {
            Self::Grpc => opentelemetry_otlp::Protocol::Grpc,
            Self::HttpJson => opentelemetry_otlp::Protocol::HttpJson,
            Self::HttpBinary => opentelemetry_otlp::Protocol::HttpBinary,
            Self::DatadogAgent => unreachable!(),
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

macro_rules! declare_trace_limits {
    ({$($name:ident,)+}) => {
        struct SpanLimits {
            $(
                $name: u32,
            )+
        }

        impl SpanLimits {
            const DEFAULT: u32 = 128;

            #[inline(always)]
            const fn new() -> Self {
                Self {
                    $(
                        $name: Self::DEFAULT,
                    )+
                }
            }

            #[allow(unused)]
            #[inline(always)]
            fn apply_to(&self, mut builder: opentelemetry_sdk::trace::TracerProviderBuilder) -> opentelemetry_sdk::trace::TracerProviderBuilder {
                $(
                    if self.$name != Self::DEFAULT {
                        builder = builder.$name(self.$name);
                    }
                )+
                builder
            }
        }
    };
}

declare_trace_limits!({
    with_max_events_per_span,
    with_max_attributes_per_span,
    with_max_links_per_span,
    with_max_attributes_per_link,
    with_max_attributes_per_event,
});

#[allow(unused)]
#[derive(Copy, Clone, Debug)]
struct AlwaysOnSampler;

impl opentelemetry_sdk::trace::ShouldSample for AlwaysOnSampler {
    #[inline(always)]
    fn should_sample(&self, parent_context: Option<&opentelemetry::Context>, _: opentelemetry::TraceId, _: &str, _: &opentelemetry::trace::SpanKind, _: &[opentelemetry::KeyValue], _: &[opentelemetry::trace::Link]) -> opentelemetry::trace::SamplingResult {
        use opentelemetry::trace::TraceContextExt;

        opentelemetry::trace::SamplingResult {
            decision: opentelemetry::trace::SamplingDecision::RecordAndSample,
            attributes: Vec::new(),
            trace_state: match parent_context {
                Some(ctx) => ctx.span().span_context().trace_state().clone(),
                None => opentelemetry::trace::TraceState::default(),
            },
        }
    }
}

#[allow(unused)]
#[derive(Copy, Clone, Debug)]
struct AlwaysOffSampler;

impl opentelemetry_sdk::trace::ShouldSample for AlwaysOffSampler {
    #[inline(always)]
    fn should_sample(&self, parent_context: Option<&opentelemetry::Context>, _: opentelemetry::TraceId, _: &str, _: &opentelemetry::trace::SpanKind, _: &[opentelemetry::KeyValue], _: &[opentelemetry::trace::Link]) -> opentelemetry::trace::SamplingResult {
        use opentelemetry::trace::TraceContextExt;

        opentelemetry::trace::SamplingResult {
            decision: opentelemetry::trace::SamplingDecision::Drop,
            attributes: Vec::new(),
            trace_state: match parent_context {
                Some(ctx) => ctx.span().span_context().trace_state().clone(),
                None => opentelemetry::trace::TraceState::default(),
            },
        }
    }
}

///Trace configuration
pub struct TraceSettings {
    #[allow(unused)]
    ///Sample ratio to apply to all traces (unless parent overrides it)
    sample_rate: f64,
    #[allow(unused)]
    limits: SpanLimits,
    #[allow(unused)]
    respect_parent: bool,
}

macro_rules! set_trace_limit {
    ($limits:expr, $name:ident) => {
        $limits.$name = $name;
    };
}

impl TraceSettings {
    ///Creates new instance with provided `sample_rate`
    pub const fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            limits: SpanLimits::new(),
            respect_parent: true,
        }
    }

    ///Specifies whether to respect parent trace's sampling decision. Defaults to `true`
    pub const fn with_respect_parent_sampling(mut self, value: bool) -> Self {
        self.respect_parent = value;
        self
    }

    ///The max events that can be added to a Span. Defaults to 128
    pub const fn with_max_events_per_span(mut self, with_max_events_per_span: u32) -> Self {
        set_trace_limit!(self.limits, with_max_events_per_span);
        self
    }

    ///The max attributes that can be added to a Span.
    pub const fn with_max_attributes_per_span(mut self, with_max_attributes_per_span: u32) -> Self {
        set_trace_limit!(self.limits, with_max_attributes_per_span);
        self
    }

    ///The max links that can be added to a Span. Defaults to 128
    pub const fn with_max_links_per_span(mut self, with_max_links_per_span: u32) -> Self {
        set_trace_limit!(self.limits, with_max_links_per_span);
        self
    }

    ///The max attributes that can be added into an Event. Defaults to 128
    pub const fn with_max_attributes_per_event(mut self, with_max_attributes_per_event: u32) -> Self {
        set_trace_limit!(self.limits, with_max_attributes_per_event);
        self
    }

    ///The max attributes that can be added into a Link. Defaults to 128
    pub const fn with_max_attributes_per_link(mut self, with_max_attributes_per_link: u32) -> Self {
        set_trace_limit!(self.limits, with_max_attributes_per_link);
        self
    }
}

#[cfg(any(feature = "metrics", feature = "tracing-metrics"))]
///Metrics settings
pub struct MetricsSettings {
    temporality: opentelemetry_sdk::metrics::Temporality,
}

#[cfg(any(feature = "metrics", feature = "tracing-metrics"))]
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


                let exporter = builder.with_timeout(self.timeout).build().expect("Failed to initialize logs grpc exporter");
                opentelemetry_sdk::logs::BatchLogProcessor::builder(exporter).build()
            },
            #[cfg(not(feature = "grpc"))]
            Protocol::Grpc => missing_grpc_feature(),

            #[cfg(feature = "datadog")]
            Protocol::DatadogAgent => unsupported_datadog_feature(),
            #[cfg(not(feature = "datadog"))]
            Protocol::DatadogAgent => missing_datadog_feature(),

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
                let exporter = builder.with_timeout(self.timeout).build().expect("Failed to initialize logs http exporter");
                opentelemetry_sdk::logs::BatchLogProcessor::builder(exporter).build()
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

            this.otlp.logs = Some(builder.with_log_processor(_exporter).build());
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

        let _batch_config = opentelemetry_sdk::trace::BatchConfigBuilder::default().build();
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


                let exporter = builder.with_timeout(self.timeout).build().expect("Failed to initialize trace grpc exporter");
                opentelemetry_sdk::trace::BatchSpanProcessor::new(exporter, _batch_config)
            },
            #[cfg(not(feature = "grpc"))]
            Protocol::Grpc => missing_grpc_feature(),

            #[cfg(feature = "datadog")]
            Protocol::DatadogAgent => {
                let exporter = opentelemetry_datadog::new_pipeline().with_agent_endpoint(self.destination.url.clone()).build_exporter().expect("Failed to initialize datadog exporter");
                opentelemetry_sdk::trace::BatchSpanProcessor::new(exporter, _batch_config)
            },
            #[cfg(not(feature = "datadog"))]
            Protocol::DatadogAgent => missing_datadog_feature(),

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
                let exporter = builder.with_timeout(self.timeout).build().expect("Failed to initialize trace http exporter");
                opentelemetry_sdk::trace::BatchSpanProcessor::new(exporter, _batch_config)
            },
            #[cfg(not(feature = "http"))]
            _ => missing_http_feature(),
        };

        #[cfg(any(feature = "grpc", feature = "http", feature = "datadog"))]
        {
            let mut this = self;
            let sample_rate = _settings.sample_rate.clamp(0.0, 1.0);
            let mut builder = SdkTracerProvider::builder().with_id_generator(opentelemetry_sdk::trace::RandomIdGenerator::default());
            if _settings.respect_parent {
                let sampler = opentelemetry_sdk::trace::Sampler::ParentBased(Box::new(opentelemetry_sdk::trace::Sampler::TraceIdRatioBased(sample_rate)));
                builder = builder.with_sampler(sampler);
            } else {
                if sample_rate == 0.0 {
                    builder = builder.with_sampler(AlwaysOffSampler);
                } else if sample_rate == 1.0 {
                    builder = builder.with_sampler(AlwaysOnSampler);
                } else {
                    let sampler = opentelemetry_sdk::trace::Sampler::TraceIdRatioBased(sample_rate);
                    builder = builder.with_sampler(sampler);
                }
            }
            builder = _settings.limits.apply_to(builder);
            if let Some(attrs) = _attrs {
                builder = builder.with_resource(attrs.0.clone());
            }

            this.otlp.trace = Some(builder.with_span_processor(_exporter).build());
            return this;
        }
    }

    #[cfg(any(feature = "metrics", feature = "tracing-metrics"))]
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

            #[cfg(feature = "datadog")]
            Protocol::DatadogAgent => unsupported_datadog_feature(),
            #[cfg(not(feature = "datadog"))]
            Protocol::DatadogAgent => missing_datadog_feature(),

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
