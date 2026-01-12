//! Opentelemtry propagation support

use core::marker;
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry_sdk::propagation::TraceContextPropagator;

pub use opentelemetry::trace::Status;
pub use opentelemetry::propagation::Injector;

///Interface to extract parent trace context
pub trait ParentSource {
    ///Retrieves the value by specified key
    fn get(&self, key: &str) -> Option<&str>;
    ///Returns list of keys
    fn keys(&self) -> impl Iterator<Item = &str>;
}

impl<T: ParentSource> ParentSource for &'_ T {
    #[inline(always)]
    fn get(&self, key: &str) -> Option<&str> {
        T::get(self, key)
    }
    #[inline(always)]
    fn keys(&self) -> impl Iterator<Item = &str> {
        T::keys(self)
    }
}

impl<T: ParentSource> ParentSource for &'_ mut T {
    #[inline(always)]
    fn get(&self, key: &str) -> Option<&str> {
        T::get(self, key)
    }
    #[inline(always)]
    fn keys(&self) -> impl Iterator<Item = &str> {
        T::keys(self)
    }
}

impl<T: ParentSource> ParentSource for Box<T> {
    #[inline(always)]
    fn get(&self, key: &str) -> Option<&str> {
        T::get(self, key)
    }
    #[inline(always)]
    fn keys(&self) -> impl Iterator<Item = &str> {
        T::keys(self)
    }
}

impl<T: ParentSource> ParentSource for std::sync::Arc<T> {
    #[inline(always)]
    fn get(&self, key: &str) -> Option<&str> {
        T::get(self, key)
    }
    #[inline(always)]
    fn keys(&self) -> impl Iterator<Item = &str> {
        T::keys(self)
    }
}

impl<T: ParentSource> ParentSource for std::rc::Rc<T> {
    #[inline(always)]
    fn get(&self, key: &str) -> Option<&str> {
        T::get(self, key)
    }
    #[inline(always)]
    fn keys(&self) -> impl Iterator<Item = &str> {
        T::keys(self)
    }
}

#[cfg(feature = "grpc")]
impl ParentSource for tonic::metadata::MetadataMap {
    #[inline(always)]
    fn get(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|value| value.to_str().ok())
    }
    #[inline(always)]
    fn keys(&self) -> impl Iterator<Item = &str> {
        self.iter().map(|kv| match kv {
            tonic::metadata::KeyAndValueRef::Ascii(key, _) => key.as_str(),
            tonic::metadata::KeyAndValueRef::Binary(key, _) => key.as_str(),
        })
    }
}

#[cfg(feature = "http")]
impl ParentSource for http::HeaderMap {
    #[inline(always)]
    fn get(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|value| value.to_str().ok())
    }
    #[inline(always)]
    fn keys(&self) -> impl Iterator<Item = &str> {
        self.iter().map(|(key, _)| key.as_str())
    }
}

#[repr(transparent)]
struct ParentSourceImpl<T: ParentSource>(T);

impl<T: ParentSource> Extractor for ParentSourceImpl<T> {
    #[inline(always)]
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key)
    }

    #[inline(always)]
    fn keys(&self) -> Vec<&str> {
        self.0.keys().collect()
    }
}

#[repr(transparent)]
#[derive(Copy, Clone)]
///Generic source taking over key value pairs
///
///Any reference to collection will work as source
///
///```rust
///use tracing_opentelemetry_setup::propagation::{Context, ParentSourceIter};
///
///let source = std::collections::HashMap::<String, String>::new();
///Context::current().set_parent_from(ParentSourceIter::new(&source));
/////or directly since it is map
///Context::current().set_parent_from(&source);
///```
pub struct ParentSourceIter<'a, K: AsRef<str> + 'a, V: AsRef<str> + 'a, T: IntoIterator<Item = (&'a K, &'a V)> + Copy + 'a> {
    inner: T,
    _fields: marker::PhantomData<(&'a K, &'a V)>,
}

impl<'a, K: AsRef<str> + 'a, V: AsRef<str> + 'a, T: IntoIterator<Item = (&'a K, &'a V)> + Copy + 'a> ParentSourceIter<'a, K, V, T> {
    #[inline(always)]
    ///Creates new instance
    pub const fn new(inner: T) -> Self {
        Self {
            inner,
            _fields: marker::PhantomData
        }
    }
}

impl<'a, K: AsRef<str> + 'a, V: AsRef<str> + 'a, T: IntoIterator<Item = (&'a K, &'a V)> + Copy + 'a> ParentSource for ParentSourceIter<'a, K, V, T> {
    #[inline(always)]
    fn get(&self, expected_key: &str) -> Option<&str> {
        for (key, value) in self.inner.into_iter() {
            if key.as_ref() == expected_key {
                return Some(value.as_ref())
            }
        }

        None
    }

    #[inline(always)]
    fn keys(&self) -> impl Iterator<Item = &str> {
        self.inner.into_iter().map(|(key, _)| key.as_ref())
    }
}

impl<K: core::borrow::Borrow<str> + core::hash::Hash + Eq, V: AsRef<str>> ParentSource for std::collections::HashMap<K, V> {
    #[inline(always)]
    fn get(&self, expected_key: &str) -> Option<&str> {
        std::collections::HashMap::get(self, expected_key).map(|value| value.as_ref())
    }

    #[inline(always)]
    fn keys(&self) -> impl Iterator<Item = &str> {
        self.into_iter().map(|(key, _)| key.borrow())
    }
}

impl<K: core::borrow::Borrow<str> + Ord, V: AsRef<str>> ParentSource for std::collections::BTreeMap<K, V> {
    #[inline(always)]
    fn get(&self, expected_key: &str) -> Option<&str> {
        std::collections::BTreeMap::get(self, expected_key).map(|value| value.as_ref())
    }

    #[inline(always)]
    fn keys(&self) -> impl Iterator<Item = &str> {
        self.into_iter().map(|(key, _)| key.borrow())
    }
}

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
    pub fn set_parent_from(&self, source: impl ParentSource) {
        let parent = TraceContextPropagator::new().extract(&ParentSourceImpl(source));
        let _ = self.span.set_parent(parent);
    }

    #[inline(always)]
    ///Extract `self` into `dest`
    pub fn inject_into(&self, dest: &mut dyn Injector) {
        TraceContextPropagator::new().inject_context(&self.span.context(), dest);
    }
}
