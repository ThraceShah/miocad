use cad_client::{protocol::*, ClientError, RpcClient};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn fixture(
    mode: &'static str,
) -> (
    RpcClient<
        BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
        tokio::io::WriteHalf<tokio::io::DuplexStream>,
    >,
    tokio::task::JoinHandle<usize>,
) {
    let (client, server) = tokio::io::duplex(65536);
    let (read, write) = tokio::io::split(client);
    let (reader, mut writer) = tokio::io::split(server);
    let task = tokio::spawn(async move {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let hello: Value = serde_json::from_str(&line).unwrap();
        let caps = json!({"kernel":"smlib","gpu_picking":false,"persistent_naming":false,"native_format":"smb"});
        let mut response = json!({"protocol":"1.0","client_id":hello["client_id"],"request_id":0,"worker_id":"fixture-worker","session_id":"fixture-session","service_id":null,"ok":true,"events":[],"replayed":false,"result":{"capabilities":caps,"deduplication":{"scope":"worker","max_entries":64,"max_bytes":16777216,"durable":false},"mesh_transport":"json.inline","events":"inline","shared_service":false,"last_request_id":if mode=="resume"{10}else{0}}});
        writer
            .write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        if mode == "eof" || mode == "resume" {
            return 1;
        }
        response["request_id"] = if mode == "correlation" {
            json!(999)
        } else {
            request["request_id"].clone()
        };
        response["result"] = caps;
        if mode == "malformed" {
            writer.write_all(b"not-json\n").await.unwrap();
        } else if mode == "correlation" {
            writer
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
        }
        line.clear();
        let bytes = reader.read_line(&mut line).await.unwrap();
        assert_eq!(bytes, 0, "client automatically retried a request");
        1
    });
    let client = RpcClient::open(
        BufReader::new(read),
        write,
        ClientId::new("fixture-client").unwrap(),
        Duration::from_millis(100),
    )
    .await
    .unwrap();
    (client, task)
}
#[tokio::test(flavor = "current_thread")]
async fn eof_timeout_malformed_and_wrong_correlation_never_retry() {
    for mode in ["eof", "timeout", "malformed", "correlation"] {
        let (mut client, task) = fixture(mode).await;
        assert!(
            matches!(
                client.execute(Command::DocumentCreate).await,
                Err(ClientError::OutcomeUnknown { .. })
            ),
            "{mode}"
        );
        assert!(matches!(
            client.execute(Command::DocumentCreate).await,
            Err(ClientError::ConnectionUnusable)
        ));
        drop(client);
        assert_eq!(task.await.unwrap(), 1);
    }
}
#[tokio::test(flavor = "current_thread")]
async fn cancelled_future_poisoned_the_connection() {
    let (mut client, task) = fixture("timeout").await;
    assert!(tokio::time::timeout(
        Duration::from_millis(10),
        client.execute(Command::DocumentCreate)
    )
    .await
    .is_err());
    assert!(matches!(
        client.execute(Command::DocumentCreate).await,
        Err(ClientError::ConnectionUnusable)
    ));
    drop(client);
    assert_eq!(task.await.unwrap(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn hello_high_water_does_not_restart_request_ids() {
    let (mut client, task) = fixture("resume").await;
    let request = client.prepare(Command::DocumentCreate).unwrap();
    assert_eq!(request.request_id.get(), 11);
    assert!(matches!(
        client.send(&request).await,
        Err(ClientError::OutcomeUnknown { .. })
    ));
    drop(client);
    assert_eq!(task.await.unwrap(), 1);
}
