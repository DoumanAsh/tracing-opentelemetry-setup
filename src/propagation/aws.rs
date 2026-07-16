//! Opentelemtry propagation support for AWS Load Balancer [XRay](https://docs.aws.amazon.com/elasticloadbalancing/latest/application/load-balancer-request-tracing.html)

use super::{Context, ParentSource, ParentDestination};
use opentelemetry::trace::{TraceId, TraceFlags, TraceState, SpanId, SpanContext, TraceContextExt};
use opentelemetry::propagation::{text_map_propagator, TextMapPropagator, Extractor};

///Header name for AWS trace context
pub const AWS_XRAY_TRACE_HEADER: &str = "x-amzn-trace-id";

const HEADER_PARENT_KEY: &str = "Parent";
const HEADER_ROOT_KEY: &str = "Root";
const HEADER_SAMPLED_KEY: &str = "Sampled";

//Time is 8 bytes
//Id is 24 bytes
//Reference: https://docs.aws.amazon.com/elasticloadbalancing/latest/application/load-balancer-request-tracing.html#request-tracing-syntax
type TraceIdBuffer = str_buf::StrBuf::<{str_buf::capacity(24 + 8)}>;

const SAMPLED: &str = "1";
const NOT_SAMPLED: &str = "0";
const REQUESTED_SAMPLE_DECISION: &str = "?";

const TRACE_FLAG_DEFERRED: TraceFlags = TraceFlags::new(0x02);

#[derive(Clone, Default, Debug)]
///TextMapPropagator implementation for AWS XRay Trace
pub struct XRayPropagator {
    fields: [String; 1]
}

impl XRayPropagator {
    #[inline(always)]
    ///Creates new instance
    pub fn new() -> Self {
        Self::with_custom_header_name(AWS_XRAY_TRACE_HEADER.to_owned())
    }

    #[inline(always)]
    ///Creates new instance with non-default name of the context field
    pub fn with_custom_header_name(header: String) -> Self {
        Self {
            fields: [header]
        }
    }

    #[inline(always)]
    fn extract_span_context(&self, extractor: &dyn Extractor) -> Option<SpanContext> {
        extractor.get(self.fields[0].as_str()).and_then(|value| extract_span_context(value.trim()))
    }
}

impl TextMapPropagator for XRayPropagator {
    #[inline(always)]
    fn inject_context(&self, ctx: &opentelemetry::Context, injector: &mut dyn opentelemetry::propagation::Injector) {
        let span = ctx.span();
        let span_context = span.span_context();
        if span_context.is_valid() {
            injector.set(self.fields[0].as_str(), encode_span_context(span_context));
        }
    }

    #[inline(always)]
    fn extract_with_context(&self, ctx: &opentelemetry::Context, extractor: &dyn opentelemetry::propagation::Extractor) -> opentelemetry::Context {
        self.extract_span_context(extractor)
            .map(|span_context| ctx.with_remote_span_context(span_context))
            .unwrap_or_else(|| ctx.clone())
    }

    #[inline(always)]
    fn fields(&self) -> text_map_propagator::FieldIter<'_> {
        text_map_propagator::FieldIter::new(&self.fields)
    }
}

///Extracts SpanContext from header value containing value of [AWS_XRAY_TRACE_HEADER]
pub fn extract_span_context(header: &str) -> Option<SpanContext> {
    let mut trace_id = TraceId::INVALID;
    let mut parent_segment_id = SpanId::INVALID;
    let mut sampling_decision = TRACE_FLAG_DEFERRED;
    let mut trace_state = TraceState::default();

    //Do reverse as TraceState::insert performs reverse insert
    for value in header.split_terminator(';').rev() {
        let (key, value) = match value.trim().find('=') {
            Some(idx) => {
                let (key, value) = value.split_at(idx);
                (key, value.trim_start_matches('='))
            },
            None => continue
        };

        match key {
            HEADER_ROOT_KEY => {
                let mut parts = value.split_terminator('-');
                match parts.next() {
                    Some("1") => (),
                    Some(unknown) => {
                        opentelemetry::otel_warn!(name: "SpanContextFromAwsTraceId", message = format!("Version '{unknown}' is unrecognized"));
                    },
                    None => continue
                }

                let time = match parts.next() {
                    Some(time) => time,
                    _ => continue,
                };
                let id = match parts.next() {
                    Some(id) => id,
                    _ => continue,
                };

                let mut buffer = TraceIdBuffer::new();
                //trace id is very well documented so it is unlikely to fail hence no warning log
                if time.len() + id.len() == TraceIdBuffer::capacity() {
                    buffer.push_str(time);
                    buffer.push_str(id);
                    if let Ok(parsed_trace_id) = TraceId::from_hex(buffer.as_str()) {
                        trace_id = parsed_trace_id;
                    }
                }
            },
            HEADER_PARENT_KEY => if let Ok(span_id) = SpanId::from_hex(value) {
                parent_segment_id = span_id;
            },
            HEADER_SAMPLED_KEY => {
                sampling_decision = match value {
                    NOT_SAMPLED => TraceFlags::NOT_SAMPLED,
                    SAMPLED => TraceFlags::SAMPLED,
                    REQUESTED_SAMPLE_DECISION => TRACE_FLAG_DEFERRED,
                    _ => TRACE_FLAG_DEFERRED,
                }
            }
            _ => match trace_state.insert(key.to_ascii_lowercase(), value) {
                Ok(new_trace_state) => {
                    trace_state = new_trace_state;
                },
                Err(error) => {
                    opentelemetry::otel_warn!(name: "SpanContextFromAwsTraceId", message = error.to_string());
                }
            },
        }
    }

    if trace_id == TraceId::INVALID {
        None
    } else {
        Some(SpanContext::new(trace_id, parent_segment_id, sampling_decision, true, trace_state))
    }
}

///Encodes span context encoding according AWS XRay version 1
pub fn encode_span_context(ctx: &SpanContext) -> String {
    use core::fmt::Write;

    let mut trace_id_buffer = TraceIdBuffer::new();
    let _ = Write::write_fmt(&mut trace_id_buffer, format_args!("{}", ctx.trace_id()));
    let (time, id) = trace_id_buffer.split_at(8);

    let sampling_decision = if ctx.trace_flags() & TRACE_FLAG_DEFERRED == TRACE_FLAG_DEFERRED {
        REQUESTED_SAMPLE_DECISION
    } else if ctx.is_sampled() {
        SAMPLED
    } else {
        NOT_SAMPLED
    };

    let mut trace_state = String::new();
    for (state_key, state_value) in ctx.trace_state() {
        if !state_value.is_empty() {
            trace_state.reserve(state_key.len());
            let mut characters = state_key.chars();

            if let Some(first) = characters.next() {
                trace_state.push(first.to_ascii_uppercase())
            }
            trace_state.extend(characters)
        }
        trace_state.push('=');
        trace_state.push_str(state_value);
        trace_state.push(';');
    }

    let trace_state_prefix = if trace_state.is_empty() {
        ""
    } else {
        trace_state.pop();
        ";"
    };


    format!("{HEADER_ROOT_KEY}=1-{time}-{id};{HEADER_PARENT_KEY}={:016x};{HEADER_SAMPLED_KEY}={sampling_decision}{trace_state_prefix}{trace_state}", ctx.span_id())
}

impl Context {
    #[inline(always)]
    ///Creates new context inheriting parent context information from `source` using `context` associated with `span` using [XRayPropagator] format.
    ///
    ///Note that `span` must be freshly created and not entered, otherwise propagation will not work as opentelemetry allows propagation only on span creation
    ///
    ///You cannot initialize context using `tracing::instrument` so you always have to manually
    ///construct span (without entering into it) using one of `tracing::span!` macros
    pub fn new_from_aws_parent(span: tracing::Span, source: impl ParentSource) -> (tracing::Span, Self) {
        use tracing_opentelemetry::OpenTelemetrySpanExt;

        if let Some(parent) = source.get(AWS_XRAY_TRACE_HEADER).and_then(extract_span_context) {
            let _ = span.set_parent(opentelemetry::Context::map_current(|ctx| ctx.with_remote_span_context(parent)));
        }
        let this = Self {
            context: span.context()
        };
        (span, this)
    }

    #[inline(always)]
    ///Extracts Context from `source` linking it to the current span using [XRayPropagator] format.
    pub fn add_aws_link_from(&self, source: impl ParentSource) -> &Self {
        if let Some(parent) = source.get(AWS_XRAY_TRACE_HEADER).and_then(extract_span_context) {
            self.inner_add_link(&parent);
        }

        self
    }

    #[inline(always)]
    ///Extract `self` into `dest` using [XRayPropagator] format
    pub fn inject_aws_into(&self, dest: &mut impl ParentDestination) {
        let span = self.context.span();
        let span_context = span.span_context();
        if span_context.is_valid() {
            dest.set(AWS_XRAY_TRACE_HEADER, encode_span_context(&span_context));
        }
    }
}
