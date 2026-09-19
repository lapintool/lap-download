use super::engine::{DownloadOptions, ProgressEvent, download_file};
use crate::json_util::{read_json, write_json};
use crate::paths;
use crate::settings::load_settings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Queued,
    Running,
    Completed,
    Skipped,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub url: String,
    pub save_path: String,
    #[serde(default)]
    pub sha256: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub downloaded: u64,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub speed_bps: u64,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TaskStore {
    tasks: Vec<TaskRecord>,
}

pub struct TaskManager {
    inner: Mutex<Inner>,
}

struct Inner {
    tasks: Vec<TaskRecord>,
    cancel_flags: HashMap<String, Arc<AtomicBool>>,
    runner_started: bool,
}

impl TaskManager {
    pub fn new() -> Self {
        let store: TaskStore = read_json(&paths::tasks_path());
        // Reset interrupted running tasks to queued so they can resume.
        let tasks = store
            .tasks
            .into_iter()
            .map(|mut t| {
                if t.status == TaskStatus::Running {
                    t.status = TaskStatus::Queued;
                    t.message = "interrupted".into();
                    t.speed_bps = 0;
                }
                t
            })
            .collect();
        Self {
            inner: Mutex::new(Inner {
                tasks,
                cancel_flags: HashMap::new(),
                runner_started: false,
            }),
        }
    }

    pub async fn list(&self) -> Vec<TaskRecord> {
        let mut tasks = self.inner.lock().await.tasks.clone();
        for task in &mut tasks {
            if !task.save_path.is_empty() {
                task.save_path = paths::display_windows_path(&task.save_path);
            }
        }
        tasks
    }

    pub async fn add_urls(&self, urls: Vec<String>) -> Result<Vec<TaskRecord>, String> {
        let mut guard = self.inner.lock().await;
        let mut added = Vec::new();
        for raw in urls {
            for line in raw.lines() {
                let url = line.trim();
                if url.is_empty() || url.starts_with('#') {
                    continue;
                }
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Err(format!("invalid url: {url}"));
                }
                if guard.tasks.iter().any(|t| {
                    t.url == url
                        && matches!(
                            t.status,
                            TaskStatus::Queued
                                | TaskStatus::Running
                                | TaskStatus::Completed
                                | TaskStatus::Skipped
                        )
                }) {
                    continue;
                }
                let task = TaskRecord {
                    id: Uuid::new_v4().to_string(),
                    url: url.to_string(),
                    save_path: String::new(),
                    sha256: String::new(),
                    status: TaskStatus::Queued,
                    downloaded: 0,
                    total: 0,
                    speed_bps: 0,
                    message: String::new(),
                    error: String::new(),
                };
                guard.tasks.insert(0, task.clone());
                added.push(task);
            }
        }
        persist(&guard.tasks)?;
        Ok(added)
    }

    pub async fn remove(&self, id: &str) -> Result<(), String> {
        let mut guard = self.inner.lock().await;
        if let Some(flag) = guard.cancel_flags.get(id) {
            flag.store(true, Ordering::SeqCst);
        }
        guard.tasks.retain(|t| t.id != id);
        persist(&guard.tasks)?;
        Ok(())
    }

    pub async fn clear_finished(&self) -> Result<(), String> {
        let mut guard = self.inner.lock().await;
        guard.tasks.retain(|t| {
            !matches!(
                t.status,
                TaskStatus::Completed
                    | TaskStatus::Skipped
                    | TaskStatus::Cancelled
                    | TaskStatus::Failed
            )
        });
        persist(&guard.tasks)?;
        Ok(())
    }

    pub async fn cancel(&self, id: &str) -> Result<(), String> {
        let mut guard = self.inner.lock().await;
        if let Some(flag) = guard.cancel_flags.get(id) {
            flag.store(true, Ordering::SeqCst);
        }
        if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == id) {
            if task.status == TaskStatus::Queued {
                task.status = TaskStatus::Cancelled;
                task.message = "cancelled".into();
            }
        }
        persist(&guard.tasks)?;
        Ok(())
    }

    pub fn start_queue(self: &Arc<Self>, app: AppHandle) {
        let mgr = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            {
                let mut guard = mgr.inner.lock().await;
                if guard.runner_started {
                    return;
                }
                guard.runner_started = true;
            }

            loop {
                let next_id = {
                    let guard = mgr.inner.lock().await;
                    guard
                        .tasks
                        .iter()
                        .rev()
                        .find(|t| t.status == TaskStatus::Queued)
                        .map(|t| t.id.clone())
                };
                let Some(id) = next_id else {
                    let mut guard = mgr.inner.lock().await;
                    guard.runner_started = false;
                    break;
                };
                mgr.run_one(&app, &id).await;
            }
        });
    }

    async fn run_one(self: &Arc<Self>, app: &AppHandle, id: &str) {
        let settings = load_settings();
        let cancel = Arc::new(AtomicBool::new(false));
        let (url, sha256) = {
            let mut guard = self.inner.lock().await;
            guard.cancel_flags.insert(id.to_string(), Arc::clone(&cancel));
            let Some(task) = guard.tasks.iter_mut().find(|t| t.id == id) else {
                return;
            };
            task.status = TaskStatus::Running;
            task.error.clear();
            task.message = "starting".into();
            let url = task.url.clone();
            let sha256 = task.sha256.clone();
            let snapshot = guard.tasks.clone();
            let _ = persist(&snapshot);
            let _ = app.emit("download-updated", snapshot);
            (url, sha256)
        };

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ProgressEvent>();
        let mgr = Arc::clone(self);
        let app_progress = app.clone();
        let progress_id = id.to_string();
        let progress_task = tauri::async_runtime::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let mut guard = mgr.inner.lock().await;
                if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == progress_id) {
                    task.downloaded = ev.downloaded;
                    task.total = ev.total;
                    task.speed_bps = ev.speed_bps;
                    task.message = ev.message.clone();
                }
                let snapshot = guard.tasks.clone();
                drop(guard);
                let _ = app_progress.emit("download-progress", &ev);
                let _ = app_progress.emit("download-updated", snapshot);
            }
        });

        let opts = DownloadOptions {
            task_id: id.to_string(),
            url,
            save_dir: settings.resolve_save_dir(),
            preferred_name: None,
            sha256: if sha256.is_empty() {
                None
            } else {
                Some(sha256)
            },
            settings: settings.clone(),
        };

        let result = download_file(
            opts,
            Arc::new(move |ev| {
                let _ = tx.send(ev);
            }),
            cancel,
        )
        .await;

        let _ = progress_task.await;

        let mut guard = self.inner.lock().await;
        guard.cancel_flags.remove(id);
        if let Some(task) = guard.tasks.iter_mut().find(|t| t.id == id) {
            match result {
                Ok(res) => {
                    task.save_path = paths::to_display_path(&res.path);
                    task.status = if res.skipped {
                        TaskStatus::Skipped
                    } else {
                        TaskStatus::Completed
                    };
                    task.message = if res.skipped {
                        "already completed".into()
                    } else if res.resumed {
                        "resumed complete".into()
                    } else {
                        "done".into()
                    };
                    if res.verified {
                        task.message.push_str(" · sha256 ok");
                    }
                    task.speed_bps = 0;
                    task.error.clear();
                }
                Err(e) if e == "cancelled" => {
                    task.status = TaskStatus::Cancelled;
                    task.message = "cancelled".into();
                    task.speed_bps = 0;
                    task.error.clear();
                }
                Err(e) => {
                    task.status = TaskStatus::Failed;
                    task.message = "failed".into();
                    task.speed_bps = 0;
                    task.error = e;
                }
            }
        }
        let snapshot = guard.tasks.clone();
        let _ = persist(&snapshot);
        let _ = app.emit("download-updated", snapshot);
    }
}

fn persist(tasks: &[TaskRecord]) -> Result<(), String> {
    write_json(
        &paths::tasks_path(),
        &TaskStore {
            tasks: tasks.to_vec(),
        },
    )
}
