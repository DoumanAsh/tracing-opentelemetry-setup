//! tracing subscriber layer

#[non_exhaustive]
///Layer aggregation
pub struct OtlpLayer<S> {
    ///tracing layer
    pub trace: Option<tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::SdkTracer>>,
    ///logging layer
    pub logs: Option<opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge<opentelemetry_sdk::logs::SdkLoggerProvider, opentelemetry_sdk::logs::SdkLogger>>,
    #[cfg(feature = "tracing-metrics")]
    ///metrics layer
    pub metrics: Option<tracing_opentelemetry::MetricsLayer<S, opentelemetry_sdk::metrics::SdkMeterProvider>>,
}

macro_rules! impl_method {
    ($this:ident.$as_ref:ident().$method:ident($($fields:expr),+ $(,)*)) => {
        if let Some(trace) = $this.trace.$as_ref() {
            trace.$method($($fields,)+)
        }
        if let Some(logs) = $this.logs.$as_ref() {
            logs.$method($($fields,)+)
        }
        #[cfg(feature = "tracing-metrics")]
        if let Some(metrics) = $this.metrics.$as_ref() {
            metrics.$method($($fields,)+)
        }
    };
}

#[inline(always)]
fn apply_new_interest(interest: &mut tracing::subscriber::Interest, new_interest: tracing::subscriber::Interest) {
    if (interest.is_sometimes() && new_interest.is_always()) || (interest.is_never() && !new_interest.is_never()) {
        *interest = new_interest;
    }
}

impl<S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>> tracing_subscriber::Layer<S> for OtlpLayer<S> {
    #[inline(always)]
    fn on_layer(&mut self, subscriber: &mut S) {
        impl_method!(self.as_mut().on_layer(subscriber));
    }

    #[inline]
    fn on_new_span(&self, attrs: &tracing::span::Attributes<'_>, id: &tracing::span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        impl_method!(self.as_ref().on_new_span(attrs, id, ctx.clone()));
    }

    #[inline]
    fn register_callsite(&self, metadata: &'static tracing::Metadata<'static>) -> tracing::subscriber::Interest {
        let mut interest = tracing::subscriber::Interest::never();
        if let Some(trace) = self.trace.as_ref() {
            let new_interest = trace.register_callsite(metadata);
            apply_new_interest(&mut interest, new_interest);
        }
        if let Some(logs) = self.logs.as_ref() {
            let new_interest = tracing_subscriber::Layer::<S>::register_callsite(logs, metadata);
            apply_new_interest(&mut interest, new_interest);
        }
        #[cfg(feature = "tracing-metrics")]
        if let Some(metrics) = self.metrics.as_ref() {
            let new_interest = metrics.register_callsite(metadata);
            apply_new_interest(&mut interest, new_interest);
        }
        interest
    }

    #[inline]
    fn enabled(&self, metadata: &tracing::Metadata<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) -> bool {
        let mut is_enabled = true;
        if let Some(trace) = self.trace.as_ref() {
            is_enabled &= trace.enabled(metadata, ctx.clone());
        }
        if let Some(logs) = self.logs.as_ref() {
            is_enabled &= logs.enabled(metadata, ctx.clone());
        }
        #[cfg(feature = "tracing-metrics")]
        if let Some(metrics) = self.metrics.as_ref() {
            is_enabled &= metrics.enabled(metadata, ctx.clone());
        }
        is_enabled
    }

    #[inline]
    fn event_enabled(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) -> bool {
        let mut is_enabled = true;
        if let Some(trace) = self.trace.as_ref() {
            is_enabled &= trace.event_enabled(event, ctx.clone());
        }
        if let Some(logs) = self.logs.as_ref() {
            is_enabled &= logs.event_enabled(event, ctx.clone());
        }
        #[cfg(feature = "tracing-metrics")]
        if let Some(metrics) = self.metrics.as_ref() {
            is_enabled &= metrics.event_enabled(event, ctx.clone());
        }
        is_enabled
    }

    #[inline]
    fn max_level_hint(&self) -> Option<tracing_subscriber::filter::LevelFilter> {
        let mut level = tracing_subscriber::filter::LevelFilter::OFF;
        if let Some(trace) = self.trace.as_ref() {
            let new_level = trace.max_level_hint()?;
            level = core::cmp::min(level, new_level);
        }
        if let Some(logs) = self.logs.as_ref() {
            let new_level = tracing_subscriber::Layer::<S>::max_level_hint(logs)?;
            level = core::cmp::min(level, new_level);
        }
        #[cfg(feature = "tracing-metrics")]
        if let Some(metrics) = self.metrics.as_ref() {
            let new_level = metrics.max_level_hint()?;
            level = core::cmp::min(level, new_level);
        }
        Some(level)
    }

    #[inline]
    fn on_record(&self, span: &tracing::span::Id, values: &tracing::span::Record<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        impl_method!(self.as_ref().on_record(span, values, ctx.clone()));
    }

    #[inline]
    fn on_follows_from(&self, span: &tracing::span::Id, follows: &tracing::span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        impl_method!(self.as_ref().on_follows_from(span, follows, ctx.clone()));
    }

    #[inline]
    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        impl_method!(self.as_ref().on_event(event, ctx.clone()));
    }

    #[inline]
    fn on_enter(&self, id: &tracing::span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        impl_method!(self.as_ref().on_enter(id, ctx.clone()));
    }

    #[inline]
    fn on_exit(&self, id: &tracing::span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        impl_method!(self.as_ref().on_exit(id, ctx.clone()));
    }

    #[inline]
    fn on_close(&self, id: tracing::span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        impl_method!(self.as_ref().on_close(id.clone(), ctx.clone()));
    }

    #[inline]
    fn on_id_change(&self, old: &tracing::span::Id, new: &tracing::span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        impl_method!(self.as_ref(). on_id_change(old, new, ctx.clone()));
    }
}
