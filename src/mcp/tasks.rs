//! MCP Task Manager — supports tasks/send, tasks/cancel, and task status notifications.
//!
//! Per the MCP 2025-11-25 spec:
//! - tasks/send: create a task, returns task_id + status
//! - tasks/cancel: cancel a running task by id
//! - notifications/tasks/message: sent when task produces output

use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

/// Task status per MCP spec.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum TaskStatus {
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// A running or completed task.
struct Task {
    id: String,
    status: TaskStatus,
    result: Option<Value>,
    cancel_flag: Arc<AtomicU64>,
    notify: Arc<Notify>,
}

/// Thread-safe task registry.
pub struct TaskManager {
    next_id: AtomicU64,
    tasks: Mutex<HashMap<String, Task>>,
}

impl TaskManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            next_id: AtomicU64::new(1),
            tasks: Mutex::new(HashMap::new()),
        })
    }

    /// Generate a task id like "task_0001"
    fn next_id(&self) -> String {
        let n = self.next_id.fetch_add(1, Ordering::Relaxed);
        format!("task_{:04}", n)
    }

    /// Register a new running task. Returns (id, cancel_flag, notify).
    pub async fn register(&self) -> (String, Arc<AtomicU64>, Arc<Notify>) {
        let id = self.next_id();
        let cancel_flag = Arc::new(AtomicU64::new(0));
        let notify = Arc::new(Notify::new());
        let task = Task {
            id: id.clone(),
            status: TaskStatus::Running,
            result: None,
            cancel_flag: cancel_flag.clone(),
            notify: notify.clone(),
        };
        self.tasks.lock().await.insert(id.clone(), task);
        (id, cancel_flag, notify)
    }

    /// Mark a task as completed with output.
    pub async fn complete(&self, id: &str, result: Value) {
        if let Some(task) = self.tasks.lock().await.get_mut(id) {
            task.status = TaskStatus::Completed;
            task.result = Some(result);
            task.notify.notify_waiters();
        }
    }

    /// Mark a task as failed.
    #[allow(dead_code)]
    pub async fn fail(&self, id: &str, error: String) {
        if let Some(task) = self.tasks.lock().await.get_mut(id) {
            task.status = TaskStatus::Failed(error);
            task.notify.notify_waiters();
        }
    }

    /// Request cancellation of a task.
    pub async fn cancel(&self, id: &str) -> bool {
        if let Some(task) = self.tasks.lock().await.get_mut(id) {
            if task.status == TaskStatus::Running {
                task.cancel_flag.fetch_add(1, Ordering::Relaxed);
                task.status = TaskStatus::Cancelled;
                task.notify.notify_waiters();
                return true;
            }
        }
        false
    }

    /// Get the current status of a task.
    pub async fn status(&self, id: &str) -> Option<Value> {
        let tasks = self.tasks.lock().await;
        let task = tasks.get(id)?;
        let (status_str, maybe_result) = match &task.status {
            TaskStatus::Running => ("running", None),
            TaskStatus::Completed => ("completed", task.result.clone()),
            TaskStatus::Failed(e) => ("failed", Some(Value::String(e.clone()))),
            TaskStatus::Cancelled => ("cancelled", None),
        };
        let mut v = serde_json::json!({
            "id": task.id,
            "status": status_str,
        });
        if let Some(r) = maybe_result {
            v["result"] = r;
        }
        Some(v)
    }

    /// Get status for all tasks (for list).
    pub async fn all_statuses(&self) -> Vec<Value> {
        let tasks = self.tasks.lock().await;
        let mut list = Vec::new();
        for task in tasks.values() {
            let status_str = match &task.status {
                TaskStatus::Running => "running",
                TaskStatus::Completed => "completed",
                TaskStatus::Failed(_) => "failed",
                TaskStatus::Cancelled => "cancelled",
            };
            list.push(serde_json::json!({
                "id": task.id,
                "status": status_str,
            }));
        }
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_complete() {
        let mgr = TaskManager::new();
        let (id, _cancel, _notify) = mgr.register().await;
        assert!(id.starts_with("task_"));
        let st = mgr.status(&id).await.unwrap();
        assert_eq!(st["status"], "running");

        mgr.complete(&id, serde_json::json!({"done": true})).await;
        let st = mgr.status(&id).await.unwrap();
        assert_eq!(st["status"], "completed");
        assert_eq!(st["result"]["done"], true);
    }

    #[tokio::test]
    async fn test_cancel_task() {
        let mgr = TaskManager::new();
        let (id, cancel_flag, _notify) = mgr.register().await;
        assert_eq!(cancel_flag.load(Ordering::Relaxed), 0);

        let cancelled = mgr.cancel(&id).await;
        assert!(cancelled);
        assert_eq!(cancel_flag.load(Ordering::Relaxed), 1);

        let st = mgr.status(&id).await.unwrap();
        assert_eq!(st["status"], "cancelled");
    }

    #[tokio::test]
    async fn test_fail_task() {
        let mgr = TaskManager::new();
        let (id, _cancel, _notify) = mgr.register().await;
        mgr.fail(&id, "something went wrong".to_string()).await;
        let st = mgr.status(&id).await.unwrap();
        assert_eq!(st["status"], "failed");
        assert_eq!(st["result"], "something went wrong");
    }

    #[tokio::test]
    async fn test_cancel_nonexistent() {
        let mgr = TaskManager::new();
        assert!(!mgr.cancel("task_nonexistent").await);
    }

    #[tokio::test]
    async fn test_all_statuses() {
        let mgr = TaskManager::new();
        let (id1, _, _) = mgr.register().await;
        let (_id2, _, _) = mgr.register().await;
        mgr.complete(&id1, serde_json::json!({"ok": 1})).await;
        let all = mgr.all_statuses().await;
        assert_eq!(all.len(), 2);
    }
}
