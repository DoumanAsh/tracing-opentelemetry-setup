//!OpenTelemetry integration for tracing.
//!
//!The goal of this crate is to provide all-in-one crate to initialize OpenTelemetry integration with tracing
//!
//!MSRV 1.85
//!
//!## Features
//!
//! - `panic` - Provides panic hook implementation. Must be enabled via panic module
//!- `propagation` - Enables propagation utilities
//!- `metrics` - Enable integration with [metrics](https://crates.io/crates/metrics)
//!- `tracing-metrics` - Enable metrics usage via [tracing-opentelemetry](https://docs.rs/tracing-opentelemetry/latest/tracing_opentelemetry/struct.MetricsLayer.html)
//!- `rt-tokio` - Tell OpenTelemetry sdk that you use tokio runtime
//!
//!### Non-standard exporters
//!
//!- `datadog` - Enables datadog agent exporter. Currently supports only traces
//!
//!### Grpc features
//!
//!- `grpc` - Enables tonic based gRPC transport
//!- `grpc-compression` - Enables tonic based gRPC transport with compression
//!- `grpc-tls` - Enables tonic based gRPC transport with TLS
//!
//!### HTTP features
//!
//!Note that when enabling multiple clients, only one client will be used by default and it is up to [opentelemetry-otlp](https://github.com/open-telemetry/opentelemetry-rust/tree/main/opentelemetry-otlp)
//!
//!- `http` - Enables http exporter code without specific client as default option.
//!- `http-compression` - Enables http transport with compression
//!- `http-tls` - Enables http transport with TLS
//!
//!- `http-reqwest-blocking` - Enables blocking reqwest client.
//!- `http-reqwest` - Enables async reqwest client.
//!- `http-hyper` - Enables hyper client.
//!
//!## Usage
//!
//!Make sure `tracing-opentelemetry-setup` is installed to your dependencies
//!
//!```rust
//! use tracing_opentelemetry_setup::{Otlp, tracing_subscriber, tracing};
//! use tracing_opentelemetry_setup::builder::{Destination, Protocol, Attributes, TraceSettings};
//!
//! use tracing_subscriber::layer::SubscriberExt;
//!
//! let default_attrs = Attributes::builder().with_attr("service.name", "サービス").finish();
//! let trace_settings = TraceSettings::new(1.0);
//! let destination = Destination {
//!     protocol: Protocol::HttpBinary,
//!     url: "http://localhost:45081".into()
//! };
//! let mut otlp = Otlp::builder(destination).with_header("Authorization", "Basic <my token>").with_trace(Some(&default_attrs), trace_settings).finish();
//! let registry = tracing_subscriber::registry().with(tracing_subscriber::filter::LevelFilter::from_level(tracing::Level::INFO));
//! otlp.init_tracing_subscriber("tracing-opentelemetry", registry);
//!
//! //Do your job then shutdown to make sure you flush everything
//! otlp.shutdown(None).expect("successfully shut down OTLP")
//!```

#![warn(missing_docs)]
#![allow(clippy::style)]

#[cfg(feature = "datadog")]
mod datadog;
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
