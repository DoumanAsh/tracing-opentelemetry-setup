//! Opentelemtry propagation support

use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use opentelemetry::propagation::TextMapPropagator;
use opentelemetry_sdk::propagation::TraceContextPropagator;

pub use opentelemetry::trace::Status;
pub use opentelemetry::propagation::{Extractor, Injector};

///Span wrapper to provide opentelemetry context propagation
pub struct Context {
    span: Span,
}

impl Context {
    #[inline(always)]
    ///Creates context associated with `span`
    pub const fn new(span: Span) -> Self {
        Self {
            span,
        }
    }

    #[inline(always)]
    ///Creates context from currently execution context using `tracing::Span::current`
    pub fn current() -> Self {
        Self::new(tracing::Span::current())
    }

    #[inline(always)]
    ///Extracts `tracing::Span`
    pub fn into_tracing_span(self) -> Span {
        self.span
    }

    #[inline(always)]
    ///Sets span status
    pub fn set_status(&self, status: Status) {
        self.span.set_status(status);
    }

    #[inline(always)]
    ///Sets parent context from `source`
    ///
    ///Has effect only once
    pub fn set_parent_from(&self, source: &dyn Extractor) {
        let parent = TraceContextPropagator::new().extract(source);
        let _ = self.span.set_parent(parent);
    }

    #[inline(always)]
    ///Extract `self` into `dest`
    pub fn inject_into(&self, dest: &mut dyn Injector) {
        TraceContextPropagator::new().inject_context(&self.span.context(), dest);
    }
}
