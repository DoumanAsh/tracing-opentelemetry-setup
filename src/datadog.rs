use std::{fs, io};
use std::borrow::Cow;
use core::{fmt, cmp};
use core::sync::atomic::{self, Ordering};

use opentelemetry_sdk::logs::LogBatch;
use opentelemetry_sdk::error::{OTelSdkError, OTelSdkResult};
use serde::ser::{SerializeSeq, SerializeMap};

struct AnyValueSerde<'a>(&'a opentelemetry::logs::AnyValue);

impl serde::Serialize for AnyValueSerde<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use opentelemetry::logs::AnyValue;

        #[cold]
        #[inline(never)]
        fn unexpected_value<E: serde::ser::Error>(unexpected: &AnyValue) -> E {
            E::custom(format_args!("Unsupported value: {:?}", unexpected))
        }

        match self.0 {
            AnyValue::Boolean(value) => serializer.serialize_bool(*value),
            AnyValue::Int(value) => serializer.serialize_i64(*value),
            AnyValue::Double(value) => serializer.serialize_f64(*value),
            AnyValue::String(value) => serializer.serialize_str(value.as_str()),
            AnyValue::Bytes(value) => serializer.serialize_bytes(value),
            AnyValue::ListAny(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values.iter() {
                    seq.serialize_element(&AnyValueSerde(value))?
                }
                seq.end()
            },
            AnyValue::Map(values) => {
                let mut map = serializer.serialize_map(Some(values.len()))?;
                for (key, value) in values.iter() {
                    map.serialize_entry(key.as_str(), &AnyValueSerde(value))?
                }
                map.end()
            },
            //They use non exhaust for no reason so have to add this branch...
            value => Err(unexpected_value(value)),
        }
    }
}

pub struct Buffer {
    inner: [u8; 1024],
    len: usize,
}

impl Buffer {
    pub const fn new() -> Self {
        Self {
            inner: [0; 1024],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn as_str_with(&mut self, cb: impl FnOnce(&mut Self) -> bool) -> Option<&'_ str> {
        if (cb)(self) {
            self.as_str()
        } else {
            self.clear();
            None
        }
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    #[inline(always)]
    pub fn as_str(&self) -> Option<&'_ str> {
        core::str::from_utf8(&self.inner[..self.len]).ok()
    }

    pub fn push_bytes(&mut self, buf: &[u8]) -> usize {
        let output = &mut self.inner[self.len..];
        let written = cmp::min(output.len(), buf.len());
        output[..written].copy_from_slice(buf);
        self.len = self.len.saturating_add(written);
        written
    }
}

impl io::Write for Buffer {
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(self.push_bytes(buf))
    }

    #[inline(always)]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[repr(transparent)]
struct LogRecord<'a>(&'a opentelemetry_sdk::logs::SdkLogRecord);

impl<'a> serde::Serialize for LogRecord<'a> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut buffer = Buffer::new();
        let mut map = serializer.serialize_map(None)?;
        if let Some(message) = self.0.body() {
            map.serialize_entry("message", &AnyValueSerde(message))?;
        }

        if let Some(timestamp) = self.0.timestamp().or_else(|| self.0.observed_timestamp()) {
            let timestamp: time::UtcDateTime = timestamp.into();
            let timestamp = buffer.as_str_with(|buffer| timestamp.format_into(buffer, &time::format_description::well_known::Rfc3339).is_ok());
            if let Some(timestamp) = timestamp  {
                map.serialize_entry("timestamp", &timestamp)?;
            }
            buffer.clear();
        }

        if let Some(severity_text) = self.0.severity_text() {
            map.serialize_entry("level", severity_text)?;
        }

        if let Some(ctx) = &self.0.trace_context() {
            //Imagine not giving proper accessor to inner value...
            let trace_id = u128::from_be_bytes(ctx.trace_id.to_bytes());
            let span_id = u64::from_be_bytes(ctx.span_id.to_bytes());
            map.serialize_entry("dd.trace_id", &trace_id)?;
            map.serialize_entry("dd.span_id", &span_id)?;
        }
        for (key, value) in self.0.attributes_iter() {
            let key = buffer.as_str_with(|buffer| {
                buffer.push_bytes(b"fields.");
                buffer.push_bytes(key.as_str().as_bytes());
                true
            });
            if let Some(key) = key {
                map.serialize_entry(key, &AnyValueSerde(value))?;
            }
            buffer.clear();
        }
        map.end()
    }
}

pub struct IoLogExporter<IO> {
    create_dest: IO,
    is_shutdown: atomic::AtomicBool
}

impl<O: io::Write, IO: Fn() -> io::Result<O> + Sync + Send + 'static> IoLogExporter<IO> {
    #[inline(always)]
    pub fn new(create_dest: IO) -> Self {
        Self {
            create_dest,
            is_shutdown: atomic::AtomicBool::new(false),
        }
    }
}

impl<O: io::Write, IO: Fn() -> io::Result<O> + Sync + Send + 'static> opentelemetry_sdk::logs::LogExporter for IoLogExporter<IO> {
    /// Export logs to stdout
    async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
        if self.is_shutdown.load(Ordering::Acquire) {
            return Err(OTelSdkError::AlreadyShutdown)
        }

        let mut out = match (self.create_dest)() {
            Ok(out) => out,
            Err(error) => return Err(opentelemetry_sdk::error::OTelSdkError::InternalFailure(error.to_string())),
        };
        for (record, _) in batch.iter() {
            let record = LogRecord(record);
            if let Err(error) = serde_json::to_writer(&mut out, &record) {
                return Err(opentelemetry_sdk::error::OTelSdkError::InternalFailure(error.to_string()))
            }
        }

        Ok(())
    }

    #[inline(always)]
    fn shutdown_with_timeout(&self, _timeout: core::time::Duration) -> OTelSdkResult {
        self.is_shutdown.store(true, Ordering::Release);
        Ok(())
    }

    #[inline(always)]
    fn set_resource(&mut self, _res: &opentelemetry_sdk::Resource) {
    }
}

impl<IO> fmt::Debug for IoLogExporter<IO> {
    #[inline(always)]
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("IoLogExporter")
           .field("is_shutdown", &self.is_shutdown.load(Ordering::Acquire))
           .finish()
    }
}

///Creates stdout exporter
pub fn stdout_exporter() -> IoLogExporter<impl Fn() -> io::Result<io::StdoutLock<'static>>> {
    IoLogExporter::new(|| Ok(io::stdout().lock()))
}

pub fn file_exporter(path: Cow<'static, str>) -> IoLogExporter<impl Fn() -> io::Result<fs::File>> {
    IoLogExporter::new(move || fs::OpenOptions::new().append(true).create(true).open(&path.as_ref()))
}
