use opentelemetry::{SpanId, TraceId, KeyValue};
use opentelemetry::trace::{SpanContext, TraceFlags, SpanKind, Status};
use opentelemetry::Context;
use opentelemetry_sdk::trace::{ShouldSample, SamplingDecision};

use crate::builder::{ParentBasedSampler, AlwaysOnSampler, AlwaysOffSampler};

use std::borrow::Cow;

#[derive(Debug)]
struct TestSpan(SpanContext);

impl opentelemetry::trace::Span for TestSpan {
    fn add_event_with_timestamp<T>(&mut self, _name: T, _timestamp: std::time::SystemTime, _attributes: Vec<KeyValue>) where T: Into<Cow<'static, str>> {
    }

    fn span_context(&self) -> &SpanContext {
        &self.0
    }
    fn is_recording(&self) -> bool {
        false
    }
    fn set_attribute(&mut self, _attribute: KeyValue) {}
    fn set_status(&mut self, _status: Status) {}
    fn update_name<T>(&mut self, _new_name: T) where T: Into<Cow<'static, str>> {
    }

    fn add_link(&mut self, _span_context: SpanContext, _attributes: Vec<KeyValue>) {}
    fn end_with_timestamp(&mut self, _timestamp: std::time::SystemTime) {}
}

#[test]
fn should_use_parent_decision_if_sampled() {
    use opentelemetry::trace::TraceContextExt;

    let sampler = ParentBasedSampler {
        sampler: AlwaysOffSampler,
    };
    let span_context = SpanContext::new(
        TraceId::from(1),
        SpanId::from(1),
        TraceFlags::SAMPLED,
        false,
        Default::default(),
    );
    let context = Context::new().with_span(TestSpan(span_context));
    let result = sampler.should_sample(Some(&context), 2.into(), "child_span", &SpanKind::Internal, &[], &[]);
    assert_eq!(result.decision, SamplingDecision::RecordAndSample);
}

#[test]
fn should_use_parent_decision_if_not_sampled() {
    use opentelemetry::trace::TraceContextExt;

    let sampler = ParentBasedSampler {
        sampler: AlwaysOnSampler,
    };
    let span_context = SpanContext::new(
        TraceId::from(1),
        SpanId::from(1),
        TraceFlags::NOT_SAMPLED,
        false,
        Default::default(),
    );
    let context = Context::new().with_span(TestSpan(span_context));
    let result = sampler.should_sample(Some(&context), 2.into(), "child_span", &SpanKind::Internal, &[], &[]);
    assert_eq!(result.decision, SamplingDecision::Drop);
}

#[test]
fn should_use_own_decision_if_no_parent() {
    use opentelemetry::trace::TraceContextExt;

    let sampler = ParentBasedSampler {
        sampler: AlwaysOffSampler,
    };
    let result = sampler.should_sample(None, 2.into(), "child_span", &SpanKind::Internal, &[], &[]);
    assert_eq!(result.decision, SamplingDecision::Drop);

    let sampler = ParentBasedSampler {
        sampler: AlwaysOnSampler,
    };
    let result = sampler.should_sample(None, 2.into(), "child_span", &SpanKind::Internal, &[], &[]);
    assert_eq!(result.decision, SamplingDecision::RecordAndSample);
}
