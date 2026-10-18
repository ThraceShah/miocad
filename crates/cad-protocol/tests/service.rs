use cad_protocol::{service::*, ClientId, Request};
use serde_json::json;
#[test]
fn all_service_methods_match_the_neutral_contract() {
    let requests = [
        ServiceRequest::Ping,
        ServiceRequest::Sessions,
        ServiceRequest::CreateSession {
            key: "batch".into(),
        },
        ServiceRequest::Rpc {
            session: None,
            request: Request::hello(ClientId::new("client").unwrap())
                .unwrap()
                .value()
                .unwrap(),
        },
        ServiceRequest::Snapshot {
            session: "session".into(),
        },
        ServiceRequest::Events {
            session: "session".into(),
            after: 0,
        },
        ServiceRequest::Cancel {
            session: "session".into(),
            client_id: "client".into(),
            request_id: 1,
        },
        ServiceRequest::RenderSnapshot {
            session: "session".into(),
            document: "document".into(),
            width: 384,
            height: 384,
            directory: None,
        },
        ServiceRequest::RenderPick {
            session: "session".into(),
            render_id: "render".into(),
            x: 0,
            y: 0,
            target: "edge".into(),
        },
        ServiceRequest::Shutdown,
    ];
    assert_eq!(
        cad_protocol::contract().unwrap()["service_methods"]
            .as_array()
            .unwrap()
            .len(),
        requests.len()
    );
    for request in requests {
        let envelope = ServiceEnvelope {
            protocol: "1.0".into(),
            service_id: "service".into(),
            token: "secret".into(),
            request,
        };
        let value = serde_json::to_value(envelope).unwrap();
        validate_envelope(&value).unwrap();
        let decoded: ServiceEnvelope = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), value);
        let mut unknown = value.clone();
        unknown["request"]["surprise"] = json!(true);
        assert!(validate_envelope(&unknown).is_err());
        let mut wrong = value.clone();
        wrong["protocol"] = json!("2.0");
        assert!(validate_envelope(&wrong).is_err());
    }
    for after in [json!(-1), json!(1.0), json!(9007199254740992u64)] {
        assert!(validate_envelope(&json!({"protocol":"1.0","service_id":"service","token":"secret","request":{"method":"events","session":"session","after":after}})).is_err());
    }
}
