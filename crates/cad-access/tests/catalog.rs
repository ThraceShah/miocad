use cad_access::{catalog, tool};
use cad_protocol::validate;
use serde_json::json;
#[test]
fn published_tool_and_catalog_share_strict_parameter_contracts() {
    let tool = tool().unwrap();
    let input = &tool["inputSchema"];
    assert!(!input.to_string().contains("$ref"));
    let catalog = catalog().unwrap();
    let names: std::collections::BTreeSet<_> = catalog
        .iter()
        .map(|spec| spec["id"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), catalog.len());
    for spec in &catalog {
        assert_eq!(spec["input_schema"]["additionalProperties"], false);
    }
    for command in [
        json!({"op":"document.create"}),
        json!({"op":"model.box","document":"doc","revision":0,"size":[10,10,10]}),
        json!({"op":"render.snapshot","document":"doc"}),
        json!({"op":"render.pick","render_id":"frame","x":0,"y":1}),
        json!({"op":"service.events","after":0}),
    ] {
        validate(&json!({"command":command}), input).unwrap();
        let spec = catalog
            .iter()
            .find(|spec| spec["id"] == command["op"])
            .unwrap();
        validate(&command, &spec["input_schema"]).unwrap();
        let mut extra = command.clone();
        extra["surprise"] = json!(true);
        assert!(validate(&json!({"command":extra}), input).is_err());
    }
    for command in [
        json!({"op":"model.box","document":"doc","revision":false,"size":[1,1,1]}),
        json!({"op":"render.pick","render_id":"frame","x":1.0,"y":0}),
        json!({"op":"render.snapshot","document":"doc","width":9000}),
        json!({"op":"unknown"}),
    ] {
        assert!(validate(&json!({"command":command}), input).is_err());
    }
}
