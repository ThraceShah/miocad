use cad_client::{protocol::*, WorkerClient};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "build/cad-server".into());
    let mut client = WorkerClient::spawn(server, ClientId::new("rust-example")?).await?;
    let created: CreatedDocument = client.execute(Command::DocumentCreate).await?.result_as()?;
    let doc = created.document;
    client
        .execute(Command::Box {
            document: doc.clone(),
            revision: created.revision,
            size: [Finite::new(10.)?; 3],
        })
        .await?
        .result_as::<DocumentResult<DocumentState>>()?;
    let edges: DocumentResult<Vec<Edge>> = client
        .execute(Command::Edges {
            document: doc.clone(),
        })
        .await?
        .result_as()?;
    let selected = edges.data.first().ok_or("box contains no edges")?;
    let reply = client
        .execute(Command::Fillet {
            document: doc,
            revision: edges.revision,
            edge: selected.reference.clone(),
            radius: Finite::new(1.)?,
        })
        .await?;
    let result: DocumentResult<DocumentState> = reply.result_as()?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"worker":reply.context.worker_id,"session":reply.context.session_id,"state":result,"events":reply.events})
        )?
    );
    client.shutdown().await?;
    Ok(())
}
