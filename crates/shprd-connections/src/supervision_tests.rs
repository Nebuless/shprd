use crate::{
    ConnectionId, Error, Manager, Profile, ProfileService, Registry, Runtime, RuntimeContext,
    RuntimeFactory, RuntimeFuture, SocketPaths, State, Store,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{Mutex, Notify, mpsc};

struct BlockingRuntime {
    started: Arc<Notify>,
    stops: Arc<AtomicUsize>,
}
impl Runtime for BlockingRuntime {
    fn start<'a>(&'a self, context: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async move {
            self.started.notify_one();
            context.cancelled().await;
            Err(Error::Stale)
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async move {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

struct StopFails {
    stops: Arc<AtomicUsize>,
    failures: usize,
}
impl Runtime for StopFails {
    fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async {
            Ok(SocketPaths {
                control: "/tmp/control.sock".into(),
                render: "/tmp/render.sock".into(),
            })
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async move {
            if self.stops.fetch_add(1, Ordering::SeqCst) < self.failures {
                return Err(Error::Runtime {
                    message: "fixture stop failed".into(),
                    retryable: true,
                });
            }
            Ok(())
        })
    }
}

struct Fixture {
    contexts: Arc<Mutex<Vec<RuntimeContext>>>,
    started: Arc<Notify>,
    starts: Arc<AtomicUsize>,
}
struct RetryRuntime {
    contexts: Arc<Mutex<Vec<RuntimeContext>>>,
    attempts: mpsc::UnboundedSender<usize>,
    starts: Arc<AtomicUsize>,
    failures_before_success: usize,
    reconnect_retryable: bool,
}
impl Runtime for RetryRuntime {
    fn start<'a>(&'a self, context: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async move {
            self.contexts.lock().await.push(context.clone());
            let attempt = self.starts.fetch_add(1, Ordering::SeqCst) + 1;
            self.attempts
                .send(attempt)
                .expect("retry attempt receiver remains active");
            if attempt > 1 && attempt <= self.failures_before_success + 1 {
                return Err(Error::Runtime {
                    message: "fixture reconnect failed".into(),
                    retryable: self.reconnect_retryable,
                });
            }
            Ok(SocketPaths {
                control: "/tmp/control.sock".into(),
                render: "/tmp/render.sock".into(),
            })
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}
impl Runtime for Fixture {
    fn start<'a>(&'a self, context: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async move {
            self.contexts.lock().await.push(context.clone());
            self.starts.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            Ok(SocketPaths {
                control: "/tmp/control.sock".into(),
                render: "/tmp/render.sock".into(),
            })
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}
fn profile() -> Profile {
    Profile::parse(
        r#"{"id":"local","label":"Local","type":"local","control_socket_path":"/tmp/control.sock","client_socket_path":"/tmp/render.sock","auto_connect":false}"#,
    )
    .unwrap()
}
fn factory(
    contexts: Arc<Mutex<Vec<RuntimeContext>>>,
    started: Arc<Notify>,
    starts: Arc<AtomicUsize>,
) -> RuntimeFactory {
    Arc::new(move |_| {
        Ok(Arc::new(Fixture {
            contexts: contexts.clone(),
            started: started.clone(),
            starts: starts.clone(),
        }))
    })
}
fn retry_factory(
    contexts: Arc<Mutex<Vec<RuntimeContext>>>,
    attempts: mpsc::UnboundedSender<usize>,
    starts: Arc<AtomicUsize>,
    failures_before_success: usize,
) -> RuntimeFactory {
    retry_factory_with_retryability(contexts, attempts, starts, failures_before_success, true)
}
fn retry_factory_with_retryability(
    contexts: Arc<Mutex<Vec<RuntimeContext>>>,
    attempts: mpsc::UnboundedSender<usize>,
    starts: Arc<AtomicUsize>,
    failures_before_success: usize,
    reconnect_retryable: bool,
) -> RuntimeFactory {
    Arc::new(move |_| {
        Ok(Arc::new(RetryRuntime {
            contexts: contexts.clone(),
            attempts: attempts.clone(),
            starts: starts.clone(),
            failures_before_success,
            reconnect_retryable,
        }))
    })
}

async fn initial_attempt(attempts: &mut mpsc::UnboundedReceiver<usize>) {
    let actual = tokio::time::timeout(std::time::Duration::from_secs(2), attempts.recv())
        .await
        .expect("initial attempt event timed out")
        .expect("retry attempt channel closed");
    assert_eq!(actual, 1);
}

async fn await_ready(observation: &Arc<crate::runtime::RetryTaskObservation>) {
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        observation.ready.notified(),
    )
    .await
    .expect("runtime did not become ready");
}

async fn next_retry_attempt(attempts: &mut mpsc::UnboundedReceiver<usize>, expected: usize) {
    next_retry_attempt_after(attempts, expected, std::time::Duration::from_secs(31)).await;
}

async fn next_retry_attempt_after(
    attempts: &mut mpsc::UnboundedReceiver<usize>,
    expected: usize,
    elapsed: std::time::Duration,
) {
    tokio::time::advance(elapsed).await;
    tokio::task::yield_now().await;
    let actual = tokio::time::timeout(std::time::Duration::from_secs(2), attempts.recv())
        .await
        .unwrap_or_else(|_| panic!("retry attempt event timed out expected={expected}"))
        .expect("retry attempt channel closed");
    assert_eq!(actual, expected);
}

async fn await_retry_completion(
    observation: &Arc<crate::runtime::RetryTaskObservation>,
    previous: usize,
) {
    let completed = async {
        loop {
            if observation.completion_count.load(Ordering::SeqCst) > previous {
                return;
            }
            observation.completed.notified().await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), completed)
        .await
        .expect("retry task did not complete");
}

async fn latest_context(contexts: &Arc<Mutex<Vec<RuntimeContext>>>) -> RuntimeContext {
    contexts
        .lock()
        .await
        .last()
        .cloned()
        .expect("runtime context missing")
}

fn assert_no_queued_retry_attempt(
    attempts: &mut mpsc::UnboundedReceiver<usize>,
    starts: &Arc<AtomicUsize>,
    expected_starts: usize,
) {
    assert!(
        attempts.try_recv().is_err(),
        "retry produced unexpected attempt event"
    );
    assert_eq!(starts.load(Ordering::SeqCst), expected_starts);
}

#[tokio::test(start_paused = true)]
async fn retry_recovers_after_first_reconnect_failure() {
    let id = ConnectionId::parse("local").unwrap();
    let manager = Arc::new(Manager::new(id.clone()));
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let (attempt_sender, mut attempts) = mpsc::unbounded_channel();
    let starts = Arc::new(AtomicUsize::new(0));
    manager
        .register(
            profile(),
            retry_factory(contexts.clone(), attempt_sender, starts.clone(), 1),
        )
        .unwrap();
    manager.connect(&id).await.unwrap();
    initial_attempt(&mut attempts).await;
    let initial_generation = manager.status(&id).unwrap().generation;
    let context = contexts.lock().await[0].clone();
    context.report_error("control EOF", true).unwrap();
    next_retry_attempt(&mut attempts, 2).await;
    next_retry_attempt(&mut attempts, 3).await;
    assert_eq!(starts.load(Ordering::SeqCst), 3);
    let status = manager.status(&id).unwrap();
    assert_eq!(status.state, State::Ready);
    assert!(status.error.is_none());
    assert!(status.generation > initial_generation);
}

#[tokio::test(start_paused = true)]
async fn rapid_ready_flaps_preserve_retry_budget_until_stable() {
    let id = ConnectionId::parse("local").unwrap();
    let manager = Arc::new(Manager::new(id.clone()));
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let (attempt_sender, mut attempts) = mpsc::unbounded_channel();
    let starts = Arc::new(AtomicUsize::new(0));
    manager
        .register(
            profile(),
            retry_factory_with_retryability(
                contexts.clone(),
                attempt_sender,
                starts.clone(),
                0,
                true,
            ),
        )
        .unwrap();
    manager.connect(&id).await.unwrap();
    initial_attempt(&mut attempts).await;
    let observation = manager.retry_task_observation(&id).unwrap();
    await_ready(&observation).await;
    for (expected, delay) in [(2, 1), (3, 2), (4, 4), (5, 8), (6, 16), (7, 22)] {
        latest_context(&contexts)
            .await
            .report_error("control EOF", true)
            .unwrap();
        tokio::task::yield_now().await;
        next_retry_attempt_after(
            &mut attempts,
            expected,
            std::time::Duration::from_secs(delay),
        )
        .await;
        await_ready(&observation).await;
    }
    let previous = observation.completion_count.load(Ordering::SeqCst);
    latest_context(&contexts)
        .await
        .report_error("control EOF", true)
        .unwrap();
    tokio::task::yield_now().await;
    await_retry_completion(&observation, previous).await;
    assert_no_queued_retry_attempt(&mut attempts, &starts, 7);
    assert_eq!(manager.status(&id).unwrap().state, State::Reconnecting);
    assert!(manager.status(&id).unwrap().error.is_some());

    manager.connect(&id).await.unwrap();
    await_ready(&observation).await;
    let actual = tokio::time::timeout(std::time::Duration::from_secs(2), attempts.recv())
        .await
        .expect("explicit reconnect start timed out")
        .expect("retry attempt channel closed");
    assert_eq!(actual, 8);
    tokio::time::advance(std::time::Duration::from_secs(30)).await;
    latest_context(&contexts)
        .await
        .report_error("control EOF", true)
        .unwrap();
    tokio::task::yield_now().await;
    next_retry_attempt(&mut attempts, 9).await;
}

#[tokio::test(start_paused = true)]
async fn nonretryable_reconnect_failure_stops_retry_task() {
    let id = ConnectionId::parse("local").unwrap();
    let manager = Arc::new(Manager::new(id.clone()));
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let (attempt_sender, mut attempts) = mpsc::unbounded_channel();
    let starts = Arc::new(AtomicUsize::new(0));
    manager
        .register(
            profile(),
            retry_factory_with_retryability(
                contexts.clone(),
                attempt_sender,
                starts.clone(),
                100,
                false,
            ),
        )
        .unwrap();
    manager.connect(&id).await.unwrap();
    initial_attempt(&mut attempts).await;
    let observation = manager.retry_task_observation(&id).unwrap();
    let previous = observation.completion_count.load(Ordering::SeqCst);
    latest_context(&contexts)
        .await
        .report_error("control EOF", true)
        .unwrap();
    next_retry_attempt(&mut attempts, 2).await;
    tokio::task::yield_now().await;
    await_retry_completion(&observation, previous).await;
    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    assert_no_queued_retry_attempt(&mut attempts, &starts, 2);
    let status = manager.status(&id).unwrap();
    assert_eq!(status.state, State::Error);
    assert!(status.error.is_some());
}

#[tokio::test(start_paused = true)]
async fn retry_stops_at_six_reconnect_attempts() {
    let id = ConnectionId::parse("local").unwrap();
    let manager = Arc::new(Manager::new(id.clone()));
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let (attempt_sender, mut attempts) = mpsc::unbounded_channel();
    let starts = Arc::new(AtomicUsize::new(0));
    manager
        .register(
            profile(),
            retry_factory(contexts.clone(), attempt_sender, starts.clone(), 6),
        )
        .unwrap();
    manager.connect(&id).await.unwrap();
    initial_attempt(&mut attempts).await;
    let context = contexts.lock().await[0].clone();
    let observation = manager.retry_task_observation(&id).unwrap();
    let armed = observation.armed.notified();
    context.report_error("control EOF", true).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), armed)
        .await
        .expect("retry task did not arm");
    for expected in 2..=7 {
        next_retry_attempt(&mut attempts, expected).await;
    }
    let completed = observation.completed.notified();
    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    tokio::time::timeout(std::time::Duration::from_secs(2), completed)
        .await
        .expect("retry task did not complete");
    assert_no_queued_retry_attempt(&mut attempts, &starts, 7);
    let status = manager.status(&id).unwrap();
    assert_eq!(status.state, State::Reconnecting);
    assert!(status.error.is_some());
    manager.disconnect(&id).await.unwrap();
    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    assert_no_queued_retry_attempt(&mut attempts, &starts, 7);
    let disconnected = manager.status(&id).unwrap();
    assert_eq!(disconnected.state, State::Disconnected);
    assert!(disconnected.error.is_none());
}

#[tokio::test(start_paused = true)]
async fn explicit_connect_starts_new_retry_budget_after_exhaustion() {
    let id = ConnectionId::parse("local").unwrap();
    let manager = Arc::new(Manager::new(id.clone()));
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let (attempt_sender, mut attempts) = mpsc::unbounded_channel();
    let starts = Arc::new(AtomicUsize::new(0));
    manager
        .register(
            profile(),
            retry_factory(contexts.clone(), attempt_sender, starts.clone(), 6),
        )
        .unwrap();
    manager.connect(&id).await.unwrap();
    initial_attempt(&mut attempts).await;
    let observation = manager.retry_task_observation(&id).unwrap();
    let armed = observation.armed.notified();
    latest_context(&contexts)
        .await
        .report_error("control EOF", true)
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), armed)
        .await
        .expect("automatic retry task did not arm");
    for expected in 2..=7 {
        next_retry_attempt(&mut attempts, expected).await;
    }
    let first_completed = observation.completed.notified();
    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    tokio::time::timeout(std::time::Duration::from_secs(2), first_completed)
        .await
        .expect("first retry task did not complete");
    assert_eq!(starts.load(Ordering::SeqCst), 7);

    manager.connect(&id).await.unwrap();
    let explicit_attempt = tokio::time::timeout(std::time::Duration::from_secs(2), attempts.recv())
        .await
        .expect("explicit connect attempt timed out")
        .expect("attempt channel closed");
    assert_eq!(explicit_attempt, 8);
    let explicit_context = latest_context(&contexts).await;
    assert_eq!(starts.load(Ordering::SeqCst), 8);
    assert_eq!(manager.status(&id).unwrap().state, State::Ready);

    let second_completed = observation.completed.notified();
    let second_armed = observation.armed.notified();
    explicit_context
        .report_error("early control EOF", true)
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), second_armed)
        .await
        .expect("explicit reconnect did not arm a new retry task");
    next_retry_attempt(&mut attempts, 9).await;
    tokio::time::timeout(std::time::Duration::from_secs(2), second_completed)
        .await
        .expect("new retry task did not complete");
    assert_eq!(manager.status(&id).unwrap().state, State::Ready);
    assert_eq!(starts.load(Ordering::SeqCst), 9);
}

#[tokio::test(start_paused = true)]
async fn retry_task_cancellation_observation_rejects_broken_abort() {
    let id = ConnectionId::parse("local").unwrap();
    let manager = Arc::new(Manager::new(id.clone()));
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let (attempt_sender, mut attempts) = mpsc::unbounded_channel();
    let starts = Arc::new(AtomicUsize::new(0));
    manager
        .register(
            profile(),
            retry_factory(contexts.clone(), attempt_sender, starts.clone(), 6),
        )
        .unwrap();
    manager.connect(&id).await.unwrap();
    initial_attempt(&mut attempts).await;
    let context = contexts.lock().await[0].clone();
    let observation = manager.retry_task_observation(&id).unwrap();
    let armed = observation.armed.notified();
    context.report_error("control EOF", true).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), armed)
        .await
        .expect("retry task did not arm");
    let completed = observation.completed.notified();
    manager.disconnect(&id).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), completed)
        .await
        .expect("retry task did not complete");
    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    assert_no_queued_retry_attempt(&mut attempts, &starts, 1);
    assert_eq!(manager.status(&id).unwrap().state, State::Disconnected);
    println!("CANCELLATION_ASSERTION=PASS manager_retry_task_completed");
}

#[tokio::test]
async fn local_control_disconnect_retries_after_generation_invalidation() {
    let id = ConnectionId::parse("local").unwrap();
    let manager = Arc::new(Manager::new(id.clone()));
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let started = Arc::new(Notify::new());
    let starts = Arc::new(AtomicUsize::new(0));
    manager
        .register(
            profile(),
            factory(contexts.clone(), started.clone(), starts.clone()),
        )
        .unwrap();
    manager.connect(&id).await.unwrap();
    started.notified().await;
    let context = contexts.lock().await[0].clone();
    let generation = context.generation();
    context
        .report_error("control event stream closed", true)
        .unwrap();
    assert_eq!(manager.status(&id).unwrap().state, State::Reconnecting);
    assert!(manager.status(&id).unwrap().generation > generation);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if starts.load(Ordering::SeqCst) >= 2 {
                break;
            }
            started.notified().await;
        }
    })
    .await
    .expect("retry did not start after control disconnect");
}

#[tokio::test]
async fn remove_surfaces_cleanup_failure_and_retry_preserves_profile() {
    let directory = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let path = directory.path().join("connections.json");
    let id = ConnectionId::parse("saved").unwrap();
    let default_id = ConnectionId::parse("default").unwrap();
    let legacy = Profile::legacy("/tmp/legacy-control", "/tmp/legacy-render").unwrap();
    let default_profile = Profile::parse(
        r#"{"id":"default","label":"Default","type":"local","control_socket_path":"/tmp/default-control.sock","client_socket_path":"/tmp/default-render.sock","auto_connect":false}"#,
    )
    .unwrap();
    let profile = Profile::parse(
        r#"{"id":"saved","label":"Saved","type":"local","control_socket_path":"/tmp/control.sock","client_socket_path":"/tmp/render.sock","auto_connect":false}"#,
    )
    .unwrap();
    let manager = Arc::new(Manager::new(default_id.clone()));
    let stops = Arc::new(AtomicUsize::new(0));
    let runtime = Arc::new(StopFails {
        stops: stops.clone(),
        failures: 2,
    });
    let mut store = Store::new(path.clone()).unwrap();
    store
        .save(&Registry {
            version: 2,
            default_connection_id: default_id,
            profiles: vec![default_profile, profile],
        })
        .unwrap();
    let mut service = ProfileService::load(
        Store::new(path.clone()).unwrap(),
        legacy,
        false,
        manager.clone(),
        Arc::new({
            let runtime = runtime.clone();
            move |_| {
                let runtime = runtime.clone();
                Arc::new(move |_| Ok(runtime.clone() as Arc<dyn Runtime>))
            }
        }),
    )
    .unwrap();
    assert!(service.profile(&id).is_ok());
    manager.connect(&id).await.unwrap();
    let lease = manager.lease(&id).unwrap();
    let failure = service.remove(&id).await.unwrap_err();
    assert!(failure.to_string().contains("fixture stop failed"));
    assert!(service.profile(&id).is_ok());
    assert!(
        Store::new(path.clone())
            .unwrap()
            .load()
            .unwrap()
            .unwrap()
            .profiles
            .iter()
            .any(|p| p.id() == &id)
    );
    assert_eq!(manager.status(&id).unwrap().state, State::Error);
    assert!(!lease.is_current());
    assert!(manager.connect(&id).await.is_err());
    assert_eq!(stops.load(Ordering::SeqCst), 2);
    manager.connect(&id).await.unwrap();
    assert_eq!(manager.status(&id).unwrap().state, State::Ready);
    service.remove(&id).await.unwrap();
    assert!(service.profile(&id).is_err());
    assert!(manager.status(&id).is_err());
    assert_eq!(stops.load(Ordering::SeqCst), 4);
}

#[tokio::test(start_paused = true)]
async fn explicit_disconnect_cancels_retry_without_new_start() {
    let id = ConnectionId::parse("local").unwrap();
    let manager = Arc::new(Manager::new(id.clone()));
    let contexts = Arc::new(Mutex::new(Vec::new()));
    let (attempt_sender, mut attempts) = mpsc::unbounded_channel();
    let starts = Arc::new(AtomicUsize::new(0));
    manager
        .register(
            profile(),
            retry_factory(contexts.clone(), attempt_sender, starts.clone(), 6),
        )
        .unwrap();
    manager.connect(&id).await.unwrap();
    initial_attempt(&mut attempts).await;
    let context = contexts.lock().await[0].clone();
    let observation = manager.retry_task_observation(&id).unwrap();
    let armed = observation.armed.notified();
    context.report_error("control EOF", true).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), armed)
        .await
        .expect("retry task did not arm");
    let completed = observation.completed.notified();
    manager.disconnect(&id).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), completed)
        .await
        .expect("retry task did not complete");
    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    assert_no_queued_retry_attempt(&mut attempts, &starts, 1);
    assert_eq!(manager.status(&id).unwrap().state, State::Disconnected);
}

#[tokio::test]
async fn failed_remove_cleanup_retains_runtime_for_second_attempt() {
    let id = ConnectionId::parse("local").unwrap();
    let default = ConnectionId::parse("default").unwrap();
    let manager = Arc::new(Manager::new(default));
    let stops = Arc::new(AtomicUsize::new(0));
    manager
        .register(profile(), failing_stop_factory(stops.clone(), 2))
        .unwrap();
    manager.connect(&id).await.unwrap();
    assert!(manager.unregister(&id).await.is_err());
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert!(manager.unregister(&id).await.is_err());
    assert_eq!(stops.load(Ordering::SeqCst), 2);
    assert!(manager.status(&id).is_ok());
}

#[tokio::test]
async fn shutdown_cancels_pending_start_and_reaps_runtime() {
    let id = ConnectionId::parse("local").unwrap();
    let manager = Arc::new(Manager::new(id.clone()));
    let started = Arc::new(Notify::new());
    let stops = Arc::new(AtomicUsize::new(0));
    let runtime = Arc::new(BlockingRuntime {
        started: started.clone(),
        stops: stops.clone(),
    });
    manager
        .register(profile(), Arc::new(move |_| Ok(runtime.clone())))
        .unwrap();
    let task = {
        let manager = manager.clone();
        let id = id.clone();
        tokio::spawn(async move { manager.connect(&id).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), started.notified())
        .await
        .unwrap();
    manager.stop_all().await.unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert_eq!(manager.status(&id).unwrap().state, State::Disconnected);
}

fn failing_stop_factory(stops: Arc<AtomicUsize>, failures: usize) -> RuntimeFactory {
    Arc::new(move |_| {
        let stops = stops.clone();
        Ok(Arc::new(FailingStopRuntime { stops, failures }))
    })
}
struct FailingStopRuntime {
    stops: Arc<AtomicUsize>,
    failures: usize,
}
impl Runtime for FailingStopRuntime {
    fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async {
            Ok(SocketPaths {
                control: "/tmp/c".into(),
                render: "/tmp/r".into(),
            })
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async move {
            let attempt = self.stops.fetch_add(1, Ordering::SeqCst);
            if attempt < self.failures {
                return Err(Error::Runtime {
                    message: "fixture cleanup failed".into(),
                    retryable: true,
                });
            }
            Ok(())
        })
    }
}
