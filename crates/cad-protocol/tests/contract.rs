use cad_protocol::*;
use serde_json::{json, Value};
#[test]
fn shared_schema_and_dto_roundtrips() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../../../protocol/conformance.json")).unwrap();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let which = case["schema"].as_str().unwrap();
        let value = &case["value"];
        let result = if let Some(op) = which.strip_prefix("command:") {
            validate(value, &command_spec(op).unwrap()["input_schema"])
        } else {
            validate_ref(value, which)
        };
        assert_eq!(
            result.is_ok(),
            case["valid"].as_bool().unwrap(),
            "{name}: {result:?}"
        );
        if case["valid"] != true {
            continue;
        }
        let roundtrip = match case["dtype"].as_str() {
            Some("request") => {
                serde_json::to_value(serde_json::from_value::<Request>(value.clone()).unwrap())
                    .unwrap()
            }
            Some("response") => {
                serde_json::to_value(Response::parse(value.clone()).unwrap()).unwrap()
            }
            Some("state") => serde_json::to_value(
                serde_json::from_value::<DocumentState>(value.clone()).unwrap(),
            )
            .unwrap(),
            _ => continue,
        };
        validate_ref(&roundtrip, which).unwrap_or_else(|e| panic!("{name}: {e}: {roundtrip}"));
        fn equivalent(a: &Value, b: &Value) -> bool {
            match (a, b) {
                (Value::Number(x), Value::Number(y)) => x == y || x.as_f64() == y.as_f64(),
                (Value::Array(x), Value::Array(y)) => {
                    x.len() == y.len() && x.iter().zip(y).all(|(a, b)| equivalent(a, b))
                }
                (Value::Object(x), Value::Object(y)) => {
                    x.len() == y.len()
                        && x.iter()
                            .all(|(k, a)| y.get(k).is_some_and(|b| equivalent(a, b)))
                }
                _ => a == b,
            }
        }
        assert!(
            equivalent(&roundtrip, value),
            "{name}: {roundtrip} != {value}"
        );
    }
}
#[test]
fn finite_and_error_types_follow_contract() {
    assert!(Finite::new(f64::NAN).is_err());
    assert!(Finite::new(f64::INFINITY).is_err());
    assert!(Revision::new(9007199254740992).is_err());
    for kind in contract().unwrap()["error_kinds"].as_array().unwrap() {
        let typed: ErrorKind = serde_json::from_value(kind.clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), *kind);
    }
    let doc = DocumentId::new("doc-1").unwrap();
    let command = Command::Fillet {
        document: doc,
        revision: Revision::new(1).unwrap(),
        edge: "opaque".into(),
        radius: Finite::new(-1.).unwrap(),
    };
    assert!(command.value().is_err());
    assert!(Response::parse(json!({"ok":true})).is_err());
}

#[test]
fn all_neutral_commands_have_rust_types() {
    for spec in contract().unwrap()["commands"].as_array().unwrap() {
        let info: CommandInfo = serde_json::from_value(spec.clone()).unwrap();
        let mut input = json!({"op":info.id});
        for name in info.input_schema["required"].as_array().unwrap() {
            let name = name.as_str().unwrap();
            if name == "op" {
                continue;
            }
            input[name] = match name {
                "document" => json!("fixture-doc"),
                "revision" => json!(0),
                "size" => json!([2., 3., 4.]),
                "edge" => json!("opaque-edge"),
                "radius" => json!(1.),
                "feature" => json!("f1"),
                "patch" => json!({"radius":2}),
                "transaction" => json!("tx.fixture"),
                "label" => json!("Fixture task"),
                "path" => json!("fixture.smb"),
                "operation" => json!({"kind":"box","size":[2,3,4]}),
                _ => panic!("missing typed fixture field {name}"),
            };
        }
        let command: Command = serde_json::from_value(input).unwrap();
        validate(&command.value().unwrap(), &info.input_schema).unwrap();
    }
}

#[test]
fn finite_f64_json_roundtrip_preserves_bits() {
    for value in [
        f64::MAX,
        f64::MIN_POSITIVE,
        1.0000000000000002,
        997.8539818196981,
        0.1,
        1e-310,
    ] {
        let wire = serde_json::to_string(&Finite::new(value).unwrap()).unwrap();
        let decoded: Finite = serde_json::from_str(&wire).unwrap();
        assert_eq!(decoded.get().to_bits(), value.to_bits(), "{wire}");
    }
}
