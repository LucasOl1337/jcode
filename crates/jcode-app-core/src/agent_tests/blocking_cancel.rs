use super::*;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::{Mutex, Notify};

#[derive(Clone, Copy)]
enum WaitStage {
    Opening,
    Streaming,
    Tool,
}

#[derive(Clone)]
struct WaitingProvider {
    stage: WaitStage,
    calls: Arc<AtomicUsize>,
    entered: Arc<Notify>,
}

#[async_trait]
impl Provider for WaitingProvider {
    async fn complete(
        &self,
        _: &[Message],
        _: &[ToolDefinition],
        _: &str,
        _: Option<&str>,
    ) -> Result<EventStream> {
        let events = if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            match self.stage {
                WaitStage::Opening => {
                    self.entered.notify_one();
                    return std::future::pending().await;
                }
                WaitStage::Streaming => {
                    self.entered.notify_one();
                    return Ok(Box::pin(futures::stream::pending()));
                }
                WaitStage::Tool => vec![
                    StreamEvent::ToolUseStart {
                        id: "waiting".into(),
                        name: "wait_fixture".into(),
                    },
                    StreamEvent::ToolInputDelta("{}".into()),
                    StreamEvent::ToolUseEnd,
                    StreamEvent::ToolUseStart {
                        id: "remaining".into(),
                        name: "after_fixture".into(),
                    },
                    StreamEvent::ToolInputDelta("{}".into()),
                    StreamEvent::ToolUseEnd,
                    StreamEvent::MessageEnd {
                        stop_reason: Some("tool_use".into()),
                    },
                ],
            }
        } else {
            vec![
                StreamEvent::TextDelta("done".into()),
                StreamEvent::MessageEnd {
                    stop_reason: Some("end_turn".into()),
                },
            ]
        };
        Ok(Box::pin(futures::stream::iter(events.into_iter().map(Ok))))
    }
    fn name(&self) -> &str {
        "blocking-cancel-fixture"
    }
    fn supports_compaction(&self) -> bool {
        false
    }
    fn fork(&self) -> Arc<dyn Provider> {
        Arc::new(self.clone())
    }
}

struct WaitingTool(Arc<Notify>);
struct AfterTool(Arc<AtomicBool>);

#[async_trait]
impl crate::tool::Tool for WaitingTool {
    fn name(&self) -> &str {
        "wait_fixture"
    }
    fn description(&self) -> &str {
        "Wait indefinitely without side effects"
    }
    fn parameters_schema(&self) -> Value {
        json!({"type":"object"})
    }
    async fn execute(&self, _: Value, _: crate::tool::ToolContext) -> Result<ToolOutput> {
        self.0.notify_one();
        std::future::pending().await
    }
}

#[async_trait]
impl crate::tool::Tool for AfterTool {
    fn name(&self) -> &str {
        "after_fixture"
    }
    fn description(&self) -> &str {
        "Record unexpected execution after cancel"
    }
    fn parameters_schema(&self) -> Value {
        json!({"type":"object"})
    }
    async fn execute(&self, _: Value, _: crate::tool::ToolContext) -> Result<ToolOutput> {
        self.0.store(true, Ordering::SeqCst);
        Ok(ToolOutput::new("unexpected"))
    }
}

async fn assert_blocking_cancel(stage: WaitStage) {
    let _env_lock = crate::storage::lock_test_env();
    struct RestoreHome(Option<std::ffi::OsString>);
    impl Drop for RestoreHome {
        fn drop(&mut self) {
            match &self.0 {
                Some(home) => crate::env::set_var("JCODE_HOME", home),
                None => crate::env::remove_var("JCODE_HOME"),
            }
            crate::config::Config::invalidate_cache();
        }
    }
    let home = tempfile::tempdir().unwrap();
    let _restore = RestoreHome(std::env::var_os("JCODE_HOME"));
    crate::env::set_var("JCODE_HOME", home.path());
    crate::config::Config::invalidate_cache();
    let entered = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let after_ran = Arc::new(AtomicBool::new(false));
    let provider = Arc::new(WaitingProvider {
        stage,
        calls: calls.clone(),
        entered: entered.clone(),
    });
    let registry = Registry::empty();
    registry
        .register(
            "wait_fixture".into(),
            Arc::new(WaitingTool(entered.clone())),
        )
        .await;
    registry
        .register(
            "after_fixture".into(),
            Arc::new(AfterTool(after_ran.clone())),
        )
        .await;
    let agent = Arc::new(Mutex::new(Agent::new(provider, registry)));
    let signal = agent.lock().await.graceful_shutdown_signal();
    let running_agent = agent.clone();
    let mut task = tokio::spawn(async move {
        running_agent
            .lock()
            .await
            .run_once_capture("start fixture")
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    signal.fire();
    let stopped = tokio::time::timeout(Duration::from_millis(500), &mut task).await;
    if stopped.is_err() {
        task.abort();
        let _ = task.await;
        panic!("blocking turn ignored cancellation while holding the agent mutex");
    }
    stopped.unwrap().unwrap().unwrap();
    assert!(!signal.is_set(), "cancel must not poison the next turn");
    assert!(
        !after_ran.load(Ordering::SeqCst),
        "remaining tools ran after cancel"
    );
    let mut agent = agent.lock().await;
    let next = tokio::time::timeout(
        Duration::from_secs(5),
        agent.run_once_capture("next prompt"),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(next.contains("done"));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "cancel must not start another provider request"
    );
}

#[tokio::test]
async fn contabo_blocking_cancel_during_provider_open_releases_agent() {
    assert_blocking_cancel(WaitStage::Opening).await;
}

#[tokio::test]
async fn contabo_blocking_cancel_during_silent_stream_releases_agent() {
    assert_blocking_cancel(WaitStage::Streaming).await;
}

#[tokio::test]
async fn contabo_blocking_cancel_during_tool_releases_agent_and_accepts_next_prompt() {
    assert_blocking_cancel(WaitStage::Tool).await;
}
