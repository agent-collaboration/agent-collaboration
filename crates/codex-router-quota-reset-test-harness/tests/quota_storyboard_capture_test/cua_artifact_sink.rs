//! Bounded original-byte export, not a renderer or product controller.
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path as RequestPath, State},
    http::{HeaderMap, StatusCode},
    response::Html,
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, Read as _, Write as _},
    net::{Ipv4Addr, SocketAddr},
    os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_FORM_BYTES: usize = 4 * MAX_IMAGE_BYTES.div_ceil(3) + 1024;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SceneName(String);

impl SceneName {
    fn parse(value: &str) -> Result<Self, ArtifactRejection> {
        if value.is_empty()
            || value.len() > 96
            || value.starts_with('-')
            || value.ends_with('-')
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(ArtifactRejection::Invalid);
        }
        Ok(Self(value.to_owned()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArtifactRejection {
    Invalid,
    Nonce,
    TooLarge,
    Duplicate,
    Storage,
}

impl ArtifactRejection {
    fn response(self) -> (StatusCode, Html<String>) {
        let (status, message) = match self {
            Self::Invalid => (
                StatusCode::BAD_REQUEST,
                "Invalid original-artifact submission",
            ),
            Self::Nonce => (StatusCode::FORBIDDEN, "Invalid export nonce"),
            Self::TooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Original artifact exceeds limit",
            ),
            Self::Duplicate => (StatusCode::CONFLICT, "Original artifact already exists"),
            Self::Storage => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Original artifact storage failed",
            ),
        };
        (status, Html(message.to_owned()))
    }
}

struct ArtifactStore {
    directory: PathBuf,
    nonce: String,
    scenes: BTreeSet<SceneName>,
    stored: AtomicUsize,
    failed: AtomicBool,
}

struct OriginalSubmission {
    scene: SceneName,
    bytes: Vec<u8>,
    encoded: String,
}

impl OriginalSubmission {
    fn parse(body: &[u8], store: &ArtifactStore) -> Result<Self, ArtifactRejection> {
        if body.len() > MAX_FORM_BYTES {
            return Err(ArtifactRejection::TooLarge);
        }
        let body = std::str::from_utf8(body).map_err(|_| ArtifactRejection::Invalid)?;
        let mut fields = BTreeMap::new();
        for line in body.lines() {
            let (name, value) = line.split_once('=').ok_or(ArtifactRejection::Invalid)?;
            if fields.insert(name, value).is_some() {
                return Err(ArtifactRejection::Invalid);
            }
        }
        if fields.len() != 4 {
            return Err(ArtifactRejection::Invalid);
        }
        let [Some(nonce), Some(scene), Some(length), Some(encoded)] =
            ["nonce", "scene", "length", "image"].map(|name| fields.get(name).copied())
        else {
            return Err(ArtifactRejection::Invalid);
        };
        if nonce != store.nonce {
            return Err(ArtifactRejection::Nonce);
        }
        let scene = SceneName::parse(scene)?;
        if !store.scenes.contains(&scene) {
            return Err(ArtifactRejection::Invalid);
        }
        let length: usize = length.parse().map_err(|_| ArtifactRejection::Invalid)?;
        if length > MAX_IMAGE_BYTES {
            return Err(ArtifactRejection::TooLarge);
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| ArtifactRejection::Invalid)?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(ArtifactRejection::TooLarge);
        }
        // Format sentinels only: these never judge pixels or scene correctness.
        if bytes.len() != length
            || !bytes.starts_with(&[0xff, 0xd8])
            || !bytes.ends_with(&[0xff, 0xd9])
            || STANDARD.encode(&bytes) != encoded
        {
            return Err(ArtifactRejection::Invalid);
        }
        Ok(Self {
            scene,
            bytes,
            encoded: encoded.to_owned(),
        })
    }
}

impl ArtifactStore {
    fn save(&self, submission: OriginalSubmission) -> Result<String, ArtifactRejection> {
        let filename = format!("{}.jpg", submission.scene.0);
        let path = self.directory.join(&filename);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    ArtifactRejection::Duplicate
                } else {
                    ArtifactRejection::Storage
                }
            })?;
        let result = (|| -> io::Result<String> {
            file.write_all(&submission.bytes)?;
            file.sync_all()?;
            drop(file);
            let stored = fs::read(&path)?;
            if stored != submission.bytes || STANDARD.encode(&stored) != submission.encoded {
                return Err(io::Error::other("original bytes changed during export"));
            }
            let hash = format!("{:x}", Sha256::digest(&stored));
            let receipt = format!(
                "{{\"kind\":\"original-cua-jpeg-bytes-only\",\"scene\":\"{}\",\"filename\":\"{filename}\",\"bytes\":{},\"sha256\":\"{hash}\",\"encoding_round_trip\":true}}\n",
                submission.scene.0,
                stored.len()
            );
            let mut receipt_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(
                    self.directory
                        .join(format!("{}.receipt.json", submission.scene.0)),
                )?;
            receipt_file.write_all(receipt.as_bytes())?;
            self.stored.fetch_add(1, Ordering::SeqCst);
            Ok(format!(
                "<!doctype html><title>Original artifact saved</title><h1>Original artifact saved</h1><pre>{receipt}</pre><p>Byte retention only; UI judgment remains separate.</p>"
            ))
        })();
        result.map_err(|_| {
            self.failed.store(true, Ordering::SeqCst);
            ArtifactRejection::Storage
        })
    }
}

async fn export_form(
    State(store): State<Arc<ArtifactStore>>,
    RequestPath((nonce, scene)): RequestPath<(String, String)>,
) -> (StatusCode, Html<String>) {
    let Ok(scene) = SceneName::parse(&scene) else {
        return ArtifactRejection::Invalid.response();
    };
    if nonce != store.nonce {
        return ArtifactRejection::Nonce.response();
    }
    if !store.scenes.contains(&scene) {
        return ArtifactRejection::Invalid.response();
    }
    (
        StatusCode::OK,
        Html(format!(
            "<!doctype html><title>Original artifact export</title><h1>Original artifact export</h1><p>Scene: {}</p><form method=post action=/save enctype=text/plain><input type=hidden name=nonce value=\"{}\"><input type=hidden name=scene value=\"{}\"><label for=length>Original byte length</label><input id=length name=length><label for=image>Original JPEG base64</label><textarea id=image name=image></textarea><button type=submit>Save original</button></form>",
            scene.0, store.nonce, scene.0
        )),
    )
}

async fn save_original(
    State(store): State<Arc<ArtifactStore>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Html<String>) {
    if !headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(';').next() == Some("text/plain"))
    {
        return ArtifactRejection::Invalid.response();
    }
    let submission = match OriginalSubmission::parse(&body, &store) {
        Ok(value) => value,
        Err(error) => return error.response(),
    };
    match tokio::task::spawn_blocking(move || store.save(submission)).await {
        Ok(Ok(page)) => (StatusCode::CREATED, Html(page)),
        Ok(Err(error)) => error.response(),
        Err(_) => ArtifactRejection::Storage.response(),
    }
}

pub(super) struct CuaArtifactSink {
    address: SocketAddr,
    store: Arc<ArtifactStore>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<io::Result<()>>>,
}

impl CuaArtifactSink {
    pub(super) async fn start(output: &Path, names: Vec<String>) -> io::Result<Self> {
        let scenes = names
            .iter()
            .map(|name| SceneName::parse(name))
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(|_| io::Error::other("invalid declared artifact scene"))?;
        if names.is_empty() || names.len() > 128 || scenes.len() != names.len() {
            return Err(io::Error::other(
                "artifact scenes must be nonempty, unique and bounded",
            ));
        }
        let directory = fs::canonicalize(output)?.join("original-images");
        fs::create_dir(&directory)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        let mut random = [0u8; 32];
        fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
        let nonce = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let store = Arc::new(ArtifactStore {
            directory,
            nonce,
            scenes,
            stored: AtomicUsize::new(0),
            failed: AtomicBool::new(false),
        });
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let router = Router::new()
            .route("/export/{nonce}/{scene}", get(export_form))
            .route("/save", post(save_original))
            .layer(DefaultBodyLimit::max(MAX_FORM_BYTES))
            .with_state(store.clone());
        let (shutdown, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        Ok(Self {
            address,
            store,
            shutdown: Some(shutdown),
            task: Some(task),
        })
    }

    pub(super) fn readiness_json(&self) -> String {
        let scenes = self
            .store
            .scenes
            .iter()
            .map(|scene| super::encode_json_string(&scene.0))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"base_url\":{},\"directory\":{},\"scenes\":[{scenes}],\"maximum_bytes\":{MAX_IMAGE_BYTES},\"mime_type\":\"image/jpeg\",\"source_sha256\":\"{:x}\"}}",
            super::encode_json_string(&format!(
                "http://{}/export/{}/",
                self.address, self.store.nonce
            )),
            super::encode_json_string(&self.store.directory.to_string_lossy()),
            Sha256::digest(include_bytes!("cua_artifact_sink.rs"))
        )
    }

    pub(super) async fn stop(mut self) -> io::Result<usize> {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        let mut task = self
            .task
            .take()
            .ok_or_else(|| io::Error::other("artifact sink task missing"))?;
        match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
            Ok(result) => result.map_err(io::Error::other)??,
            Err(_) => {
                task.abort();
                let _ = task.await;
                return Err(io::Error::other("artifact sink did not stop"));
            }
        }
        if self.store.failed.load(Ordering::SeqCst) {
            return Err(io::Error::other("artifact sink storage failed"));
        }
        Ok(self.store.stored.load(Ordering::SeqCst))
    }
}

impl Drop for CuaArtifactSink {
    fn drop(&mut self) {
        if let Some(sender) = self.shutdown.take() {
            let _ = sender.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::TcpStream,
    };

    // Synthetic protocol bytes only; no decoded/rendered image is asserted.
    const ORIGINAL: &[u8] = &[0xff, 0xd8, 0x00, 0x01, 0x7f, 0x80, 0xfe, 0xff, 0xd9];

    fn output() -> PathBuf {
        // Parallel tests can read the same coarse clock value; the counter
        // keeps each owned directory unique within this process.
        static NEXT_OUTPUT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "cua-artifact-test-{}-{}-{}",
            std::process::id(),
            NEXT_OUTPUT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn body(sink: &CuaArtifactSink) -> String {
        format!(
            "nonce={}\r\nscene=quota-options-120x36\r\nlength={}\r\nimage={}\r\n",
            sink.store.nonce,
            ORIGINAL.len(),
            STANDARD.encode(ORIGINAL)
        )
    }

    async fn exchange(address: SocketAddr, method: &str, path: &str, body: &str) -> String {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            let mut response = Vec::new(); stream.read_to_end(&mut response).await.unwrap(); String::from_utf8(response).unwrap()
        }).await.expect("bounded artifact HTTP exchange")
    }

    #[tokio::test]
    async fn original_artifact_http_round_trip_is_exact_and_duplicate_never_overwrites() {
        let root = output();
        let sink = CuaArtifactSink::start(&root, vec!["quota-options-120x36".to_owned()])
            .await
            .unwrap();
        let page = exchange(
            sink.address,
            "GET",
            &format!("/export/{}/quota-options-120x36", sink.store.nonce),
            "",
        )
        .await;
        assert!(
            page.starts_with("HTTP/1.1 200")
                && page.contains("Save original")
                && page.contains("enctype=text/plain")
        );
        let payload = body(&sink);
        let response = exchange(sink.address, "POST", "/save", &payload).await;
        assert!(
            response.starts_with("HTTP/1.1 201") && response.contains("encoding_round_trip"),
            "{response}"
        );
        let image = sink.store.directory.join("quota-options-120x36.jpg");
        assert_eq!(fs::read(&image).unwrap(), ORIGINAL);
        let second = exchange(sink.address, "POST", "/save", &payload).await;
        assert!(second.starts_with("HTTP/1.1 409"), "{second}");
        assert_eq!(fs::read(image).unwrap(), ORIGINAL);
        let address = sink.address;
        let nonce = sink.store.nonce.clone();
        let export_path = format!("/export/{nonce}/quota-options-120x36");
        assert_eq!(sink.stop().await.unwrap(), 1);
        if let Ok(mut stream) = TcpStream::connect(address).await {
            // Retain who answered the freed address before failing: the stopped
            // sink's own export route, or another listener on a reused port.
            let request = format!(
                "GET {export_path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
            );
            let mut response = Vec::new();
            let outcome = tokio::time::timeout(Duration::from_secs(2), async {
                stream.write_all(request.as_bytes()).await?;
                stream.read_to_end(&mut response).await
            })
            .await;
            let text = String::from_utf8_lossy(&response);
            let served_own_export = text.starts_with("HTTP/1.1 200")
                && text.contains("Save original")
                && text.contains(&format!("<input type=hidden name=nonce value=\"{nonce}\">"));
            let head = text.chars().take(200).collect::<String>();
            panic!(
                "export listener must stop with lease: {address} still accepted; old nonce route {outcome:?}; served_own_export={served_own_export}: {head:?}"
            );
        }
    }

    #[tokio::test]
    async fn original_artifact_http_rejects_nonce_path_payload_size_and_unknown_fields() {
        let root = output();
        let sink = CuaArtifactSink::start(&root, vec!["quota-options-120x36".to_owned()])
            .await
            .unwrap();
        let valid = body(&sink);
        let cases = [
            (valid.replace(&sink.store.nonce, "wrong"), "403"),
            (valid.replace("quota-options-120x36", "../escape"), "400"),
            (
                valid.replace("quota-options-120x36", "quota-undeclared"),
                "400",
            ),
            (
                valid.replace(&STANDARD.encode(ORIGINAL), "invalid%%%"),
                "400",
            ),
            (valid.replace("length=9", "length=8"), "400"),
            (
                valid.replace("length=9", &format!("length={}", MAX_IMAGE_BYTES + 1)),
                "413",
            ),
            (format!("{valid}path=/outside.jpg\r\n"), "400"),
            (format!("{valid}scene=quota-options-120x36\r\n"), "400"),
        ];
        for (payload, status) in cases {
            let response = exchange(sink.address, "POST", "/save", &payload).await;
            assert!(
                response.starts_with(&format!("HTTP/1.1 {status}")),
                "{response}"
            );
            assert_eq!(fs::read_dir(&sink.store.directory).unwrap().count(), 0);
        }
        assert_eq!(sink.stop().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn original_artifact_declarations_reject_duplicate_or_unsafe_scene_names() {
        for names in [
            vec![],
            vec!["../escape".to_owned()],
            vec!["quota".to_owned(), "quota".to_owned()],
        ] {
            let root = output();
            assert!(CuaArtifactSink::start(&root, names).await.is_err());
            assert!(!root.join("original-images").exists());
        }
    }
}
