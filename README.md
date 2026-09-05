# tracing-opentelemetry-setup

[![Rust](https://github.com/DoumanAsh/tracing-opentelemetry-setup/actions/workflows/rust.yml/badge.svg)](https://github.com/DoumanAsh/tracing-opentelemetry-setup/actions/workflows/rust.yml)
[![Crates.io](https://img.shields.io/crates/v/tracing-opentelemetry-setup.svg)](https://crates.io/crates/tracing-opentelemetry-setup)
[![Documentation](https://docs.rs/tracing-opentelemetry-setup/badge.svg)](https://docs.rs/crate/tracing-opentelemetry-setup/)
[![dependency status](https://deps.rs/crate/tracing-opentelemetry-setup/0.8.7/status.svg)](https://deps.rs/crate/tracing-opentelemetry-setup/0.8.7)

OpenTelemetry integration for tracing.

The goal of this crate is to provide all-in-one crate to initialize OpenTelemetry integration with tracing

MSRV 1.85

## Features

- `panic` - Provides panic hook implementation. Must be enabled via panic module
- `propagation` - Enables propagation utilities
- `propagation-aws` - Enables propagation utilities with support for [AWS XRay](https://docs.aws.amazon.com/elasticloadbalancing/latest/application/load-balancer-request-tracing.html)
- `metrics` - Enable integration with [metrics](https://crates.io/crates/metrics)
- `tracing-metrics` - Enable metrics usage via [tracing-opentelemetry](https://docs.rs/tracing-opentelemetry/latest/tracing_opentelemetry/struct.MetricsLayer.html)
- `rt-tokio` - Tell OpenTelemetry sdk that you use tokio runtime
- `tracing-log` - Enables `tracing-log` feature across all `tracing` ecosystem used by this crate.
- `internal-logs` - Enables `internal-logs` feature across opentelemetry crates.

### Grpc features

- `grpc` - Enables tonic based gRPC transport
- `grpc-compression` - Enables tonic based gRPC transport with compression
- `grpc-tls` - Enables tonic based gRPC transport with TLS
- `grpc-retry` - Enables retry logic for grpc exporter. Requires `tokio` feature to be used

### HTTP features

Note that when enabling multiple clients, only one client will be used by default and it is up to [opentelemetry-otlp](https://github.com/open-telemetry/opentelemetry-rust/tree/main/opentelemetry-otlp)

- `http` - Enables http exporter code without specific client as default option.
- `http-compression` - Enables http transport with compression
- `http-tls` - Enables http transport with TLS
- `http-retry` - Enables retry logic for HTTP exporter. Requires `tokio` feature to be used

- `http-reqwest-blocking` - Enables blocking reqwest client.
- `http-reqwest` - Enables async reqwest client.
- `http-hyper` - Enables hyper client.

- `http-ureq` - Enables option of using basic `ureq` http client (no dependency on async IO)
- `http-ureq-tls` - Enables TLS support in `ureq` using rustls with platform verifier.

## Usage

Make sure `tracing-opentelemetry-setup` is installed to your dependencies

```rust
use tracing_opentelemetry_setup::{Otlp, tracing_subscriber, tracing};
use tracing_opentelemetry_setup::builder::{Destination, Protocol, Attributes, TraceSettings, ExportRuntime};

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

let default_attrs = Attributes::builder().with_attr("service.name", "サービス").finish();
let trace_settings = TraceSettings::new("tracing-opentelemetry".into(), 1.0);
let destination = Destination {
    protocol: Protocol::HttpBinary,
    url: "http://localhost:45081".into(),
    attributes: Some(&default_attrs),
};

//Create common OTLP settings
let mut otlp = Otlp::builder().with_runtime(ExportRuntime::auto_detect()).with_header("Authorization", "Basic <my token>");
//Initialize subscriber
let registry = tracing_subscriber::registry().with(otlp.with_trace(&destination, trace_settings)) //initializes tracing and return layer
                                             .with(otlp.with_logs(&destination)) //initializes logging and returns layer
                                             .with(tracing_subscriber::filter::LevelFilter::from_level(tracing::Level::INFO));
//Finalizes OTLP returning guard
let mut otlp = otlp.finish();

let _guard = registry.set_default();
//Do your job then shutdown to make sure you flush everything
otlp.shutdown(None).expect("successfully shut down OTLP")
```

### Datadog usage

While datadog provides own protocol, it is extremely well supportive of the OTLP protocol:
- Documentation: https://docs.datadoghq.com/opentelemetry/setup/otlp_ingest_in_the_agent/?tab=host
- Reference terraform module: https://github.com/DoumanAsh/datadog-tf/tree/master/modules/agent
