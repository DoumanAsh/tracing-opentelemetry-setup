

#[cfg(feature = "datadog")]
#[test]
pub fn should_export_datadog_agent_logs() {
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

    let attrs = tracing_opentelemetry_setup::builder::Attributes::builder().with_attr("service.name", "datadog_agent_test").finish();
    let destination = tracing_opentelemetry_setup::builder::Destination {
        url: "file://datadog_agent.log".into(),
        protocol: tracing_opentelemetry_setup::builder::Protocol::DatadogAgent,
    };
    let mut otlp = tracing_opentelemetry_setup::builder::Otlp::builder(destination).with_logs(Some(&attrs)).finish();
    let _guard = otlp.local_init_tracing_subscriber("datadog_agent", tracing_subscriber::registry());

    tracing::info!(data=1, "my message");

    drop(_guard);
    otlp.shutdown(None).expect("success");

    let result: serde_json::Value = serde_json::from_reader(std::fs::File::open(OUTPUT_FILE).unwrap()).expect("to read file");
    assert_eq!(result["level"], "INFO");
    assert_eq!(result["message"], "my message");
    assert_eq!(result["fields.data"], 1);
    let timestamp = result["timestamp"].as_str().expect("to have timestamp field");
    assert!(timestamp.ends_with("Z"));
    assert!(timestamp.starts_with("20"));
}
