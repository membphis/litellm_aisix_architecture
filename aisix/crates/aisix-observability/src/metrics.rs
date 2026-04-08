use std::sync::OnceLock;

use prometheus::{core::Collector, Encoder, Registry, TextEncoder};

pub fn shared_registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::new)
}

pub fn register(collector: Box<dyn Collector>) -> anyhow::Result<()> {
    shared_registry()
        .register(collector)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

pub fn encode() -> anyhow::Result<String> {
    encode_registry(shared_registry())
}

fn encode_registry(registry: &Registry) -> anyhow::Result<String> {
    let metric_families = registry.gather();
    let mut buffer = Vec::new();
    TextEncoder::new()
        .encode(&metric_families, &mut buffer)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    String::from_utf8(buffer).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use prometheus::{IntCounter, Opts, Registry};

    use super::encode_registry;

    #[test]
    fn encode_registry_exposes_registered_metrics() {
        let registry = Registry::new();
        let metric_name = "aisix_test_counter";
        let counter = IntCounter::with_opts(Opts::new(metric_name, "test counter")).unwrap();
        registry.register(Box::new(counter.clone())).unwrap();

        counter.inc();

        let encoded = encode_registry(&registry).unwrap();
        assert!(encoded.contains(&metric_name));
        assert!(encoded.contains("test counter"));
    }
}
