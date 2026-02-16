//! Panic hook module

use core::panic::Location;
use std::panic::PanicHookInfo;
use std::sync::OnceLock;
use std::backtrace::{Backtrace, BacktraceStatus};

///Describes how to record panic
pub trait PanicRecorder {
    ///Performs capture of the panic as event
    fn capture(location: &Location, message: &str, backtrace: Backtrace);
}

///Instructs to record panics as opentelemetry [exception](https://opentelemetry.io/docs/specs/semconv/exceptions/exceptions-spans/)
pub struct Exception;
impl PanicRecorder for Exception {
    #[track_caller]
    fn capture(location: &Location, message: &str, backtrace: Backtrace) {
        if let BacktraceStatus::Captured = backtrace.status() {
            tracing::error!(
                exception.location = %location,
                exception.stacktrace = %backtrace,
                exception.message = message,
                exception.type = "Rust Panic",
                "exception",
            );
        } else {
            tracing::error!(
                exception.location = %location,
                exception.message = message,
                exception.type = "Rust Panic",
                "exception",
            );
        }
    }
}

///Instructs to record panics as opentelemetry [error](https://opentelemetry.io/docs/specs/semconv/registry/attributes/error/#error-attributes) attribute
///
///In addition to that it records stacktrace as `error.stack` following [datadog conventions](https://docs.datadoghq.com/tracing/error_tracking/#use-span-attributes-to-track-error-spans)
///
///Prefer this if you use datadog as target
pub struct Error;
impl PanicRecorder for Error {
    #[track_caller]
    fn capture(location: &Location, message: &str, backtrace: Backtrace) {
        if let BacktraceStatus::Captured = backtrace.status() {
            tracing::error!(
                error.location = %location,
                error.stack = %backtrace,
                error.message = message,
                error.type = "Rust Panic",
                "exception",
            );
        } else {
            tracing::error!(
                error.location = %location,
                error.message = message,
                error.type = "Rust Panic",
                "exception",
            );
        }
    }
}

fn propagate_status(message: &str) {
    use opentelemetry::trace::Status;
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    //Panic hook is called before unwinding so we can set status on the span
    //But this has no effect if there are no spans at all
    let span = tracing::Span::current();
    span.set_status(Status::Error {
        description: message.to_owned().into(),
    })
}

///Panic hook implementation
pub fn panic_hook(panic: &PanicHookInfo<'_>) {
    panic_hook_as::<Exception>(panic)
}

///Panic hook implementation with customized recorder
pub fn panic_hook_as<P: PanicRecorder>(panic: &PanicHookInfo<'_>) {
    const DEFAULT_MESSAGE: &'static str = "panic occurred";

    let location = match panic.location() {
        Some(location) => location,
        None => Location::caller(),
    };
    let msg = match panic.payload().downcast_ref::<&'static str>() {
        Some(message) => message,
        None => match panic.payload().downcast_ref::<String>() {
            Some(message) => message.as_str(),
            None => &DEFAULT_MESSAGE,
        }
    };

    propagate_status(msg);
    let backtrace = Backtrace::force_capture();
    P::capture(location, msg, backtrace);
}

///Installs [panic_hook] once
pub fn install_panic_hook() {
    install_panic_hook_as(Exception)
}

///Installs [panic_hook] once with specified [PanicRecorder]
pub fn install_panic_hook_as<T: PanicRecorder>(_: T) {
    static ONCE: OnceLock<()> = OnceLock::new();

    ONCE.get_or_init(|| {
        let next = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            panic_hook_as::<T>(info);
            next(info);
        }));
    });
}
