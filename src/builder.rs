//! Opentelemetry setup module

use std::env;
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

#[cfg(all(feature = "datadog", any(feature = "metrics", feature = "tracing-metrics")))]
#[cold]
#[inline(never)]
fn unsupported_datadog_feature() -> ! {
    panic!("Attempt to use 'datadog' while it doesn't support metrics functionality")
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
pub struct Attributes(pub(crate) opentelemetry_sdk::Resource);

impl Attributes {
    #[inline]
    ///Starts Attributes builder
    pub fn builder() -> AttributesBuilder {
        AttributesBuilder::new()
    }

    ///Extracts attributes from environment:
    ///
    ///- `OTEL_SERVICE_NAME` - sets value of `service.name` if present
    ///- `OTEL_RESOURCE_ATTRIBUTES` - Free key/value pair of values to set
    pub fn from_env() -> Option<Attributes> {
        //sdk sets some default values on its own, so check yourself whether you want to build attributes or not
        let mut is_set = false;
        let mut builder = AttributesBuilder::new();

        if let Ok(service_name) = env::var("OTEL_SERVICE_NAME") {
            is_set = true;
            builder = builder.with_service_name(service_name)
        }

        if let Ok(attrs) = env::var("OTEL_RESOURCE_ATTRIBUTES") {
            for key_value in attrs.split(',') {
                let mut key_value_iter = key_value.trim().splitn(2, '=');
                let key = key_value_iter.next().unwrap();
                match key_value_iter.next() {
                    Some(value) => {
                        is_set = true;
                        builder = builder.with_attr(key.to_owned(), value.to_owned());
                    },
                    None => continue
                }
            }
        }

        if is_set {
            Some(builder.finish())
        } else {
            None
        }
    }
}

impl fmt::Debug for Attributes {
    #[inline]
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, fmt)

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
    fn with_service_name(mut self, value: impl Into<opentelemetry::Value>) -> Self {
        self.inner = self.inner.with_service_name(value);
        self
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
    pub const fn builder() -> Builder {
        Builder::new()
    }

    ///Performs flush of the logs
    pub fn flush(&self) -> Result<(), ShutdownError> {
        let mut is_error = false;
        let mut errors = ShutdownError::default();
        if let Some(logs) = self.logs.as_ref() {
            if let Err(error) = logs.force_flush() {
                is_error = true;
                errors.logs = Some(error);
            }
        }

        if let Some(trace) = self.trace.as_ref() {
            if let Err(error) = trace.force_flush() {
                is_error = true;
                errors.trace = Some(error);
            }
        }

        #[cfg(any(feature = "metrics", feature = "tracing-metrics"))]
        if let Some(metrics) = self.metrics.as_ref() {
            if let Err(error) = metrics.force_flush() {
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

    ///Performs shutdown, limiting it to `limit` for individual components
    ///
    ///If `limit` is `None` then defaults to 10 second wait
    pub fn shutdown(&mut self, limit: Option<time::Duration>) -> Result<(), ShutdownError> {
        let limit = match limit {
            Some(limit) => limit,
            None => time::Duration::from_secs(10),
        };

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
}

impl fmt::Debug for Otlp {
    #[inline]
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut fmt = fmt.debug_struct("Otlp");

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

impl Drop for Otlp {
    #[inline(always)]
    fn drop(&mut self) {
        let _ = self.shutdown(None);
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
///Possible communication protocol
pub enum Protocol {
    ///GRPC
    Grpc,
    ///HTTP
    HttpBinary,
    ///HTTP
    HttpJson,
    ///Datadog agent exporter
    ///
    ///In case of traces expects valid network address to send data
    ///
    ///In case of logs it can be `file://<full path>` to specify path to append logs. Otherwise `url` is ignored and `stdout` shall be used.
    ///Note that you're advised to disable attachment of events/logs to the span in this case
    ///
    ///## Error tracking
    ///
    ///For purpose of error tracking, logger printer makes adjustments to the output in following way:
    ///- `service.name` becomes `service`
    ///- `deployment.environment.name` becomes `env`
    ///- `service.version` becomes `version
    ///- Most fields of the record are prefixed with `field.` except:
    ///    - `status` is recorded as it is. If not present, it will be mapped as `ALERT` for `error`, `ERROR` for `warn or `INFO` for info
    ///    - `error.kind` is recorded as it is. If not present, it will be mapped to `error` or `warn` for corresponding log severity
    ///    - `error.stack` is recorded as it is
    ///    - `error.message` is recorded as it is. If not present, it will be recorded for `error` or `warn`
    DatadogAgent,
}

impl Protocol {
    ///Gets default value from available feature set.
    ///
    ///Priority:
    ///- `HttpBinary`
    ///- `Grpc`
    ///- `DatadogAgent`
    ///
    ///Returns `None` if no feature is available
    pub fn select_default() -> Option<Self> {
        if cfg!(feature = "http") {
            Some(Self::HttpBinary)
        } else if cfg!(feature = "grpc") {
            Some(Self::Grpc)
        } else if cfg!(feature = "datadog") {
            Some(Self::DatadogAgent)
        } else {
            None
        }
    }

    ///Attempts to determine protocol from env variable `OTEL_EXPORTER_OTLP_PROTOCOL`
    ///
    ///Possible values:
    ///- `grpc`
    ///- `http/protobuf`
    ///- `http/json`
    ///- `datadog`
    ///
    ///Returns `None` if no env variable is available or doesn't match available protocols
    pub fn from_env() -> Option<Self> {
        match env::var("OTEL_EXPORTER_OTLP_PROTOCOL") {
            Ok(protocol) => {
                if protocol == "grpc" {
                    return Some(Self::Grpc);
                }
                if protocol == "http/protobuf" {
                    return Some(Self::HttpBinary);
                }
                if protocol == "http/json" {
                    return Some(Self::HttpJson);
                }
                if protocol == "datadog" {
                    return Some(Self::DatadogAgent);
                }

                None
            },
            Err(_) => None,
        }
    }

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
    ///When `Http*` protocol is used, destination url shall be constructed as `<url>/metrics` | `<url>/logs` | `<url>/traces`
    pub url: Cow<'a, str>,
    ///Common attributes to deliver to the destination
    ///
    ///It is good practise to include following at least:
    ///- `service.name`
    ///- `service.version`
    pub attributes: Option<&'a Attributes>
}

impl Destination<'_> {
    #[cfg_attr(not(all(feature = "grpc", feature = "http", feature = "datadog")), allow(unused))]
    fn get_service_attrs(&self) -> Option<Attributes> {
        match self.attributes {
            Some(attrs) => Some(attrs.clone()),
            None => Attributes::from_env(),
        }
    }
}

impl Destination<'static> {
    ///Determines destination parameters from environment variables:
    ///
    ///- `OTEL_EXPORTER_OTLP_ENDPOINT` - determines `url`. Defaults to localhost.
    ///- `OTEL_EXPORTER_OTLP_PROTOCOL` - determines `protocol`. Defaults to `http/protobuf` or sole available feature enabled.
    ///- `OTEL_RESOURCE_ATTRIBUTES` - optional key value setter for `attributes`.
    pub fn from_env() -> Self {
        let protocol = Protocol::from_env().or_else(Protocol::select_default).expect("Unable to determine Destination::protocol");
        let url = match env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
            Ok(url) => url.into(),
            Err(_) => match protocol {
                Protocol::Grpc => "http://localhost:4317".into(),
                Protocol::HttpBinary | Protocol::HttpJson => "http://localhost:4318".into(),
                Protocol::DatadogAgent => "http://localhost:8126".into()
            }
        };

        Self {
            protocol,
            url,
            attributes: None,
        }
    }
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
    name: Cow<'static, str>,
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
    ///Creates new instance with provided `sample_rate` with provided `name` for tracer SDK
    pub const fn new(name: Cow<'static, str>, sample_rate: f64) -> Self {
        Self {
            name,
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

#[derive(Copy, Clone)]
///Possible exporter runtimes to be used to export data
pub enum ExportRuntime {
    ///Default, independent thread is spawned by opentelemetry to export data
    Threaded,
    ///Multi-threaded tokio runtime is used to spawn task that exports data
    ///
    ///Requires `rt-tokio` feature
    Tokio,
    ///Single-threaded tokio runtime is used to spawn task that exports data
    ///
    ///Requires `rt-tokio` feature
    TokioCurrentThrad
}

impl ExportRuntime {
    #[cfg(any(feature = "grpc", feature = "http", feature = "datadog"))]
    fn create_logger_exporter<E: opentelemetry_sdk::logs::LogExporter + 'static>(self, exporter: E, config: opentelemetry_sdk::logs::BatchConfig, destination: &Destination<'_>) -> SdkLoggerProvider {
        let mut builder = SdkLoggerProvider::builder();
        if let Some(attrs) = destination.get_service_attrs() {
            builder = builder.with_resource(attrs.0);
        }
        match self {
            Self::Threaded => {
                let exporter = opentelemetry_sdk::logs::BatchLogProcessor::builder(exporter).with_batch_config(config).build();
                builder.with_log_processor(exporter).build()
            },
            #[cfg(feature = "rt-tokio")]
            Self::Tokio => {
                let exporter = opentelemetry_sdk::logs::log_processor_with_async_runtime::BatchLogProcessor::builder(exporter, opentelemetry_sdk::runtime::Tokio).with_batch_config(config).build();
                builder.with_log_processor(exporter).build()
            },
            #[cfg(feature = "rt-tokio")]
            Self::TokioCurrentThrad => {
                let exporter = opentelemetry_sdk::logs::log_processor_with_async_runtime::BatchLogProcessor::builder(exporter, opentelemetry_sdk::runtime::TokioCurrentThread).with_batch_config(config).build();
                builder.with_log_processor(exporter).build()
            },
            #[cfg(not(feature = "rt-tokio"))]
            _ => panic!("rt-tokio feature must be enabled for async runtime"),
        }
    }

    #[cfg(any(feature = "grpc", feature = "http", feature = "datadog"))]
    fn create_tracer_exporter<E: opentelemetry_sdk::trace::SpanExporter + 'static>(self, exporter: E, config: opentelemetry_sdk::trace::BatchConfig, destination: &Destination<'_>, settings: &TraceSettings) -> SdkTracerProvider {
        let sample_rate = settings.sample_rate.clamp(0.0, 1.0);
        let mut builder = SdkTracerProvider::builder().with_id_generator(opentelemetry_sdk::trace::RandomIdGenerator::default());
        if settings.respect_parent {
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
        builder = settings.limits.apply_to(builder);
        if let Some(attrs) = destination.get_service_attrs() {
            builder = builder.with_resource(attrs.0);
        }

        match self {
            Self::Threaded => {
                let exporter = opentelemetry_sdk::trace::BatchSpanProcessor::builder(exporter).with_batch_config(config).build();
                builder.with_span_processor(exporter).build()
            },
            #[cfg(feature = "rt-tokio")]
            Self::Tokio => {
                let exporter = opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor::builder(exporter, opentelemetry_sdk::runtime::Tokio).with_batch_config(config).build();
                builder.with_span_processor(exporter).build()
            },
            #[cfg(feature = "rt-tokio")]
            Self::TokioCurrentThrad => {
                let exporter = opentelemetry_sdk::trace::span_processor_with_async_runtime::BatchSpanProcessor::builder(exporter, opentelemetry_sdk::runtime::TokioCurrentThread).with_batch_config(config).build();
                builder.with_span_processor(exporter).build()
            },
            #[cfg(not(feature = "rt-tokio"))]
            _ => panic!("rt-tokio feature must be enabled for async runtime"),
        }
    }

    #[cfg(all(feature = "metrics", any(feature = "grpc", feature = "http")))]
    fn create_metrics_exporter<E: opentelemetry_sdk::metrics::exporter::PushMetricExporter + 'static>(self, exporter: E, destination: &Destination<'_>, export_interval: time::Duration) -> opentelemetry_sdk::metrics::SdkMeterProvider {
        let mut builder = opentelemetry_sdk::metrics::SdkMeterProvider::builder();
        if let Some(attrs) = destination.get_service_attrs() {
            builder = builder.with_resource(attrs.0.clone());
        }

        match self {
            Self::Threaded => {
                let mut reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter);

                if !export_interval.is_zero() {
                    reader = reader.with_interval(export_interval);
                }

                builder.with_reader(reader.build()).build()
            },
            #[cfg(feature = "rt-tokio")]
            Self::Tokio => {
                let mut reader = opentelemetry_sdk::metrics::periodic_reader_with_async_runtime::PeriodicReader::builder(exporter, opentelemetry_sdk::runtime::Tokio);

                if !export_interval.is_zero() {
                    reader = reader.with_interval(export_interval);
                }

                builder.with_reader(reader.build()).build()
            },
            #[cfg(feature = "rt-tokio")]
            Self::TokioCurrentThrad => {
                let mut reader = opentelemetry_sdk::metrics::periodic_reader_with_async_runtime::PeriodicReader::builder(exporter, opentelemetry_sdk::runtime::TokioCurrentThread);

                if !export_interval.is_zero() {
                    reader = reader.with_interval(export_interval);
                }

                builder.with_reader(reader.build()).build()
            },
            #[cfg(not(feature = "rt-tokio"))]
            _ => panic!("rt-tokio feature must be enabled for async runtime"),
        }
    }
}

///Opentelemetry integration builder
pub struct Builder {
    otlp: Otlp,
    headers: Vec<(String, String)>,
    timeout: time::Duration,
    export_interval: time::Duration,
    queue_size: usize,
    compression: bool,
    #[cfg(feature = "http-ureq")]
    ureq: Option<crate::ureq::HttpClient>,
    runtime: ExportRuntime,
}

impl Builder {
    #[inline]
    ///Starts building Opentelemetry integration
    pub const fn new() -> Self {
        Self {
            otlp: Otlp::new(),
            headers: Vec::new(),
            timeout: time::Duration::from_secs(5),
            export_interval: time::Duration::ZERO,
            queue_size: 0,
            compression: true,
            #[cfg(feature = "http-ureq")]
            ureq: None,
            runtime: ExportRuntime::Threaded,
        }
    }

    #[cfg(feature = "http-ureq")]
    ///Enables usage of simple blocking http client
    ///
    ///Requires `http-ureq` feature enabled
    pub fn with_ureq_http_client(mut self) -> Self {
        self.ureq = Some(crate::ureq::HttpClient::new());
        self
    }

    #[inline]
    ///Specify whether to use compression by all OTLP exporters
    ///
    ///Defaults to `true`
    ///
    ///Has no effect if relevant `*-compression` are _not_ enabled
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
    ///
    ///In addition to that `opentelemetry-otlp` exporter will load headers from env variable `OTEL_EXPORTER_OTLP_HEADERS`
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    #[inline]
    ///Specifies common interval to perform data export for all OTLP exporters
    ///
    ///Unless specified, this interval will be default initialized by opentelemetry-sdk optionally using following environment variables:
    ///- OTEL_BLRP_SCHEDULE_DELAY - Specifies interval between two log batches.
    ///- OTEL_BSP_EXPORT_TIMEOUT - Specifies interval between two trace batches.
    ///- OTEL_METRIC_EXPORT_INTERVAL - Specifies interval between metric exports.
    pub fn with_interval(mut self, interval: time::Duration) -> Self {
        self.export_interval = interval;
        self
    }

    #[inline]
    ///Specifies common size limit on pending queue among batch exporters (doesn't affect metrics)
    ///
    ///Unless specified, this interval will be default initialized by opentelemetry-sdk optionally using following environment variables:
    ///- OTEL_BSP_MAX_QUEUE_SIZE - Specifies queue size of the trace batch exporter. Defaults to 2048.
    ///- OTEL_BLRP_MAX_QUEUE_SIZE - Specifies queue size of the log batch exporter. Defaults to 2048.
    pub fn with_queue_size(mut self, size: usize) -> Self {
        self.queue_size = size;
        self
    }

    ///Specifies export runtime
    pub fn with_runtime(mut self, runtime: ExportRuntime) -> Self {
        self.runtime = runtime;
        self
    }

    #[cfg(feature = "http")]
    fn apply_otel_http_config<T: opentelemetry_otlp::WithHttpConfig>(&self, mut builder: T) -> T {
        if cfg!(feature = "http-compression") && self.compression {
            builder = builder.with_compression(opentelemetry_otlp::Compression::Gzip)
        }

        #[cfg(feature = "http-ureq")]
        if let Some(ureq) = self.ureq.as_ref() {
            builder = builder.with_http_client(ureq.clone());
        }

        if !self.headers.is_empty() {
            let headers = self.headers.iter().map(|(key, value)| (key.clone(), value.clone())).collect();
            builder = builder.with_headers(headers);
        }

        builder
    }

    fn create_logs(&mut self, _destination: &Destination<'_>) -> opentelemetry_sdk::logs::SdkLoggerProvider {
        if self.otlp.logs.is_some() {
            panic!("Logs is already initialized")
        }

        let mut batch_config = opentelemetry_sdk::logs::BatchConfigBuilder::default();
        if !self.export_interval.is_zero() {
            batch_config = batch_config.with_scheduled_delay(self.export_interval);
        }
        if self.queue_size != 0 {
            batch_config = batch_config.with_max_queue_size(self.queue_size);
        }

        let _batch_config = batch_config.build();
        let _logs = match _destination.protocol {
            #[cfg(feature = "grpc")]
            Protocol::Grpc => {
                use opentelemetry_otlp::{WithTonicConfig, WithExportConfig};
                let mut builder = opentelemetry_otlp::LogExporter::builder().with_tonic().with_endpoint(_destination.url.clone().into_owned());

                if cfg!(feature = "grpc-compression") && self.compression {
                    builder = builder.with_compression(opentelemetry_otlp::Compression::Gzip)
                }

                if !self.headers.is_empty() {
                    let headers = create_metadata_map(&self.headers);
                    builder = builder.with_metadata(headers);
                }

                let exporter = builder.with_timeout(self.timeout).build().expect("Failed to initialize logs grpc exporter");
                self.runtime.create_logger_exporter(exporter, _batch_config, &_destination)
            },
            #[cfg(not(feature = "grpc"))]
            Protocol::Grpc => missing_grpc_feature(),

            #[cfg(feature = "datadog")]
            Protocol::DatadogAgent => {
                let attributes = _destination.get_service_attrs();
                if let Some(file_path) = _destination.url.strip_prefix("file://") {
                    self.runtime.create_logger_exporter(crate::datadog::file_exporter(file_path.to_owned().into()).with_attrs(attributes), _batch_config, &_destination)
                } else {
                    self.runtime.create_logger_exporter(crate::datadog::stdout_exporter().with_attrs(attributes), _batch_config, &_destination)
                }
            }
            #[cfg(not(feature = "datadog"))]
            Protocol::DatadogAgent => missing_datadog_feature(),

            #[cfg(feature = "http")]
            http => {
                use opentelemetry_otlp::WithExportConfig;
                let url = format!("{}/logs", _destination.url.trim_end_matches('/'));
                let mut builder = opentelemetry_otlp::LogExporter::builder().with_http().with_protocol(http.into_otel()).with_endpoint(url);

                builder = self.apply_otel_http_config(builder);

                let exporter = builder.with_timeout(self.timeout).build().expect("Failed to initialize logs http exporter");
                self.runtime.create_logger_exporter(exporter, _batch_config, _destination)
            },
            #[cfg(not(feature = "http"))]
            _ => missing_http_feature(),
        };

        #[cfg(any(feature = "grpc", feature = "http", feature = "datadog"))]
        {
            let this = self;
            this.otlp.logs = Some(_logs.clone());
            return _logs;
        }
    }

    ///Enables `logs` exporter with provided `attrs` annotating logs
    ///
    ///Panics if called more than once
    ///
    ///Returns layer that can be used to record logs
    ///
    ///Note that it is recommended to disable sending of logs within spans via [TraceSettings::with_max_events_per_span]
    pub fn with_logs<S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>>(&mut self, destination: &Destination<'_>) -> impl tracing_subscriber::Layer<S> + Send + Sync + use<S> {
        opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&self.create_logs(destination))
    }

    fn create_tracer(&mut self, _destination: &Destination<'_>, _settings: TraceSettings) -> opentelemetry_sdk::trace::SdkTracer {
        if self.otlp.trace.is_some() {
            panic!("Trace is already initialized")
        }

        let mut batch_config = opentelemetry_sdk::trace::BatchConfigBuilder::default();
        if !self.export_interval.is_zero() {
            batch_config = batch_config.with_scheduled_delay(self.export_interval);
        }
        if self.queue_size != 0 {
            batch_config = batch_config.with_max_queue_size(self.queue_size);
        }

        let _batch_config = batch_config.build();
        let _trace = match _destination.protocol {
            #[cfg(feature = "grpc")]
            Protocol::Grpc => {
                use opentelemetry_otlp::{WithTonicConfig, WithExportConfig};
                let mut builder = opentelemetry_otlp::SpanExporter::builder().with_tonic().with_endpoint(_destination.url.clone().into_owned());

                if cfg!(feature = "grpc-compression") && self.compression {
                    builder = builder.with_compression(opentelemetry_otlp::Compression::Gzip)
                }

                if !self.headers.is_empty() {
                    let headers = create_metadata_map(&self.headers);
                    builder = builder.with_metadata(headers);
                }


                let exporter = builder.with_timeout(self.timeout).build().expect("Failed to initialize trace grpc exporter");
                self.runtime.create_tracer_exporter(exporter, _batch_config, &_destination, &_settings)
            },
            #[cfg(not(feature = "grpc"))]
            Protocol::Grpc => missing_grpc_feature(),

            #[cfg(feature = "datadog")]
            Protocol::DatadogAgent => {
                use crate::datadog::{SERVICE_NAME, SERVICE_VERSION, SERVICE_ENV};
                let mut exporter = opentelemetry_datadog::new_pipeline().with_agent_endpoint(_destination.url.clone());

                if let Some(attrs) = _destination.get_service_attrs() {
                    if let Some(service_name) = attrs.0.get(&SERVICE_NAME) {
                        exporter = exporter.with_service_name(service_name.to_string());
                    }
                    if let Some(service_version) = attrs.0.get(&SERVICE_VERSION) {
                        exporter = exporter.with_version(service_version.to_string());
                    }
                    if let Some(service_env) = attrs.0.get(&SERVICE_ENV) {
                        exporter = exporter.with_env(service_env.to_string());
                    }
                }

                #[cfg(feature = "http-ureq")]
                if let Some(ureq) = self.ureq.as_ref() {
                    exporter = exporter.with_http_client(ureq.clone());
                }

                let exporter = exporter.build_exporter().expect("Failed to initialize datadog exporter");
                self.runtime.create_tracer_exporter(exporter, _batch_config, &_destination, &_settings)
            },
            #[cfg(not(feature = "datadog"))]
            Protocol::DatadogAgent => missing_datadog_feature(),

            #[cfg(feature = "http")]
            http => {
                use opentelemetry_otlp::WithExportConfig;
                let url = format!("{}/traces", _destination.url.trim_end_matches('/'));
                let mut builder = opentelemetry_otlp::SpanExporter::builder().with_http().with_protocol(http.into_otel()).with_endpoint(url);

                builder = self.apply_otel_http_config(builder);
                let exporter = builder.with_timeout(self.timeout).build().expect("Failed to initialize trace http exporter");
                self.runtime.create_tracer_exporter(exporter, _batch_config, &_destination, &_settings)
            },
            #[cfg(not(feature = "http"))]
            _ => missing_http_feature(),
        };

        #[cfg(any(feature = "grpc", feature = "http", feature = "datadog"))]
        {
            use opentelemetry::trace::TracerProvider;

            let this = self;

            let tracer = _trace.tracer(_settings.name);
            this.otlp.trace = Some(_trace);
            return tracer;
        }
    }

    ///Enables `trace` exporter with provided `attrs` annotating traces, returning tracing layer
    ///
    ///Panics if called more than once
    ///
    ///Returns layer that can be used to record traces
    pub fn with_trace<S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>>(&mut self, destination: &Destination<'_>, settings: TraceSettings) -> impl tracing_subscriber::Layer<S> + use<S> {
        tracing_opentelemetry::OpenTelemetryLayer::new(self.create_tracer(destination, settings))
    }

    #[cfg(any(feature = "metrics", feature = "tracing-metrics"))]
    fn create_metrics(&mut self, _destination: &Destination<'_>, _settings: MetricsSettings) -> opentelemetry_sdk::metrics::SdkMeterProvider {
        if self.otlp.metrics.is_some() {
            panic!("Trace is already initialized")
        }

        let _metrics = match _destination.protocol {
            #[cfg(feature = "grpc")]
            Protocol::Grpc => {
                use opentelemetry_otlp::{WithTonicConfig, WithExportConfig};
                let mut builder = opentelemetry_otlp::MetricExporter::builder().with_tonic().with_endpoint(_destination.url.clone().into_owned()).with_temporality(_settings.temporality);

                if cfg!(feature = "grpc-compression") && self.compression {
                    builder = builder.with_compression(opentelemetry_otlp::Compression::Gzip)
                }

                if !self.headers.is_empty() {
                    let headers = create_metadata_map(&self.headers);
                    builder = builder.with_metadata(headers);
                }

                let exporter = builder.with_timeout(self.timeout).build().expect("Failed to initialize metrics grpc exporter");
                self.runtime.create_metrics_exporter(exporter, &_destination, self.export_interval)
            },
            #[cfg(not(feature = "grpc"))]
            Protocol::Grpc => missing_grpc_feature(),

            #[cfg(feature = "datadog")]
            Protocol::DatadogAgent => unsupported_datadog_feature(),
            #[cfg(not(feature = "datadog"))]
            Protocol::DatadogAgent => missing_datadog_feature(),

            #[cfg(feature = "http")]
            http => {
                use opentelemetry_otlp::WithExportConfig;
                let url = format!("{}/metrics", _destination.url.trim_end_matches('/'));
                let mut builder = opentelemetry_otlp::MetricExporter::builder().with_http().with_protocol(http.into_otel()).with_endpoint(url).with_temporality(_settings.temporality);

                builder = self.apply_otel_http_config(builder);

                let exporter = builder.with_timeout(self.timeout).build().expect("Failed to initialize metrics grpc exporter");
                self.runtime.create_metrics_exporter(exporter, &_destination, self.export_interval)
            },
            #[cfg(not(feature = "http"))]
            _ => missing_http_feature(),
        };

        #[cfg(any(feature = "grpc", feature = "http"))]
        {
            let this = self;
            this.otlp.metrics = Some(_metrics.clone());
            return _metrics;
        }
    }

    #[inline(always)]
    #[cfg(feature = "metrics")]
    ///Enables `metrics` exporter with provided `attrs` annotating metrics
    ///
    ///Panics if called more than once
    ///
    ///Returns `metrics::Recorder` to install and record `metrics`
    pub fn with_metrics(&mut self, destination: &Destination<'_>, settings: MetricsSettings, name: &'static str) -> impl crate::metrics::Recorder + Send + Sync + 'static {
        use opentelemetry::metrics::MeterProvider;

        let metrics = self.create_metrics(destination, settings);
        let meter = metrics.meter(name);
        let metrics = metrics_opentelemetry::OpenTelemetryMetrics::new(meter);
        metrics_opentelemetry::OpenTelemetryRecorder::new(metrics)
    }

    #[inline(always)]
    #[cfg(feature = "tracing-metrics")]
    ///Enables `tracing-metrics` exporter with provided `attrs` annotating metrics
    ///
    ///Panics if called more than once
    ///
    ///Returns layer that records metrics provided via event/span attributes
    ///Refer to [Layer](https://docs.rs/tracing-opentelemetry/latest/tracing_opentelemetry/struct.MetricsLayer.html) documentation for details.
    pub fn with_tracing_metrics<S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>>(&mut self, destination: &Destination<'_>, settings: MetricsSettings) -> impl tracing_subscriber::Layer<S> + Send + Sync + use<S> {
        tracing_opentelemetry::MetricsLayer::new(self.create_metrics(destination, settings))
    }

    #[inline]
    ///Finalizes building otlp integration
    pub fn finish(self) -> Otlp {
        self.otlp
    }
}
