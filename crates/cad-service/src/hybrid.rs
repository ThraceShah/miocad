//! During a live kernel upgrade, retain the legacy owner and its in-memory undo
//! history. New documents live in a persistent current-version worker, behind the
//! same session and service boundary. No legacy document is silently converted.
use super::*;
fn context(value: &mut Value, external: &Value) {
    value["worker_id"] = external["worker_id"].clone();
    value["session_id"] = external["session_id"].clone();
    if let Some(events) = value["events"].as_array_mut() {
        for e in events {
            e["worker_id"] = external["worker_id"].clone();
            e["session_id"] = external["session_id"].clone();
        }
    }
    if let Some(response) = value.get_mut("result").and_then(|r| r.get_mut("response")) {
        context(response, external);
    }
}
impl Worker {
    pub(super) async fn attach_current(
        &mut self,
        binary: PathBuf,
        store: PathBuf,
        ledger: PathBuf,
    ) -> Result<(), String> {
        let mut current = Box::new(Worker::start(&binary, ledger, store.clone()).await?);
        let hello = json!({"protocol":self.context["protocol"],"method":"hello","request_id":0,"client_id":self.observer});
        let reply = Box::pin(current.exchange(&hello)).await?;
        if reply["ok"] != true {
            return Err("current worker handshake failed".into());
        }
        let snapshot = Box::pin(current.snapshot()).await?;
        for doc in snapshot["documents"]
            .as_array()
            .ok_or("invalid current document list")?
        {
            current
                .docs
                .insert(doc["document"].as_str().ok_or("missing document")?.into());
        }
        let water = store.join("relay-water");
        std::fs::create_dir_all(&store).map_err(|e| e.to_string())?;
        let saved = std::fs::read_to_string(&water)
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        self.internal_id = self.internal_id.max(saved);
        self.hybrid_water = Some(water);
        self.current = Some(current);
        Ok(())
    }
    pub(super) async fn routed(&mut self, req: &Value) -> Result<(Value, bool), String> {
        if let Some(water) = &self.hybrid_water {
            let id = req["request_id"]
                .as_u64()
                .unwrap_or(0)
                .max(self.internal_id);
            let temp = water.with_extension("tmp");
            std::fs::write(&temp, id.to_string()).map_err(|e| e.to_string())?;
            std::fs::OpenOptions::new()
                .read(true)
                .open(&temp)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())?;
            std::fs::rename(temp, water).map_err(|e| e.to_string())?;
        }
        let op = req["command"]["op"].as_str().unwrap_or("");
        let doc = req["command"]["document"].as_str().unwrap_or("");
        let use_current = self.current.as_ref().is_some_and(|w| {
            w.docs.contains(doc)
                || doc.starts_with("tx.")
                || matches!(
                    op,
                    "document.create" | "document.open" | "commands.list" | "capabilities"
                )
        });
        if self.current.is_some()
            && (use_current || op == "document.list" || req["method"] == "request.status")
        {
            let current = self.current.as_mut().unwrap();
            let mut internal = req.clone();
            internal["worker_id"] = current.context["worker_id"].clone();
            internal["session_id"] = current.context["session_id"].clone();
            let mut value = Box::pin(current.exchange(&internal)).await?;
            context(&mut value, &self.context);
            if use_current
                || (req["method"] == "request.status" && value["result"]["status"] == "completed")
            {
                return Ok((value, true));
            }
            if op == "document.list" {
                let mut legacy = cad_client::SharedClient::control(
                    self.upstream.as_ref().unwrap(),
                    ServiceRequest::Rpc {
                        session: self.context["session_id"].as_str().map(str::to_owned),
                        request: req.clone(),
                    },
                )
                .await
                .map_err(|e| e.to_string())?;
                if legacy["ok"] != true || value["ok"] != true {
                    return Err("combined document list failed".into());
                }
                legacy["result"]
                    .as_array_mut()
                    .ok_or("invalid legacy list")?
                    .extend(
                        value["result"]
                            .as_array()
                            .ok_or("invalid current list")?
                            .iter()
                            .cloned(),
                    );
                return Ok((legacy, false));
            }
        }
        let value = cad_client::SharedClient::control(
            self.upstream.as_ref().unwrap(),
            ServiceRequest::Rpc {
                session: self.context["session_id"].as_str().map(str::to_owned),
                request: req.clone(),
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok((value, false))
    }
    pub(super) async fn sync_remote_events(&mut self) -> Result<(), String> {
        let Some(upstream) = &self.upstream else {
            return Ok(());
        };
        let result = cad_client::SharedClient::control(
            upstream,
            ServiceRequest::Events {
                session: self.context["session_id"]
                    .as_str()
                    .ok_or("missing session")?
                    .into(),
                after: self.remote_cursor,
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        if result["resync_required"] == true {
            self.cursor += 1;
            self.events.clear();
        }
        if let Some(events) = result["events"].as_array() {
            for event in events {
                self.cursor += 1;
                let mut e = event.clone();
                e["event_id"] = json!(self.cursor);
                self.docs
                    .insert(e["document"].as_str().unwrap_or("").into());
                self.events.push_back(e);
                while self.events.len() > EVENT_LIMIT {
                    self.events.pop_front();
                }
            }
        }
        self.remote_cursor = result["cursor"].as_u64().ok_or("invalid upstream cursor")?;
        Ok(())
    }
}
