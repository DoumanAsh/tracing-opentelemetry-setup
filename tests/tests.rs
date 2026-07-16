use tracing_opentelemetry_setup::builder::{self, Destination, Attributes};

#[test]
fn should_determine_default_resources_from_env() {
    let attrs = Attributes::from_env();
    if std::env::var("OTEL_RESOURCE_ATTRIBUTES").is_err() && std::env::var("OTEL_SERVICE_NAME").is_err() {
        assert!(attrs.is_none(), "{:?} should not be set", attrs);
    } else {
        assert!(attrs.is_some());
    }
}

#[test]
fn should_determine_destination_from_env() {
    #[cfg(not(all(feature = "http", feature = "grpc", feature = "datadog")))]
    if std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL").is_err() {
        return;
    }

    let dest = Destination::from_env();
    if let Some(protocol) = builder::Protocol::from_env() {
        assert_eq!(dest.protocol, protocol);
    } else {
        assert_eq!(Some(dest.protocol), builder::Protocol::select_default());
    }
}

#[cfg(feature = "datadog")]
#[test]
pub fn should_export_datadog_agent_logs() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    const OUTPUT_FILE: &str = "datadog_agent.log";

    struct CleanupFile<'a>(&'a str);

    impl CleanupFile<'_> {
        fn clean(&self) {
            let _ = std::fs::remove_file(self.0);
        }
    }

    impl Drop for CleanupFile<'_> {
        #[inline(always)]
        fn drop(&mut self) {
            self.clean();
        }
    }

    let file = CleanupFile(OUTPUT_FILE);
    file.clean();

    let attrs = tracing_opentelemetry_setup::builder::Attributes::builder().with_attr("service.name", "datadog_agent_test").with_attr("smarty", "pants").with_attr("and", "another one").finish();
    let destination = tracing_opentelemetry_setup::builder::Destination {
        url: "file://datadog_agent.log".into(),
        protocol: tracing_opentelemetry_setup::builder::Protocol::DatadogAgent,
        attributes: Some(&attrs),
    };
    let mut otlp = tracing_opentelemetry_setup::builder::Otlp::builder();
    let _guard = tracing_subscriber::registry().with(otlp.with_logs(&destination)).set_default();
    let mut otlp = otlp.finish();

    tracing::info!(data=1, "my message");

    drop(_guard);
    otlp.shutdown(None).expect("success");

    let result: serde_json::Value = serde_json::from_reader(std::fs::File::open(OUTPUT_FILE).unwrap()).expect("to read file");
    println!("result={:#?}", result);
    assert_eq!(result["level"], "INFO");
    assert_eq!(result["message"], "my message");
    assert_eq!(result["service"], "datadog_agent_test");
    assert_eq!(result["fields.smarty"], "pants");
    assert_eq!(result["fields.data"], 1);
    assert_eq!(result["fields.and"], "another one");
    let timestamp = result["timestamp"].as_str().expect("to have timestamp field");
    assert!(timestamp.ends_with("Z"));
    assert!(timestamp.starts_with("20"));
}

#[cfg(feature = "propagation-aws")]
#[test]
pub fn should_extract_span_context() {
    use opentelemetry::trace::{TraceId, TraceFlags, TraceState, SpanId, SpanContext};
    use tracing_opentelemetry_setup::propagation::aws::{extract_span_context, encode_span_context};

    const TRACE_FLAG_DEFERRED: TraceFlags = TraceFlags::new(0x02);

    let data = [
        ("", SpanContext::empty_context()),
        ("Sampled=1;Self=foo", SpanContext::empty_context()),
        ("Root=1-bogus-bad", SpanContext::empty_context()),
        ("Root=1-too-many-parts", SpanContext::empty_context()),
        ("Root=1-58406520-a006649127e371903a2de979;Parent=garbage", SpanContext::new(TraceId::from_hex("58406520a006649127e371903a2de979").unwrap(), SpanId::INVALID, TRACE_FLAG_DEFERRED, true, TraceState::default())),
        ("Root=1-58406520-a006649127e371903a2de979;Sampled=1", SpanContext::new(TraceId::from_hex("58406520a006649127e371903a2de979").unwrap(), SpanId::INVALID, TraceFlags::SAMPLED, true, TraceState::default())),
        ("Root=1-58406520-a006649127e371903a2de979;Parent=4c721bf33e3caf8f;Sampled=0", SpanContext::new(TraceId::from_hex("58406520a006649127e371903a2de979").unwrap(), SpanId::from_hex("4c721bf33e3caf8f").unwrap(), TraceFlags::default(), true, TraceState::default())),
        ("Root=1-58406520-a006649127e371903a2de979;Parent=4c721bf33e3caf8f;Sampled=1", SpanContext::new(TraceId::from_hex("58406520a006649127e371903a2de979").unwrap(), SpanId::from_hex("4c721bf33e3caf8f").unwrap(), TraceFlags::SAMPLED, true, TraceState::default())),
        ("Root=1-58406520-a006649127e371903a2de979;Parent=4c721bf33e3caf8f", SpanContext::new(TraceId::from_hex("58406520a006649127e371903a2de979").unwrap(), SpanId::from_hex("4c721bf33e3caf8f").unwrap(), TRACE_FLAG_DEFERRED, true, TraceState::default())),
        ("Root=1-58406520-a006649127e371903a2de979;Parent=4c721bf33e3caf8f;Sampled=?", SpanContext::new(TraceId::from_hex("58406520a006649127e371903a2de979").unwrap(), SpanId::from_hex("4c721bf33e3caf8f").unwrap(), TRACE_FLAG_DEFERRED, true, TraceState::default())),
        ("Root=1-58406520-a006649127e371903a2de979;Self=1-58406520-bf42676c05e20ba4a90e448e;Parent=4c721bf33e3caf8f;Sampled=1", SpanContext::new(TraceId::from_hex("58406520a006649127e371903a2de979").unwrap(), SpanId::from_hex("4c721bf33e3caf8f").unwrap(), TraceFlags::SAMPLED, true, TraceState::from_key_value([("self", "1-58406520-bf42676c05e20ba4a90e448e")]).unwrap())),
        ("Root=1-58406520-a006649127e371903a2de979;Self=1-58406520-bf42676c05e20ba4a90e448e;Parent=4c721bf33e3caf8f;Sampled=1;RandomKey=RandomValue", SpanContext::new(TraceId::from_hex("58406520a006649127e371903a2de979").unwrap(), SpanId::from_hex("4c721bf33e3caf8f").unwrap(), TraceFlags::SAMPLED, true, TraceState::from_key_value([("self", "1-58406520-bf42676c05e20ba4a90e448e"), ("randomkey", "RandomValue")]).unwrap())),
    ];

    for (header_value, expected_span) in data {
        if expected_span.trace_id() != TraceId::INVALID {
            match extract_span_context(header_value) {
                Some(extracted_span) => assert_eq!(extracted_span, expected_span),
                None => panic!("Should parse '{header_value}' with valid span context {expected_span:#?}"),
            }

            let encoded_header_value = encode_span_context(&expected_span);
            match extract_span_context(&encoded_header_value) {
                Some(extracted_span) => assert_eq!(extracted_span, expected_span, "Encoded '{encoded_header_value}' doesn't match expected context"),
                None => panic!("Should parse '{encoded_header_value}' with valid span context {expected_span:#?}"),
            }
        } else {
            if let Some(unexpected_context) = extract_span_context(header_value) {
                panic!("'{header_value}' should be invalid span, but got {unexpected_context:#?}");
            }
        }
    }
}
