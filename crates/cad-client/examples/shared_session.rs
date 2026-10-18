//! Running SDK example, not the P4 product CLI. Leaves the default daemon alive.
use cad_client::{protocol::*, ServiceOptions, SharedClient};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut options = ServiceOptions::new(
        args.first()
            .map(String::as_str)
            .unwrap_or("target/debug/cad-service"),
        args.get(1)
            .map(String::as_str)
            .unwrap_or("build/cad-server"),
    );
    if let Some(dir) = args.get(2) {
        options.runtime_dir = dir.into();
    }
    let identity = cad_client::shared::unique_id()?;
    let mut a = SharedClient::connect_or_start(&options, &format!("example-a-{identity}")).await?;
    if args.get(3).is_some_and(|arg| arg == "--shutdown") {
        println!("{}", a.shutdown().await?);
        return Ok(());
    }
    let b = SharedClient::connect_or_start(&options, &format!("example-b-{identity}")).await?;
    let before = b.snapshot().await?["cursor"]
        .as_u64()
        .ok_or("missing cursor")?;
    let doc: CreatedDocument = a.execute(Command::DocumentCreate).await?.result_as()?;
    let boxed: DocumentResult<DocumentState> = a
        .execute(Command::Box {
            document: doc.document.clone(),
            revision: doc.revision,
            size: [Finite::new(10.)?; 3],
        })
        .await?
        .result_as()?;
    let edges: DocumentResult<Vec<Edge>> = a
        .execute(Command::Edges {
            document: doc.document.clone(),
        })
        .await?
        .result_as()?;
    let filleted: DocumentResult<DocumentState> = a
        .execute(Command::Fillet {
            document: doc.document.clone(),
            revision: boxed.revision,
            edge: edges.data[0].reference.clone(),
            radius: Finite::new(1.)?,
        })
        .await?
        .result_as()?;
    // All frontends use this cursor/snapshot pattern, including after reconnection.
    let changes = b.events(before).await?;
    let refreshed = b.snapshot().await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "service_id": a.endpoint().service_id, "document": doc.document,
            "fillet": filleted, "changes": changes, "client_b_snapshot": refreshed,
            "daemon_remains_running": true
        }))?
    );
    Ok(())
}
