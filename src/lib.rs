#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![allow(clippy::style)]

#[cfg(feature = "http-ureq")]
mod ureq;
#[cfg(feature = "panic")]
pub mod panic;
#[cfg(feature = "propagation")]
pub mod propagation;
#[cfg(feature = "metrics")]
pub use metrics_opentelemetry::metrics;
pub use tracing;
pub use tracing_subscriber;
pub use opentelemetry;
pub use opentelemetry_sdk;
pub mod builder;
pub use builder::Otlp;
#[cfg(test)]
mod tests;
