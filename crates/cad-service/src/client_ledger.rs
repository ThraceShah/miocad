//! Overflow clients use the service's existing kernel channel, with persistent logical receipts.
//! This lets a relay repair an already-full legacy worker without restarting its documents.
use super::*;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
struct Entry {
    request: Value,
    upstream: Option<Value>,
    response: Option<Value>,
}
#[derive(Default, Serialize, Deserialize)]
struct Ledger {
    water: u64,
    entries: BTreeMap<u64, Entry>,
}
pub(super) struct Clients {
    root: PathBuf,
}
impl Clients {
    pub fn new(root: PathBuf, context: &Value) -> Self {
        Self {
            root: root.join(context["worker_id"].as_str().unwrap_or("unknown")),
        }
    }
    fn path(&self, client: &str) -> PathBuf {
        let encoded: String = client.bytes().map(|b| format!("{b:02x}")).collect();
        let mut p = self.root.clone();
        for part in encoded.as_bytes().chunks(120) {
            p.push(std::str::from_utf8(part).unwrap());
        }
        p.join("ledger.json")
    }
    fn save(&self, client: &str, ledger: &mut Ledger) -> Result<(), String> {
        // Keep high-water even when bounded outcomes expire, so old writes never execute again.
        while ledger.entries.len() > 64 {
            ledger.entries.pop_first();
        }
        let mut bytes = serde_json::to_vec(ledger).map_err(|e| e.to_string())?;
        while bytes.len() > 16 * 1024 * 1024 && !ledger.entries.is_empty() {
            ledger.entries.pop_first();
            bytes = serde_json::to_vec(ledger).map_err(|e| e.to_string())?;
        }
        let path = self.path(client);
        let parent = path.parent().ok_or("invalid ledger path")?;
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, path).map_err(|e| e.to_string())
    }
    fn base(worker: &Worker, req: &Value) -> Value {
        let mut value = worker.context.clone();
        let o = value.as_object_mut().unwrap();
        o.remove("result");
        o.remove("error");
        value["client_id"] = req["client_id"].clone();
        value["request_id"] = req["request_id"].clone();
        value["events"] = json!([]);
        value["replayed"] = json!(false);
        value
    }
    fn error(worker: &Worker, req: &Value, kind: &str, message: &str) -> Value {
        let mut v = Self::base(worker, req);
        v["ok"] = json!(false);
        v["error"] = json!({"kind":kind,"message":message,"retryable":false});
        v
    }
    fn rewrite(mut value: Value, req: &Value) -> Value {
        value["client_id"] = req["client_id"].clone();
        value["request_id"] = req["request_id"].clone();
        if let Some(events) = value["events"].as_array_mut() {
            for e in events {
                e["client_id"] = req["client_id"].clone();
                e["request_id"] = req["request_id"].clone();
            }
        }
        value
    }
    async fn recover(worker: &mut Worker, entry: &Entry) -> Result<Option<Value>, String> {
        if let Some(value) = &entry.response {
            return Ok(Some(value.clone()));
        }
        let Some(upstream) = &entry.upstream else {
            return Ok(None);
        };
        worker.internal_id += 1;
        let status = json!({"protocol":worker.context["protocol"],"method":"request.status","client_id":worker.observer,"request_id":worker.internal_id,"worker_id":worker.context["worker_id"],"session_id":worker.context["session_id"],"target_request_id":upstream["request_id"]});
        let response = worker.exchange(&status).await?;
        if response["result"]["status"] == "completed" {
            return Ok(Some(Self::rewrite(
                response["result"]["response"].clone(),
                &entry.request,
            )));
        }
        Ok(None)
    }
    pub async fn exchange(&self, worker: &mut Worker, req: &Value) -> Result<Value, String> {
        if req["protocol"] != worker.context["protocol"] {
            return Ok(Self::error(
                worker,
                req,
                "version_mismatch",
                "Unsupported protocol version",
            ));
        }
        let client = req["client_id"].as_str().ok_or("missing client")?;
        if client == worker.observer {
            return Err("reserved_client_id".into());
        }
        let path = self.path(client);
        let mut ledger = if path.exists() {
            Some(
                serde_json::from_slice::<Ledger>(&std::fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        if ledger.is_none() {
            let direct = worker.exchange(req).await?;
            if req["method"] != "hello" || direct["error"]["kind"] != "client_limit" {
                return Ok(direct);
            }
            ledger = Some(Ledger::default());
        }
        let mut ledger = ledger.unwrap();
        if req["method"] == "hello" {
            self.save(client, &mut ledger)?;
            let mut hello = Self::rewrite(worker.context.clone(), req);
            hello["result"]["last_request_id"] = json!(ledger.water);
            return Ok(hello);
        }
        if req["worker_id"] != worker.context["worker_id"] {
            return Ok(Self::error(
                worker,
                req,
                "wrong_instance",
                "Worker changed; never replay",
            ));
        }
        if req["session_id"] != worker.context["session_id"] {
            return Ok(Self::error(
                worker,
                req,
                "wrong_session",
                "Unknown worker session",
            ));
        }
        let id = req["request_id"].as_u64().ok_or("missing request ID")?;
        if id == 0 {
            return Ok(Self::error(
                worker,
                req,
                "invalid_request",
                "request_id zero is reserved for hello",
            ));
        }
        if let Some(entry) = ledger.entries.get(&id) {
            if entry.request != *req {
                return Ok(Self::error(
                    worker,
                    req,
                    "request_conflict",
                    "request ID reused with different content",
                ));
            }
            let result = Self::recover(worker, entry).await?;
            if let Some(mut response) = result {
                if let Some(entry) = ledger.entries.get_mut(&id) {
                    entry.response = Some(response.clone());
                }
                self.save(client, &mut ledger)?;
                response["replayed"] = json!(true);
                return Ok(response);
            }
            return Ok(Self::error(
                worker,
                req,
                "outcome_unavailable",
                "Accepted result unavailable; request will not run again",
            ));
        }
        if id <= ledger.water {
            return Ok(Self::error(
                worker,
                req,
                "outcome_unavailable",
                "Accepted result unavailable; request will not run again",
            ));
        }
        ledger.water = id;
        if req["method"] == "request.status" {
            ledger.entries.insert(
                id,
                Entry {
                    request: req.clone(),
                    upstream: None,
                    response: None,
                },
            );
            self.save(client, &mut ledger)?;
            let target = req["target_request_id"]
                .as_u64()
                .ok_or("missing status target")?;
            let response = if target >= id {
                Self::error(
                    worker,
                    req,
                    "invalid_params",
                    "status target must precede request",
                )
            } else {
                let outcome = if let Some(entry) = ledger
                    .entries
                    .get(&target)
                    .filter(|e| e.request["method"] == "execute")
                {
                    Self::recover(worker, entry).await?
                } else {
                    None
                };
                let mut r = Self::base(worker, req);
                r["ok"] = json!(true);
                r["result"] = if let Some(outcome) =
                    outcome.filter(|v| v["error"]["kind"] != "outcome_unknown")
                {
                    if let Some(entry) = ledger.entries.get_mut(&target) {
                        entry.response = Some(outcome.clone());
                    }
                    json!({"status":"completed","response":outcome})
                } else {
                    json!({"status":"outcome_unavailable"})
                };
                r
            };
            ledger.entries.insert(
                id,
                Entry {
                    request: req.clone(),
                    upstream: None,
                    response: Some(response.clone()),
                },
            );
            self.save(client, &mut ledger)?;
            return Ok(response);
        }
        worker.internal_id += 1;
        let mut translated = req.clone();
        translated["client_id"] = json!(worker.observer);
        translated["request_id"] = json!(worker.internal_id);
        ledger.entries.insert(
            id,
            Entry {
                request: req.clone(),
                upstream: Some(translated.clone()),
                response: None,
            },
        );
        self.save(client, &mut ledger)?; // Write-ahead reservation before any kernel side effect.
        let response = Self::rewrite(worker.exchange(&translated).await?, req);
        if let Some(entry) = ledger.entries.get_mut(&id) {
            entry.response = Some(response.clone());
        }
        self.save(client, &mut ledger)?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn overflow_clients_survive_transport_reload_and_never_repeat_writes() {
        let root =
            std::env::temp_dir().join(format!("cad-client-overflow-{}", unique_id().unwrap()));
        let binary = std::env::var_os("CAD_SERVER")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build/cad-server")
            });
        let mut worker = Worker::start(&binary, root.clone()).await.unwrap();
        let mut hello = Value::Null;
        for i in 0..140 {
            hello=worker.client_exchange(&json!({"protocol":"1.0","method":"hello","client_id":format!("client-{i}"),"request_id":0})).await.unwrap();
            assert_eq!(hello["ok"], true, "{hello}");
        }
        let req = json!({"protocol":"1.0","method":"execute","client_id":"client-139","request_id":1,"worker_id":hello["worker_id"],"session_id":hello["session_id"],"command":{"op":"document.create"}});
        let first = worker.client_exchange(&req).await.unwrap();
        assert_eq!(first["ok"], true, "{first}");
        assert_eq!(first["client_id"], "client-139");
        assert_eq!(first["events"][0]["client_id"], "client-139");
        let clients = worker.clients.as_ref().unwrap();
        let path = clients.path("client-139");
        let mut ledger: Ledger = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        ledger.entries.get_mut(&1).unwrap().response = None;
        clients.save("client-139", &mut ledger).unwrap();
        // Simulate a relay restart after an upstream ACK was lost.
        worker.clients = Some(Clients::new(root.clone(), &worker.context));
        let repeat = worker.client_exchange(&req).await.unwrap();
        assert_eq!(repeat["replayed"], true);
        assert_eq!(repeat["result"], first["result"]);
        let mut conflict = req.clone();
        conflict["command"] = json!({"op":"capabilities"});
        assert_eq!(
            worker.client_exchange(&conflict).await.unwrap()["error"]["kind"],
            "request_conflict"
        );
        let status = json!({"protocol":"1.0","method":"request.status","client_id":"client-139","request_id":2,"worker_id":hello["worker_id"],"session_id":hello["session_id"],"target_request_id":1});
        let result = worker.client_exchange(&status).await.unwrap();
        assert_eq!(result["result"]["status"], "completed");
        assert_eq!(result["result"]["response"]["client_id"], "client-139");
        assert_eq!(
            worker.snapshot().await.unwrap()["documents"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        drop(worker);
        let _ = std::fs::remove_dir_all(root);
    }
}
