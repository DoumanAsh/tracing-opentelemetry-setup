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
    #[cfg(not(all(feature = "http", feature = "grpc")))]
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
