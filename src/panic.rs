//! Panic hook module

use core::panic::Location;
use std::panic::PanicHookInfo;
use std::sync::OnceLock;
use std::backtrace::{Backtrace, BacktraceStatus};

///Panic hook implementation
pub fn panic_hook(panic: &PanicHookInfo<'_>) {
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

    let backtrace = Backtrace::force_capture();
    if let BacktraceStatus::Captured = backtrace.status() {
        tracing::error!(
            exception.location = %location,
            exception.stacktrace = %backtrace,
            exception.message = msg,
            exception.type = "Rust Panic",
            "exception",
        );
    } else {
        tracing::error!(
            exception.location = %location,
            exception.message = msg,
            exception.type = "Rust Panic",
            "exception",
        );
    }
}

///Installs [panic_hook] once
pub fn install_panic_hook() {
    static ONCE: OnceLock<()> = OnceLock::new();

    ONCE.get_or_init(|| {
        let next = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            panic_hook(info);
            next(info);
        }));
    });
}
