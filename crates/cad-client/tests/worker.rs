use cad_client::{protocol::*, WorkerClient};
fn server() -> std::path::PathBuf {
    std::env::var_os("CAD_SERVER")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build/cad-server")
        })
}
#[tokio::test(flavor = "current_thread")]
async fn real_worker_workflow_and_replay() {
    let mut client = WorkerClient::spawn(server(), ClientId::new("rust-contract").unwrap())
        .await
        .unwrap();
    assert!(client.context().unwrap().service_id.is_none());
    let request = client.prepare(Command::DocumentCreate).unwrap();
    let first = client.send(&request).await.unwrap();
    let replay = client.send(&request).await.unwrap();
    assert!(replay.replayed);
    assert_eq!(first.result, replay.result);
    assert_eq!(
        serde_json::to_value(&first.events).unwrap(),
        serde_json::to_value(&replay.events).unwrap()
    );
    let created: CreatedDocument = first.result_as().unwrap();
    let doc = created.document;
    let mut conflict = request.clone();
    conflict.command = Some(Command::Capabilities);
    assert!(matches!(
        client.send(&conflict).await.unwrap().error.unwrap().kind,
        ErrorKind::RequestConflict
    ));
    let size = [Finite::new(10.).unwrap(); 3];
    let box_reply = client
        .execute(Command::Box {
            document: doc.clone(),
            revision: Revision::new(0).unwrap(),
            size,
        })
        .await
        .unwrap();
    let boxed: DocumentResult<DocumentState> = box_reply.result_as().unwrap();
    assert_eq!(boxed.data.volume.unwrap().get(), 1000.);
    assert_eq!(box_reply.events.len(), 1);
    let edges_reply = client
        .execute(Command::Edges {
            document: doc.clone(),
        })
        .await
        .unwrap();
    assert!(edges_reply.events.is_empty());
    let edges: DocumentResult<Vec<Edge>> = edges_reply.result_as().unwrap();
    assert_eq!(edges.revision, boxed.revision);
    let command = Command::Fillet {
        document: doc.clone(),
        revision: edges.revision,
        edge: edges.data[0].reference.clone(),
        radius: Finite::new(1.).unwrap(),
    };
    let fillet: DocumentResult<DocumentState> =
        client.execute(command).await.unwrap().result_as().unwrap();
    assert!(fillet.data.valid.unwrap());
    assert!(
        (fillet.data.volume.unwrap().get() - (1000. - 10. * (1. - std::f64::consts::PI / 4.)))
            .abs()
            < 1e-4
    );
    let state: DocumentResult<DocumentState> = client
        .execute(Command::DocumentState {
            document: doc.clone(),
        })
        .await
        .unwrap()
        .result_as()
        .unwrap();
    assert_eq!(state.revision, fillet.revision);
    let mesh: DocumentResult<Mesh> = client
        .execute(Command::Mesh {
            document: doc.clone(),
        })
        .await
        .unwrap()
        .result_as()
        .unwrap();
    assert!(mesh.data.triangles.len() > 12);
    assert_eq!(mesh.revision, fillet.revision);
    let stale = client
        .execute(Command::Box {
            document: doc.clone(),
            revision: Revision::new(0).unwrap(),
            size,
        })
        .await
        .unwrap();
    assert!(matches!(
        stale.error.unwrap().kind,
        ErrorKind::RevisionConflict
    ));
    assert!(stale.events.is_empty());
    let undone: DocumentResult<DocumentState> = client
        .execute(Command::Undo {
            document: doc.clone(),
            revision: fillet.revision,
        })
        .await
        .unwrap()
        .result_as()
        .unwrap();
    assert_eq!(undone.data.volume.unwrap().get(), 1000.);
    let redone: DocumentResult<DocumentState> = client
        .execute(Command::Redo {
            document: doc,
            revision: undone.revision,
        })
        .await
        .unwrap()
        .result_as()
        .unwrap();
    assert_eq!(redone.data.volume, fillet.data.volume);
    let status = client.status(request.request_id).await.unwrap();
    match status.result_as::<RequestStatus>().unwrap() {
        RequestStatus::Completed { response } => assert_eq!(response.result, first.result),
        _ => panic!("cached request result missing"),
    }
    let mut unsupported = client.prepare(Command::Capabilities).unwrap();
    unsupported.protocol = "999.0".into();
    assert!(matches!(
        client.send(&unsupported).await.unwrap().error.unwrap().kind,
        ErrorKind::VersionMismatch
    ));
    client.shutdown().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn invalid_numbers_never_reach_the_worker() {
    let mut client = WorkerClient::spawn(server(), ClientId::new("rust-invalid").unwrap())
        .await
        .unwrap();
    assert!(Finite::new(f64::NAN).is_err());
    assert!(Finite::new(f64::INFINITY).is_err());
    let doc: CreatedDocument = client
        .execute(Command::DocumentCreate)
        .await
        .unwrap()
        .result_as()
        .unwrap();
    let invalid = Command::Box {
        document: doc.document.clone(),
        revision: Revision::new(0).unwrap(),
        size: [Finite::new(-1.).unwrap(); 3],
    };
    assert!(client.execute(invalid).await.is_err());
    let state: DocumentResult<DocumentState> = client
        .execute(Command::DocumentState {
            document: doc.document,
        })
        .await
        .unwrap()
        .result_as()
        .unwrap();
    assert!(state.data.empty);
    assert_eq!(state.revision.get(), 0);
    client.shutdown().await.unwrap();
}
