//! Event-driven native protocol stand-in; this does not prove an installed Codex.
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use tokio::{
    net::UnixListener,
    task::{JoinHandle, JoinSet},
};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

pub(super) struct NativeFixture {
    pub export: Arc<codex_native_integration::NativeSchemaExport>,
    pub creates: Arc<Mutex<Vec<Value>>>,
    pub resumes: Arc<Mutex<Vec<Value>>>,
    pub cold: Arc<AtomicBool>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}
impl NativeFixture {
    pub async fn start(root: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let mut definitions = BTreeMap::new();
        for operation in [
            "ThreadRead",
            "ThreadResume",
            "ThreadStart",
            "ThreadFork",
            "ThreadLoadedList",
            "TurnStart",
            "TurnSteer",
            "TurnInterrupt",
            "ThreadTurnsList",
            "ThreadItemsList",
            "ThreadSetName",
        ] {
            definitions.insert(format!("{operation}Params"), json!({"type":"object"}));
            definitions.insert(format!("{operation}Response"), json!({"type":"object"}));
        }
        let schema = json!({"definitions":{"v2":definitions,"ServerRequest":{"type":"object"},"ServerNotification":{"type":"object"}}});
        let executable = root.join("native-schema-fixture");
        std::fs::write(&executable,format!("#!/bin/sh\ncat > \"$5/codex_app_server_protocol.schemas.json\" <<'SCHEMA'\n{schema}\nSCHEMA\n")).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let identity = codex_native_integration::executable_identity(&executable)
            .await
            .unwrap();
        let export = Arc::new(
            codex_native_integration::NativeSchemaExport::generate(
                &identity,
                &root.join("native-export"),
            )
            .await
            .unwrap(),
        );
        let listener = UnixListener::bind(root.join("backend.sock")).unwrap();
        let creates = Arc::new(Mutex::new(Vec::new()));
        let resumes = Arc::new(Mutex::new(Vec::new()));
        let cold = Arc::new(AtomicBool::new(false));
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let recorded = creates.clone();
        let recorded_resumes = resumes.clone();
        let read_cold = cold.clone();
        let ids = Arc::new(AtomicU64::new(1));
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _=cancellation.cancelled()=>break,
                    accepted=listener.accept()=>{
                        let (stream,_)=accepted.unwrap();
                        let recorded=recorded.clone(); let ids=ids.clone();
                        let recorded_resumes=recorded_resumes.clone(); let read_cold=read_cold.clone();
                        connections.spawn(async move {
                            let mut socket=tokio_tungstenite::accept_async(stream).await.unwrap();
                            while let Some(Ok(Message::Text(frame)))=socket.next().await {
                                let request:Value=serde_json::from_str(&frame).unwrap();
                                let Some(id)=request.get("id") else {continue;};
                                let method=request["method"].as_str().unwrap();
                                let result=match method {
                                    "initialize"=>json!({"userAgent":"fixture-native"}),
                                    "thread/loaded/list"|"thread/list"|"thread/turns/list"|"thread/items/list"=>json!({"data":[],"nextCursor":null}),
                                    "thread/start"|"thread/fork"=>{
                                        recorded.lock().unwrap().push(request.clone());
                                        let session=format!("runtime-native-{}",ids.fetch_add(1,Ordering::SeqCst));
                                        thread_result(&request,&session)
                                    },
                                    "thread/read"=>{
                                        let mut result=thread_result(&request,request["params"]["threadId"].as_str().unwrap());
                                        if read_cold.load(Ordering::SeqCst) { result["thread"]["status"]["type"]=json!("notLoaded"); }
                                        result
                                    },
                                    "thread/resume"=>{
                                        recorded_resumes.lock().unwrap().push(request.clone());
                                        thread_result(&request,request["params"]["threadId"].as_str().unwrap())
                                    },
                                    _=>json!({}),
                                };
                                if socket.send(Message::Text(json!({"id":id,"result":result}).to_string().into())).await.is_err(){break;}
                            }
                        });
                    }
                }
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        Self {
            export,
            creates,
            resumes,
            cold,
            stop,
            task,
        }
    }
    pub async fn shutdown(self) {
        self.stop.cancel();
        self.task.await.unwrap();
    }
}

fn thread_result(request: &Value, session: &str) -> Value {
    let params = &request["params"];
    let cwd = params.get("cwd").cloned().unwrap_or(Value::Null);
    let profile = params["permissions"].as_str();
    let active=profile.map(|profile|json!({"id":profile,"extends":params["config"][format!("permissions.{profile}.extends")]}));
    let roots = profile
        .and_then(|profile| params["config"].get(format!("permissions.{profile}.filesystem")))
        .and_then(Value::as_object)
        .map(|grants| {
            grants
                .iter()
                .filter(|(_, access)| access.as_str() == Some("write"))
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "cwd":cwd,"model":params["model"],"approvalPolicy":"on-request","approvalsReviewer":"user","activePermissionProfile":active,
        "sandbox":{"type":"workspaceWrite","writableRoots":roots,"networkAccess":true,"excludeTmpdirEnvVar":false,"excludeSlashTmp":false},
        "thread":{"id":session,"cwd":cwd,"status":{"type":"idle"},"turns":[]}
    })
}
