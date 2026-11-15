//! Plugin requests reuse the trusted guided setup and the existing PC port runner.
use super::*;
use crate::plugin_host::jobs::Fence;
use gamenode_core::ports::view::PortPluginRequest;
use gamenode_plugin_protocol::proto::PortRecord;
use pc_ports::{PortInstallRequest, PortsError};
use tonic::Status;

pub(super) struct PendingPort {
    id: String,
    catalog_id: String,
    check: Fence,
    started: bool,
    native_job: Option<String>,
    done: Option<tokio::sync::oneshot::Sender<Result<String, Status>>>,
}
struct ReviewWait {
    engine: Engine,
    id: String,
}
impl Drop for ReviewWait {
    fn drop(&mut self) {
        self.engine.close_plugin_port(&self.id);
    }
}

impl Engine {
    pub(super) async fn plugin_ports_catalog(
        &self,
        detail: Option<String>,
    ) -> Result<Vec<PortRecord>, Status> {
        let service = self
            .ports_service()
            .ok_or_else(|| Status::unavailable("ports catalog unavailable"))?;
        service.open();
        tokio::task::spawn_blocking(move || {
            let initial = service
                .build_view()
                .map_err(|_| Status::unavailable("ports catalog unavailable"))?;
            if initial.entries.is_empty()
                && initial.catalog.state != gamenode_core::ports::view::PortCatalogState::Ready
            {
                return Err(Status::unavailable(
                    "ports catalog is loading or unavailable; retry",
                ));
            }
            if let Some(id) = &detail
                && service
                    .detail(id)
                    .map_err(|_| Status::unavailable("port details unavailable"))?
                    .is_none()
            {
                return Err(Status::not_found("port not found"));
            }
            let view = service
                .build_view()
                .map_err(|_| Status::unavailable("ports catalog unavailable"))?;
            use gamenode_core::ports::view::PortCatalogState;
            if !matches!(
                view.catalog.state,
                PortCatalogState::Ready | PortCatalogState::Stale | PortCatalogState::Refreshing
            ) {
                return Err(Status::unavailable(
                    "ports catalog is loading or unavailable; retry",
                ));
            }
            if view.catalog.state == PortCatalogState::Refreshing && view.entries.is_empty() {
                return Err(Status::unavailable("ports catalog is loading; retry"));
            }
            let records = view
                .entries
                .into_iter()
                .filter(|e| detail.as_ref().is_none_or(|id| e.catalog_id == *id))
                .map(|entry| PortRecord {
                    port_id: entry.catalog_id,
                    name: entry.name,
                    platform: "windows".into(),
                    availability: if !service.is_supported() {
                        "unavailable"
                    } else if entry.installed_registration_key.is_some() {
                        "installed"
                    } else if entry.installable {
                        "available"
                    } else {
                        "blocked"
                    }
                    .into(),
                })
                .collect::<Vec<_>>();
            if detail.is_some() && records.is_empty() {
                return Err(Status::not_found("port not found"));
            }
            Ok(records)
        })
        .await
        .map_err(|_| Status::unavailable("ports catalog worker unavailable"))?
    }

    pub(super) async fn review_plugin_port(
        &self,
        catalog_id: String,
        mut review: PortPluginRequest,
        check: Fence,
    ) -> Result<(), Status> {
        let service = self
            .ports_service()
            .filter(|s| s.is_supported())
            .ok_or_else(|| Status::unavailable("port installs unavailable on this platform"))?;
        // Resolve the catalog before opening the window; arbitrary URLs never become recipes.
        self.plugin_ports_catalog(Some(catalog_id.clone())).await?;
        let id = gamenode_platform::random::session_id();
        review.id = id.clone();
        review.catalog_id = catalog_id.clone();
        let (done, mut accepted) = tokio::sync::oneshot::channel();
        {
            let mut state = self.lock();
            if state.plugin_port_request.is_some() {
                return Err(Status::resource_exhausted(
                    "a plugin port setup is already open",
                ));
            }
            state.plugin_port_request = Some(PendingPort {
                id: id.clone(),
                catalog_id,
                check: check.clone(),
                started: false,
                native_job: None,
                done: Some(done),
            });
            state.ports_view.plugin_request = Some(Box::new(review));
            self.publish(&mut state);
        }
        let _wait = ReviewWait {
            engine: self.clone(),
            id: id.clone(),
        };
        let deadline = Instant::now() + Duration::from_secs(300);
        let native_job = loop {
            tokio::select! {
                answer = &mut accepted => break answer.map_err(|_| Status::cancelled("port setup closed"))??,
                () = tokio::time::sleep(Duration::from_millis(100)) => {
                    let started = self.lock().plugin_port_request.as_ref().is_some_and(|p| p.id == id && p.started);
                    if !started {
                        let guard = check.clone();
                        tokio::task::spawn_blocking(move || guard()).await.map_err(|_| Status::unavailable("port scope worker unavailable"))??;
                        if Instant::now() >= deadline { return Err(Status::deadline_exceeded("port setup timed out")); }
                    }
                }
            }
        };
        loop {
            let guard = check.clone();
            let authorized = tokio::task::spawn_blocking(move || guard())
                .await
                .map_err(|_| Status::unavailable("port scope worker unavailable"))?;
            if authorized.is_err() {
                service.cancel(&native_job);
            }
            let backend = service.clone();
            let job_id = native_job.clone();
            let job = tokio::task::spawn_blocking(move || backend.plugin_job(&job_id))
                .await
                .map_err(|_| Status::unavailable("port job worker unavailable"))?
                .map_err(|_| Status::unavailable("port job unavailable"))?
                .ok_or_else(|| Status::unavailable("port job unavailable"))?;
            use gamenode_core::ports::PortJobPhase;
            match job.phase {
                PortJobPhase::Completed => return Ok(()),
                PortJobPhase::Cancelled => {
                    return Err(Status::cancelled("port installation cancelled"));
                }
                PortJobPhase::Failed { .. } | PortJobPhase::Interrupted { .. } => {
                    return Err(Status::failed_precondition(
                        "port installation did not complete; inspect Core status",
                    ));
                }
                _ => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
    }

    pub(super) fn close_plugin_port(&self, id: &str) {
        let mut state = self.lock();
        if state
            .plugin_port_request
            .as_ref()
            .is_some_and(|p| p.id == id)
        {
            if let Some(mut pending) = state.plugin_port_request.take() {
                if let Some(job) = &pending.native_job
                    && let Some(service) = self.ports_service()
                {
                    service.cancel(job);
                }
                if let Some(done) = pending.done.take() {
                    let _ = done.send(Err(Status::cancelled("port setup declined")));
                }
            }
            state.ports_view.plugin_request = None;
            self.publish(&mut state);
        }
    }

    pub fn dismiss_plugin_port(&self, id: String) {
        let started = self
            .lock()
            .plugin_port_request
            .as_ref()
            .is_some_and(|p| p.id == id && p.started);
        if started {
            let mut state = self.lock();
            if state
                .ports_view
                .plugin_request
                .as_ref()
                .is_some_and(|p| p.id == id)
            {
                state.ports_view.plugin_request = None;
                self.publish(&mut state);
            }
        } else {
            self.close_plugin_port(&id);
        }
    }

    pub(super) fn plugin_port_install_check(
        &self,
        request: &PortInstallRequest,
    ) -> Result<Option<Fence>, PortsError> {
        let mut state = self.lock();
        let Some(pending) = state.plugin_port_request.as_mut() else {
            return if request.plugin_request_id.is_some() {
                Err(PortsError::Unavailable("port setup expired".into()))
            } else {
                Ok(None)
            };
        };
        // A user's own setup is unrelated to the plugin review and must keep working while a
        // reviewed install waits for the plugin's completion.
        if request.plugin_request_id.is_none() {
            return Ok(None);
        }
        if pending.started
            || request.plugin_request_id.as_deref() != Some(&pending.id)
            || request.catalog_id != pending.catalog_id
            || request.retry_job_id.is_some()
        {
            return Err(PortsError::Unavailable(
                "port setup does not match this request".into(),
            ));
        }
        pending.started = true;
        Ok(Some(pending.check.clone()))
    }

    pub(super) fn plugin_port_enqueued(
        &self,
        id: &str,
        result: &Result<Option<String>, PortsError>,
    ) {
        let mut state = self.lock();
        if let Some(pending) = state.plugin_port_request.as_mut().filter(|p| p.id == id) {
            match result {
                Ok(Some(job_id)) => {
                    pending.native_job = Some(job_id.clone());
                    if let Some(done) = pending.done.take() {
                        let _ = done.send(Ok(job_id.clone()));
                    }
                }
                _ => pending.started = false, // Keep the same review open for a corrected user choice.
            }
        }
    }
}
