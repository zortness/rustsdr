//! Versioned, independently owned settings sections. No engine or UI dependency.
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread::JoinHandle,
    time::Duration,
};

pub fn default_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("RUSTSDR_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("settings.json"));
    }
    directories::ProjectDirs::from("", "", "RustSDR")
        .map(|dirs| dirs.config_dir().join("settings.json"))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document(Value);
impl Default for Document {
    fn default() -> Self {
        Self(json!({"version": 1, "sections": {}}))
    }
}
impl Document {
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, String> {
        let Some(section) = self.0["sections"].get(key) else {
            return Ok(None);
        };
        if section["version"] != 1 {
            return Err(format!(
                "Settings section {key}: unsupported version; preserved unchanged"
            ));
        }
        serde_json::from_value(section["data"].clone())
            .map(Some)
            .map_err(|error| format!("Settings section {key}: {error}"))
    }
    pub fn put<T: Serialize>(&mut self, key: &str, value: &T) {
        let sections = self.0["sections"]
            .as_object_mut()
            .expect("validated sections");
        if sections.get(key).is_some_and(|v| v["version"] != 1) {
            return;
        }
        let section = sections
            .entry(key)
            .or_insert_with(|| json!({"version":1,"data":{}}));
        merge(
            &mut section["data"],
            serde_json::to_value(value).expect("settings serialize"),
        );
    }
}
fn merge(old: &mut Value, new: Value) {
    if let (Some(old), Some(new)) = (old.as_object_mut(), new.as_object()) {
        for (key, value) in new {
            merge(old.entry(key).or_insert(Value::Null), value.clone());
        }
    } else {
        *old = new;
    }
}
fn read(path: &Path) -> Result<Document, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Document::default()),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("Settings exceed 1 MiB".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if value["version"] != 1 || !value["sections"].is_object() {
        return Err("Unsupported settings document version or invalid sections".into());
    }
    Ok(Document(value))
}
fn write(path: &Path, document: &Document) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)?;
        serde_json::to_writer_pretty(&mut file, &document.0)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result.map_err(|e| e.to_string())
}

/// A single writer, with bounded coalescing and a final flush on orderly shutdown.
/// A corrupt/future document or another running instance makes this store read-only.
pub struct Store {
    pub document: Document,
    sender: Option<mpsc::SyncSender<Document>>,
    worker: Option<JoinHandle<()>>,
    queued: Option<Document>,
    error: Arc<Mutex<Option<String>>>,
}
impl Store {
    pub fn open(path: PathBuf) -> Self {
        let mut store = Self {
            document: Document::default(),
            sender: None,
            worker: None,
            queued: None,
            error: Arc::default(),
        };
        let result = (|| -> Result<(), String> {
            fs::create_dir_all(path.parent().ok_or("Settings path has no parent")?)
                .map_err(|e| e.to_string())?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path.with_extension("lock"))
                .map_err(|e| e.to_string())?;
            // Load even when another instance owns the writer lock.
            store.document = read(&path)?;
            lock.try_lock()
                .map_err(|e| format!("Another instance owns the settings writer: {e}"))?;
            let (sender, receiver) = mpsc::sync_channel::<Document>(1);
            let error = store.error.clone();
            let worker = std::thread::Builder::new()
                .name("settings-writer".into())
                .spawn(move || {
                    let _lock = lock;
                    while let Ok(mut document) = receiver.recv() {
                        let started = std::time::Instant::now();
                        let disconnected = loop {
                            if started.elapsed() >= Duration::from_secs(2) {
                                break false;
                            }
                            match receiver.recv_timeout(Duration::from_millis(500)) {
                                Ok(next) => document = next,
                                Err(mpsc::RecvTimeoutError::Timeout) => break false,
                                Err(mpsc::RecvTimeoutError::Disconnected) => break true,
                            }
                        };
                        *error.lock().unwrap() = write(&path, &document)
                            .err()
                            .map(|e| format!("Cannot save settings: {e}"));
                        if disconnected {
                            break;
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            store.sender = Some(sender);
            store.worker = Some(worker);
            Ok(())
        })();
        if let Err(error) = result {
            *store.error.lock().unwrap() = Some(format!("Settings are read-only: {error}"));
        }
        store
    }
    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }
    pub fn save(&mut self, document: Document) {
        if self.queued.as_ref() == Some(&document) {
            return;
        }
        self.document = document.clone();
        if let Some(sender) = &self.sender
            && sender.try_send(document.clone()).is_ok()
        {
            self.queued = Some(document);
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(self.document.clone());
            drop(sender);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    struct TestPath(PathBuf);
    impl TestPath {
        fn new() -> Self {
            static SERIAL: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir()
                .join(format!(
                    "rustsdr-settings-{}-{}",
                    std::process::id(),
                    SERIAL.fetch_add(1, Ordering::Relaxed)
                ))
                .join("settings.json");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            Self(path)
        }
    }
    impl Drop for TestPath {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.0.parent().unwrap());
        }
    }
    #[test]
    fn shutdown_flushes_latest_and_replaces_existing_file() {
        let path = TestPath::new();
        for pass in 0..2 {
            let mut store = Store::open(path.0.clone());
            assert_eq!(store.error(), None);
            for n in 0..100 {
                let mut doc = store.document.clone();
                doc.put("modes.cw", &json!({"wpm":n+100*pass}));
                store.save(doc);
            }
            drop(store);
            assert_eq!(
                read(&path.0)
                    .unwrap()
                    .get::<Value>("modes.cw")
                    .unwrap()
                    .unwrap()["wpm"],
                99 + 100 * pass
            );
        }
    }
    #[test]
    fn autosaves_without_shutdown() {
        let path = TestPath::new();
        let mut store = Store::open(path.0.clone());
        let mut doc = Document::default();
        doc.put("test", &42);
        store.save(doc);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !path.0.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(read(&path.0).unwrap().get::<u32>("test").unwrap(), Some(42));
    }
    #[test]
    fn preserves_unknown_fields_and_future_sections() {
        let mut doc = Document(
            json!({"version":1,"future_root":true,"sections":{"modes.cw":{"version":1,"data":{"speed":20,"future":7}},"modes.ft8":{"version":2,"data":{"x":1}}}}),
        );
        doc.put("modes.cw", &json!({"speed":25}));
        doc.put("modes.ft8", &json!({"x":2}));
        assert_eq!(
            doc.0["sections"]["modes.cw"]["data"],
            json!({"speed":25,"future":7})
        );
        assert_eq!(doc.0["sections"]["modes.ft8"]["data"]["x"], 1);
        assert!(doc.get::<Value>("modes.ft8").is_err());
        assert_eq!(doc.0["future_root"], true);
    }
    #[test]
    fn corrupt_and_future_files_are_not_overwritten() {
        for content in ["{broken", r#"{"version":99,"sections":{}}"#] {
            let path = TestPath::new();
            fs::write(&path.0, content).unwrap();
            let mut store = Store::open(path.0.clone());
            assert!(store.error().unwrap().contains("read-only"));
            store.save(Document::default());
            drop(store);
            assert_eq!(fs::read_to_string(&path.0).unwrap(), content);
        }
    }
    #[test]
    fn second_instance_cannot_overwrite_first() {
        let path = TestPath::new();
        let mut first = Store::open(path.0.clone());
        let mut second = Store::open(path.0.clone());
        assert!(second.error().unwrap().contains("writer"));
        let mut doc = Document::default();
        doc.put("owner", &1);
        first.save(doc);
        let mut doc = Document::default();
        doc.put("owner", &2);
        second.save(doc);
        drop(first);
        drop(second);
        assert_eq!(read(&path.0).unwrap().get::<u32>("owner").unwrap(), Some(1));
    }
    #[test]
    fn failed_replace_keeps_original_and_reports_error() {
        let path = TestPath::new();
        fs::create_dir(&path.0).unwrap();
        assert!(write(&path.0, &Document::default()).is_err());
        assert!(path.0.is_dir());
    }
}
