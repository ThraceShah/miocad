//! Background autonomous task owner, with disk receipts and revision-scoped group Undo.
use super::*;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
#[derive(Clone)]
pub struct Tasks {
    root: PathBuf,
    endpoint: cad_protocol::service::ServiceEndpoint,
    session: String,
    jobs: Arc<Mutex<HashMap<String, TaskState>>>,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    client_id: String,
    state: TaskState,
    pending: Option<Request>,
}
impl Tasks {
    pub fn new(
        root: PathBuf,
        endpoint: cad_protocol::service::ServiceEndpoint,
        session: String,
    ) -> Self {
        Self {
            root,
            endpoint,
            session,
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    fn key(owner: &str, id: &str) -> Result<String, String> {
        if owner.is_empty()
            || owner.len() > 256
            || id.is_empty()
            || id.len() > 96
            || !owner
                .bytes()
                .chain(id.bytes())
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("Invalid task identity".into());
        }
        let key = format!("{owner}--{id}");
        if key.len() > 240 {
            return Err("Task identity too long".into());
        }
        Ok(key)
    }
    fn path(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.json"))
    }
    fn save(&self, key: &str, state: &TaskState, pending: Option<Request>) -> Result<(), String> {
        use std::os::unix::fs::DirBuilderExt;
        if !self.root.exists() {
            match std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.root)
            {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(_) => return Err("Cannot create Agent task journal".into()),
            }
        }
        let path = self.path(key);
        let tmp = path.with_extension("tmp");
        let client_id = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Journal>(&bytes).ok())
            .map(|r| r.client_id)
            .unwrap_or_else(|| {
                format!(
                    "agent-task-{}",
                    unique_id().expect("OS random source unavailable")
                )
            });
        let bytes = serde_json::to_vec(&Journal {
            client_id,
            state: state.clone(),
            pending,
        })
        .map_err(|_| "Cannot encode task journal")?;
        std::fs::write(&tmp, bytes).map_err(|_| "Cannot write task journal")?;
        std::fs::rename(tmp, path).map_err(|_| "Cannot save task journal".into())
    }
    async fn publish(&self, key: &str, state: &TaskState) {
        self.jobs.lock().await.insert(key.into(), state.clone());
    }
    pub async fn status(&self, owner: &str, id: &str) -> Result<TaskState, String> {
        let key = Self::key(owner, id)?;
        if let Some(state) = self.jobs.lock().await.get(&key).cloned() {
            return Ok(state);
        }
        let bytes = std::fs::read(self.path(&key)).map_err(|_| "Agent task not found")?;
        let mut record: Journal =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid Agent task journal")?;
        if !record.state.done {
            // Never replay an interrupted write. A pending request can be checked on the same client.
            record.state.done = true;
            record.state.phase = "interrupted".into();
            record.state.can_undo = false;
            record.state.text="Task host restarted. No writes were replayed; inspect the current model before continuing.".into();
            if let Some(request) = record.pending {
                if let Ok(mut client) = self.client(&key).await {
                    if request.worker_id.as_ref() == Some(&client.context().worker_id) {
                        if let Ok(response) = client.status(request.request_id).await {
                            if let Ok(RequestStatus::Completed { response }) =
                                response.result_as::<RequestStatus>()
                            {
                                if response.ok {
                                    record.state.text="The last interrupted operation was acknowledged. Inspect the current model; nothing was replayed.".into();
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(record.state)
    }
    pub async fn start(&self, owner: &str, request: TaskRequest) -> Result<TaskState, String> {
        let key = Self::key(owner, &request.id)?;
        request.turn.validate()?;
        let mut jobs = self.jobs.lock().await;
        if jobs.len() >= 256 {
            jobs.retain(|_, s| !s.done);
        }
        if let Some(state) = jobs.get(&key) {
            return Ok(state.clone());
        }
        if self.path(&key).exists() {
            drop(jobs);
            return self.status(owner, &request.id).await;
        }
        if jobs.values().filter(|s| !s.done).count() >= 2 {
            return Err("Agent is busy; wait for a running task".into());
        }
        let state = TaskState {
            id: request.id.clone(),
            document: request.turn.document.clone(),
            before: request.turn.revision,
            revision: request.turn.revision,
            phase: "thinking".into(),
            text: "Agent is working…".into(),
            steps: vec![],
            can_undo: false,
            done: false,
        };
        self.save(&key, &state, None)?;
        jobs.insert(key.clone(), state.clone());
        drop(jobs);
        let host = self.clone();
        tokio::spawn(async move {
            host.run(key, request.turn, state).await;
        });
        self.status(owner, &request.id).await
    }
    async fn client(&self, key: &str) -> Result<SharedClient, String> {
        let record: Journal = serde_json::from_slice(
            &std::fs::read(self.path(key)).map_err(|_| "Task journal unavailable")?,
        )
        .map_err(|_| "Invalid task journal")?;
        SharedClient::attach(
            self.endpoint.clone(),
            &record.client_id,
            Some(&self.session),
        )
        .await
        .map_err(|_| "CAD service unavailable".into())
    }
    async fn scene(
        client: &mut SharedClient,
        doc: DocumentId,
    ) -> Result<DocumentResult<Scene>, String> {
        let reply = client
            .execute(Command::Scene { document: doc })
            .await
            .map_err(|_| "CAD scene unavailable")?;
        if !reply.ok {
            return Err("CAD document unavailable".into());
        }
        reply.result_as().map_err(|_| "Invalid CAD scene".into())
    }
    async fn write(
        &self,
        key: &str,
        state: &mut TaskState,
        client: &mut SharedClient,
        command: Command,
    ) -> Result<Response, String> {
        let request = client.prepare(command).map_err(|e| e.to_string())?;
        self.save(key, state, Some(request.clone()))?;
        match client.send(&request).await {
            Ok(reply) => {
                let uncertain = reply.error.as_ref().is_some_and(|e| {
                    matches!(
                        serde_json::to_value(&e.kind)
                            .ok()
                            .and_then(|v| v.as_str().map(str::to_owned))
                            .as_deref(),
                        Some("outcome_unknown" | "outcome_unavailable")
                    )
                });
                if uncertain {
                    state.phase = "outcome_unknown".into();
                    return Err("Operation outcome unknown; nothing will be replayed".into());
                }
                Ok(reply)
            }
            Err(_) => {
                let recovery = async {
                    let mut recovered = self.client(key).await?;
                    let status = recovered
                        .status(request.request_id)
                        .await
                        .map_err(|_| "status unavailable")?;
                    let outcome: RequestStatus =
                        status.result_as().map_err(|_| "status unavailable")?;
                    match outcome {
                        RequestStatus::Completed { response } => {
                            *client = recovered;
                            Ok(*response)
                        }
                        _ => Err::<Response, String>("status unavailable".into()),
                    }
                }
                .await;
                match recovery {
                    Ok(response) => Ok(response),
                    Err(_) => {
                        state.phase = "outcome_unknown".into();
                        Err("Operation outcome unknown; nothing will be replayed".into())
                    }
                }
            }
        }
    }
    fn command(op: &Proposal, doc: &DocumentId, revision: Revision) -> Result<Command, String> {
        Ok(match op {
            Proposal::Fillet { reference, radius } => Command::Fillet {
                document: doc.clone(),
                revision,
                edge: reference.clone(),
                radius: Finite::new(*radius).map_err(|e| e.to_string())?,
            },
            Proposal::Box { size } => Command::Box {
                document: doc.clone(),
                revision,
                size: [
                    Finite::new(size[0]).map_err(|e| e.to_string())?,
                    Finite::new(size[1]).map_err(|e| e.to_string())?,
                    Finite::new(size[2]).map_err(|e| e.to_string())?,
                ],
            },
        })
    }
    async fn run(&self, key: String, turn: Turn, mut state: TaskState) {
        let result = self.execute(&key, &turn, &mut state).await;
        if let Err(error) = result {
            if state.phase == "outcome_unknown" {
                state.done = true;
                state.phase = "outcome_unknown".into();
                state.can_undo = false;
                state.text = error;
                self.publish(&key, &state).await;
                return;
            }
            state.phase = "failed".into();
            state.text = error;
            if !state.steps.is_empty() {
                match self.rollback(&key, &mut state).await {
                    Ok(()) => {
                        state.phase = "rolled_back".into();
                        state.text.push_str(" Task changes were rolled back.");
                    }
                    Err(e) => {
                        if state.phase != "outcome_unknown" {
                            state.phase = "partial".into();
                        }
                        state.text.push_str(&format!(" {e}"));
                    }
                }
            }
        }
        state.done = true;
        if state.phase == "outcome_unknown" {
            self.publish(&key, &state).await;
            return;
        }
        // A journal failure cannot turn an acknowledged write into a request for approval.
        if self.save(&key, &state, None).is_err() {
            state.can_undo = false;
            state
                .text
                .push_str(" Task journal unavailable; inspect the document history.");
        }
        self.publish(&key, &state).await;
    }
    async fn execute(&self, key: &str, turn: &Turn, state: &mut TaskState) -> Result<(), String> {
        let mut client = self.client(key).await?;
        let mut current = Self::scene(&mut client, turn.document.clone()).await?;
        let events = client
            .events(0)
            .await
            .map_err(|_| "CAD history unavailable")?;
        let events: Vec<Event> =
            serde_json::from_value(events["events"].clone()).map_err(|_| "Invalid CAD history")?;
        validate_references(turn, &current, &events)?;
        let mut errors: Vec<String> = vec![];
        // Each turn may execute multiple modeled operations with fresh kernel feedback.
        let max_steps = match std::env::var("CAD_AGENT_MAX_STEPS") {
            Ok(value) => value
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=128).contains(n))
                .ok_or("Invalid CAD_AGENT_MAX_STEPS (1–128)")?,
            Err(_) => 32,
        };
        for _ in 0..(max_steps + 4) {
            if current.revision != state.revision {
                return Err(
                    "Another client changed the model. Task stopped without overwriting it.".into(),
                );
            }
            state.phase = "thinking".into();
            state.text = format!(
                "Agent is working · {} operations completed",
                state.steps.len()
            );
            self.publish(key, state).await;
            let edges:Vec<_>=current.data.edges.iter().map(|e|serde_json::json!({"reference":e.reference,"length_mm":e.length,"points":e.points})).collect();
            let feedback = serde_json::json!({"operation_budget_remaining":max_steps-state.steps.len(),"current_revision":current.revision,"completed":state.steps,"errors":errors,"edges":edges,"instruction":"Select current edge IDs. Completed operations have already been applied. Return proposal:null when the goal is done. Never request user approval."});
            let reply = generate(turn, &current.data, feedback).await?;
            let latest = Self::scene(&mut client, turn.document.clone()).await?;
            if latest.revision != state.revision {
                return Err("Another client changed the model while Agent was thinking.".into());
            }
            if reply.failed {
                return Err(reply.text);
            }
            let Some(operation) = reply.proposal.clone() else {
                state.phase = "completed".into();
                state.text = reply.text;
                state.can_undo = !state.steps.is_empty();
                return Ok(());
            };
            if state.steps.len() >= max_steps {
                return Err("Agent operation budget reached before task completion".into());
            }
            let mut scope = turn.clone();
            scope.revision = current.revision;
            if !state.steps.is_empty()
                || !scope
                    .references
                    .iter()
                    .any(|r| r.kind == ReferenceKind::Edge)
            {
                scope.references = current
                    .data
                    .edges
                    .iter()
                    .map(|e| Reference {
                        name: e.reference.clone(),
                        document: turn.document.clone(),
                        revision: current.revision,
                        kind: ReferenceKind::Edge,
                        target: e.reference.clone(),
                    })
                    .collect();
            }
            scope.validate_reply(&reply)?;
            if let Proposal::Fillet { reference, .. } = &operation {
                if !current.data.edges.iter().any(|e| &e.reference == reference) {
                    return Err("Agent selected a non-current edge".into());
                }
            }
            let command = Self::command(&operation, &turn.document, current.revision)?;
            state.phase = "executing".into();
            state.text = reply.text;
            self.publish(key, state).await;
            let result = self.write(key, state, &mut client, command).await?;
            if !result.ok {
                let message = result
                    .error
                    .map(|e| e.message)
                    .unwrap_or_else(|| "Kernel rejected operation".into());
                errors.push(message);
                if errors.len() >= 3 {
                    return Err(format!(
                        "Kernel could not complete this task: {}",
                        errors.last().unwrap()
                    ));
                }
                current = Self::scene(&mut client, turn.document.clone()).await?;
                continue;
            }
            let doc: DocumentResult<DocumentState> =
                result.result_as().map_err(|_| "Invalid operation result")?;
            state.revision = doc.revision;
            state.steps.push(StepReceipt {
                operation,
                revision: doc.revision,
            });
            self.save(key, state, None)?;
            current = Self::scene(&mut client, turn.document.clone()).await?;
            if current.revision != state.revision {
                return Err("Model changed after an Agent operation".into());
            }
            if current.data.state.valid != Some(true) {
                return Err("Kernel validation failed after operation".into());
            }
        }
        Err("Agent reasoning budget reached before task completion".into())
    }
    async fn rollback(&self, key: &str, state: &mut TaskState) -> Result<(), String> {
        let mut client = self.client(key).await?;
        for _ in 0..state.steps.len() {
            let scene = Self::scene(&mut client, state.document.clone()).await?;
            if scene.revision != state.revision {
                return Err(
                    "Later edits prevent task Undo; no other client's edits were undone.".into(),
                );
            }
            let result = self
                .write(
                    key,
                    state,
                    &mut client,
                    Command::Undo {
                        document: state.document.clone(),
                        revision: state.revision,
                    },
                )
                .await?;
            if !result.ok {
                return Err("Task Undo rejected by kernel; inspect document history".into());
            }
            let doc: DocumentResult<DocumentState> =
                result.result_as().map_err(|_| "Invalid Undo result")?;
            state.revision = doc.revision;
        }
        state.can_undo = false;
        Ok(())
    }
    pub async fn undo(&self, owner: &str, id: &str) -> Result<TaskState, String> {
        let key = Self::key(owner, id)?;
        let mut jobs = self.jobs.lock().await;
        let mut state = if let Some(s) = jobs.get(&key).cloned() {
            s
        } else {
            drop(jobs);
            let s = self.status(owner, id).await?;
            jobs = self.jobs.lock().await;
            s
        };
        if state.phase == "undone" || state.phase == "undoing" {
            return Ok(state);
        }
        if !state.done || !state.can_undo {
            return Err("This task cannot currently be undone".into());
        }
        state.done = false;
        state.phase = "undoing".into();
        state.can_undo = false;
        self.save(&key, &state, None)?;
        jobs.insert(key.clone(), state.clone());
        drop(jobs);
        let host = self.clone();
        let initial = state.clone();
        tokio::spawn(async move {
            match host.rollback(&key, &mut state).await {
                Ok(()) => {
                    state.phase = "undone".into();
                    state.text = "已撤销本次 Agent 任务。".into();
                }
                Err(e) => {
                    if state.phase != "outcome_unknown" {
                        state.phase = "undo_failed".into();
                    }
                    state.text = e;
                }
            }
            state.done = true;
            if state.phase != "outcome_unknown" {
                let _ = host.save(&key, &state, None);
            }
            host.publish(&key, &state).await;
        });
        Ok(initial)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn host(root: PathBuf) -> Tasks {
        Tasks::new(
            root,
            cad_protocol::service::ServiceEndpoint {
                protocol: "1.0".into(),
                address: "127.0.0.1:1".parse().unwrap(),
                service_id: "unavailable".into(),
                token: "not-exported".into(),
            },
            "session".into(),
        )
    }
    fn state() -> TaskState {
        TaskState {
            id: "task-1".into(),
            document: DocumentId::new("doc").unwrap(),
            before: Revision::new(1).unwrap(),
            revision: Revision::new(1).unwrap(),
            phase: "executing".into(),
            text: "Working".into(),
            steps: vec![],
            can_undo: false,
            done: false,
        }
    }
    #[tokio::test]
    async fn interrupted_tasks_are_reported_without_replaying_and_are_owner_scoped() {
        let scratch = Scratch::new().unwrap();
        let h = host(scratch.0.join("tasks"));
        let key = Tasks::key("browser-1", "task-1").unwrap();
        h.save(&key, &state(), None).unwrap();
        let restarted = host(scratch.0.join("tasks"));
        let recovered = restarted.status("browser-1", "task-1").await.unwrap();
        assert_eq!(recovered.phase, "interrupted");
        assert!(recovered.done);
        assert!(!recovered.can_undo);
        assert!(restarted.status("other-browser", "task-1").await.is_err());
        assert!(restarted.status("browser-1", "../task-1").await.is_err());
        let turn = Turn {
            document: DocumentId::new("doc").unwrap(),
            revision: Revision::new(1).unwrap(),
            text: "Do not replay".into(),
            references: vec![],
            attachments: vec![],
            history: vec![],
        };
        let duplicate = restarted
            .start(
                "browser-1",
                TaskRequest {
                    id: "task-1".into(),
                    turn,
                },
            )
            .await
            .unwrap();
        assert_eq!(duplicate.phase, "interrupted");
    }
}
