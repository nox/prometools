use prometheus_client::encoding::text::EncodeMetric;
use prometheus_client::registry::Registry;
use prometools::nonstandard::NonstandardUnsuffixedCounter;
use prometools::serde::Family;
use serde::Serialize;

#[track_caller]
fn encode_prom_text<M: EncodeMetric>(registry: &Registry<M>) -> String {
    let mut buf = vec![];
    prometheus_client::encoding::text::encode(&mut buf, registry).unwrap();
    String::from_utf8(buf).unwrap()
}

#[derive(Clone, PartialEq, Eq, Hash, Serialize)]
struct Labels {
    path: String,
    status: u16,
}

#[test]
fn counter_family() {
    let mut registry = Registry::default();
    let family = Family::<Labels, NonstandardUnsuffixedCounter>::default();
    registry.register("my_metric", "help text", family.clone());

    let label_a = Labels {
        path: "/foo".to_owned(),
        status: 200,
    };
    let label_b = Labels {
        path: "/bar".to_owned(),
        status: 404,
    };

    family.get_or_create(&label_a).inc_by(50);
    family.get_or_create(&label_b).inc();

    assert_eq!(family.get_or_create(&label_a).get(), 50);
    assert_eq!(family.get_or_create(&label_b).get(), 1);

    family.remove(&label_b);
    let prom_output = encode_prom_text(&registry);

    assert_eq!(
        prom_output,
        r#"# HELP my_metric help text.
# TYPE my_metric counter
my_metric{path="/foo",status="200"} 50
# EOF
"#
    );

    family.clear();
    let prom_output = encode_prom_text(&registry);

    assert_eq!(
        prom_output,
        r#"# HELP my_metric help text.
# TYPE my_metric counter
# EOF
"#
    )
}
