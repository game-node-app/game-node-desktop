//! Volume observations are read-only worker operations. Watches poll with bounded backpressure.
use super::*;
use gamenode_platform::plugin_storage::FileApproval;
use gamenode_platform::volume;
use std::path::Path;
use tokio_stream::wrappers::ReceiverStream;

static WATCH_SLOTS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(64)));

fn describe(approval: &FileApproval, id: String) -> LocationDescription {
    let path = Path::new(&approval.root);
    let mut ancestor = Some(path);
    let info = loop {
        let Some(candidate) = ancestor else {
            break None;
        };
        if let Some(info) = volume::volume_info(candidate) {
            break Some(info);
        }
        ancestor = candidate.parent();
    };
    let mut result = LocationDescription {
        resource_id: id,
        availability: InspectionAvailability::Unknown as i32,
        network_state: "unknown".into(),
        media_type: "unknown".into(),
        attachment: "unknown".into(),
        interface_type: "unknown".into(),
        evidence: if cfg!(target_os = "linux") {
            "mountinfo/statvfs; sysfs connection and rotational hints; unknown is not evidence of internal storage or SSD"
        } else if cfg!(windows) {
            "Win32 volume GUID, drive type and capacity; physical interface and medium unknown"
        } else { "platform metadata unavailable" }.into(),
        observed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64),
        ..Default::default()
    };
    match info {
        None => {
            result.availability = match std::fs::metadata(&approval.root) {
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    InspectionAvailability::Inaccessible
                }
                _ => InspectionAvailability::Disconnected,
            } as i32;
        }
        Some(info) => {
            if approval.volume_token.is_empty()
                || info.volume_identity_token != approval.volume_token
            {
                result.availability = InspectionAvailability::Disconnected as i32;
                // Do not disclose metadata for the replacement volume.
                return result;
            }
            result.availability = if std::fs::metadata(path).is_ok() {
                InspectionAvailability::Available
            } else {
                InspectionAvailability::Missing
            } as i32;
            result.volume_identity_token = info.volume_identity_token;
            result.mount_point = info.mount_point;
            result.network_state = info.locality.as_str().into();
            result.media_type = info.media_type.as_str().into();
            result.attachment = info.removability.as_str().into();
            result.interface_type = info.bus_type.as_str().into();
            result.capacity_bytes = info.capacity_bytes;
            result.free_bytes = info.free_bytes;
            result.observed_at = info.observed_at;
        }
    }
    result
}

impl ResourceApi {
    pub(super) async fn inspect_location(
        &self,
        request: Request<LocationInspectRequest>,
    ) -> Result<Response<LocationInspectReply>, Status> {
        let request = request.into_inner();
        for dimension in &request.requested_dimensions {
            if !matches!(
                dimension.as_str(),
                "availability"
                    | "volume"
                    | "locality"
                    | "attachment"
                    | "interface"
                    | "media"
                    | "capacity"
            ) {
                return Err(Status::invalid_argument("unsupported inspection dimension"));
            }
        }
        let context = request.context;
        let handle = request.approved_resource_handle;
        let (caller, place, permission) = self.inspect_place(context.as_ref(), &handle).await?;
        let (files, feed, session) = (
            self.files.clone(),
            self.library.clone(),
            caller.session_id.clone(),
        );
        let reply = self
            .in_namespace(&caller, move |namespace| {
                let mut locations = Vec::new();
                match place {
                    Place::Handle { .. } => {
                        let approval =
                            approval_of_handle(&files, &feed, namespace, &session, &handle)?;
                        let mut description = describe(&approval, handle.clone());
                        if description.availability == InspectionAvailability::Available as i32 {
                            // Revalidate both the folder identity and the relative entry; the latter
                            // can be missing even when its approved volume is still mounted.
                            match open_handle_inspect(&files, &feed, namespace, &session, &handle)?
                            {
                                Ok(opened) => {
                                    if let Err(error) = opened.confined.stat(&opened.path) {
                                        description.availability = availability_of(error)
                                            .ok_or_else(|| file_status(error))?
                                            as i32;
                                    }
                                }
                                Err(error) => {
                                    description.availability = availability_of(error)
                                        .ok_or_else(|| file_status(error))?
                                        as i32
                                }
                            }
                        }
                        locations.push(description);
                    }
                    Place::Profile { row } => {
                        for (index, directory) in saves::directories_from_json(&row.directories)
                            .into_iter()
                            .enumerate()
                        {
                            let approval = namespace
                                .approval(&directory.approval_id)?
                                .filter(|a| a.revoked_at.is_none())
                                .ok_or_else(|| Status::not_found("folder approval not found"))?;
                            // Validate scope and affected games just as a regular handle does.
                            let root_handle =
                                files.seal_handle(&session, &approval.id, &directory.path);
                            let approval = approval_of_handle(
                                &files,
                                &feed,
                                namespace,
                                &session,
                                &root_handle,
                            )?;
                            if approval.installation_id != row.installation_id {
                                return Err(Status::not_found("folder approval not found"));
                            }
                            let mut description = describe(&approval, format!("{handle}/d{index}"));
                            if description.availability == InspectionAvailability::Available as i32
                            {
                                match open_approval_inspect(
                                    &files,
                                    &feed,
                                    namespace,
                                    &approval.id,
                                    directory.path,
                                )? {
                                    Ok(opened) => {
                                        if let Err(error) = opened.confined.stat(&opened.path) {
                                            description.availability = availability_of(error)
                                                .ok_or_else(|| file_status(error))?
                                                as i32;
                                        }
                                    }
                                    Err(error) => {
                                        description.availability = availability_of(error)
                                            .ok_or_else(|| file_status(error))?
                                            as i32
                                    }
                                }
                            }
                            locations.push(description);
                        }
                    }
                }
                let location = if locations.len() == 1 {
                    locations.first().cloned()
                } else {
                    Some(LocationDescription {
                        resource_id: handle,
                        availability: if locations
                            .iter()
                            .all(|l| l.availability == InspectionAvailability::Available as i32)
                        {
                            InspectionAvailability::Available
                        } else {
                            InspectionAvailability::Unknown
                        } as i32,
                        network_state: "unknown".into(),
                        media_type: "unknown".into(),
                        attachment: "unknown".into(),
                        interface_type: "unknown".into(),
                        ..Default::default()
                    })
                };
                Ok(LocationInspectReply {
                    location,
                    locations,
                    observations: vec![],
                })
            })
            .await?;
        self.providers
            .authorize_resource(context.as_ref(), permission)?;
        Ok(Response::new(reply))
    }

    pub(super) async fn watch_location_availability(
        &self,
        request: Request<LocationAvailabilityWatchRequest>,
    ) -> Result<Response<BoxStream<LocationAvailabilityEvent>>, Status> {
        let permit = WATCH_SLOTS
            .clone()
            .try_acquire_owned()
            .map_err(|_| Status::resource_exhausted("availability watch limit reached"))?;
        let request = request.into_inner();
        if request.after_sequence != 0 {
            return Err(Status::failed_precondition(
                "availability streams have no replay; resubscribe with after_sequence=0 for a fresh observation",
            ));
        }
        let inspect = LocationInspectRequest {
            context: request.context,
            approved_resource_handle: request.approved_resource_handle,
            ..Default::default()
        };
        let initial = self
            .inspect_location(Request::new(inspect.clone()))
            .await?
            .into_inner();
        let api = self.clone();
        // A bounded queue stops slow consumers. Dropping the receiver stops polling.
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        tokio::spawn(async move {
            let _permit = permit;
            let mut sequence = 0u64;
            let mut previous = Vec::new();
            let mut reply = initial;
            loop {
                for location in &reply.locations {
                    let state = (
                        location.resource_id.clone(),
                        location.availability,
                        location.volume_identity_token.clone(),
                    );
                    if !previous.contains(&state) {
                        sequence += 1;
                        let event = LocationAvailabilityEvent {
                            sequence,
                            resource_id: state.0,
                            availability: state.1,
                            volume_identity_token: state.2,
                            observed_at: location.observed_at,
                        };
                        if tx.try_send(Ok(event)).is_err() {
                            return;
                        }
                    }
                }
                previous = reply
                    .locations
                    .iter()
                    .map(|l| {
                        (
                            l.resource_id.clone(),
                            l.availability,
                            l.volume_identity_token.clone(),
                        )
                    })
                    .collect();
                tokio::select! { _ = tx.closed() => return, _ = tokio::time::sleep(Duration::from_secs(1)) => {} }
                match api.inspect_location(Request::new(inspect.clone())).await {
                    Ok(next) => reply = next.into_inner(),
                    Err(error) => {
                        let _ = tx.try_send(Err(error));
                        return;
                    }
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}
