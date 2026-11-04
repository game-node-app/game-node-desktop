//! Application composition: runs `core`'s effects against the platform and API adapters.
//!
//! The UI thread only calls [`Engine::dispatch`] (cheap, never blocks on I/O) and renders the
//! [`Snapshot`]s passed to the publish callback. HTTP and browser work runs on Tokio; keyring,
//! file and database work runs on one FIFO storage thread, so a credential write can never be
//! reordered after the sign-out that follows it.

use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use gamenode_core::auth::{NonceSource, RemoteSignOut};
use gamenode_core::ports::{AuthApi, BrowserOpener, CredentialStore, SettingsStore};
use gamenode_core::{
    AuthMachine, Effect, Input, SessionCredential, Settings, SignInFailure, Snapshot,
};
use tokio::runtime::Handle;

/// Longest the app waits for the server to confirm a sign-out.
const REMOTE_SIGN_OUT_TIMEOUT: Duration = Duration::from_secs(5);

/// The adapters the engine drives.
#[derive(Clone)]
pub struct Ports {
    pub settings: Arc<dyn SettingsStore>,
    pub credentials: Arc<dyn CredentialStore>,
    pub api: Arc<dyn AuthApi>,
    pub browser: Arc<dyn BrowserOpener>,
}

type Job = Box<dyn FnOnce() + Send>;

/// Runs blocking storage jobs one at a time, in submission order.
struct StorageWorker {
    jobs: mpsc::Sender<Job>,
}

impl StorageWorker {
    fn new() -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("gamenode-storage".into())
            .spawn(move || {
                while let Ok(job) = queue.recv() {
                    job();
                }
            })
            .expect("storage thread");
        Self { jobs }
    }

    fn submit(&self, job: impl FnOnce() + Send + 'static) {
        // The worker only stops when the engine is dropped.
        let _ = self.jobs.send(Box::new(job));
    }
}

struct State {
    machine: AuthMachine,
    settings: Settings,
}

struct Shared {
    state: Mutex<State>,
    ports: Ports,
    runtime: Handle,
    storage: StorageWorker,
    publish: Box<dyn Fn(Snapshot) + Send + Sync>,
}

/// Cheap to clone; all clones share one state.
#[derive(Clone)]
pub struct Engine {
    shared: Arc<Shared>,
}

impl Engine {
    /// `publish` is called with every new snapshot, from arbitrary threads, while the engine's
    /// lock is held: it must hand the snapshot off (e.g. `slint::invoke_from_event_loop`) and
    /// must not call back into the engine.
    pub fn new(
        ports: Ports,
        runtime: Handle,
        settings: Settings,
        nonce: NonceSource,
        publish: impl Fn(Snapshot) + Send + Sync + 'static,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    machine: AuthMachine::new(nonce),
                    settings,
                }),
                ports,
                runtime,
                storage: StorageWorker::new(),
                publish: Box::new(publish),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn snapshot(&self) -> Snapshot {
        let state = self.lock();
        self.snapshot_of(&state)
    }

    fn snapshot_of(&self, state: &State) -> Snapshot {
        Snapshot::new(
            &state.machine,
            self.shared.ports.credentials.method(),
            state.settings,
        )
    }

    /// Reads the stored session and, if there is one, validates it.
    pub fn start(&self) {
        self.dispatch(Input::Start);
    }

    /// Feeds an input to the state machine and launches its effects. Never blocks on I/O.
    pub fn dispatch(&self, input: Input) {
        let mut state = self.lock();
        let effects = state.machine.handle(input, Instant::now());
        // Effects are launched under the lock so storage jobs keep their dispatch order.
        for effect in effects {
            self.execute(effect);
        }
        (self.shared.publish)(self.snapshot_of(&state));
    }

    /// Applies a settings change, persists it, and publishes the result.
    pub fn update_settings(&self, change: impl FnOnce(&mut Settings)) {
        let mut state = self.lock();
        change(&mut state.settings);
        let settings = state.settings;
        let store = self.shared.ports.settings.clone();
        self.shared.storage.submit(move || {
            if let Err(error) = store.save(&settings) {
                tracing::warn!(%error, "could not save settings");
            }
        });
        (self.shared.publish)(self.snapshot_of(&state));
    }

    fn execute(&self, effect: Effect) {
        let ports = &self.shared.ports;
        match effect {
            Effect::LoadCredential { op } => {
                let (engine, store) = (self.clone(), ports.credentials.clone());
                self.shared.storage.submit(move || {
                    let result = store.load().map_err(|_| ());
                    engine.dispatch(Input::CredentialLoaded { op, result });
                });
            }
            Effect::StoreCredential { op, credential } => {
                let (engine, store) = (self.clone(), ports.credentials.clone());
                self.shared.storage.submit(move || {
                    let ok = store.save(&credential).is_ok();
                    engine.dispatch(Input::CredentialStored { op, ok });
                });
            }
            Effect::DeleteCredential => {
                let (engine, store) = (self.clone(), ports.credentials.clone());
                self.shared.storage.submit(move || {
                    let ok = store.delete().is_ok();
                    engine.dispatch(Input::CredentialDeleted { ok });
                });
            }
            Effect::BeginSignIn {
                op,
                provider,
                success_url,
                failure_url,
            } => {
                let (engine, api, browser) =
                    (self.clone(), ports.api.clone(), ports.browser.clone());
                self.shared.runtime.spawn(async move {
                    let url = match api
                        .begin_sign_in(provider, &success_url, &failure_url)
                        .await
                    {
                        Ok(url) => url,
                        Err(reason) => {
                            engine.dispatch(Input::SignInFailed { op, reason });
                            return;
                        }
                    };
                    // Mark the browser as opened first so a prompt callback is never rejected
                    // for arriving "too early"; a launch failure then cancels the attempt.
                    engine.dispatch(Input::BrowserOpened { op });
                    let opened = tokio::task::spawn_blocking(move || browser.open(&url)).await;
                    if !matches!(opened, Ok(Ok(()))) {
                        engine.dispatch(Input::SignInFailed {
                            op,
                            reason: SignInFailure::BrowserLaunch,
                        });
                    }
                });
            }
            Effect::VerifySession { op, credential } => {
                let (engine, api) = (self.clone(), ports.api.clone());
                self.shared.runtime.spawn(async move {
                    let result = api.get_session(&credential).await;
                    engine.dispatch(Input::SessionChecked { op, result });
                });
            }
            Effect::SignOut { op, credential } => self.sign_out(op, credential),
        }
    }

    /// Local deletion is queued immediately (in order after any pending credential write); the
    /// server invalidation runs concurrently and may fail without blocking local sign-out.
    fn sign_out(&self, op: u64, credential: Option<SessionCredential>) {
        let ports = &self.shared.ports;
        let (local_tx, local_rx) = tokio::sync::oneshot::channel();
        let store = ports.credentials.clone();
        self.shared.storage.submit(move || {
            let _ = local_tx.send(store.delete().is_ok());
        });
        let (engine, api) = (self.clone(), ports.api.clone());
        self.shared.runtime.spawn(async move {
            let remote = match &credential {
                None => RemoteSignOut::Skipped,
                Some(credential) => {
                    match tokio::time::timeout(REMOTE_SIGN_OUT_TIMEOUT, api.sign_out(credential))
                        .await
                    {
                        Ok(Ok(())) => RemoteSignOut::Confirmed,
                        _ => RemoteSignOut::NotConfirmed,
                    }
                }
            };
            drop(credential); // held only long enough to attempt invalidation
            let local_deleted = local_rx.await.unwrap_or(false);
            engine.dispatch(Input::SignOutFinished {
                op,
                remote,
                local_deleted,
            });
        });
    }
}

#[cfg(test)]
mod tests;
