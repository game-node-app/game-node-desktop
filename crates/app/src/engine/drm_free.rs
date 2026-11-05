//! Short UI commands; filesystem and persistence work run on blocking workers.
use super::*;
use gamenode_core::drm_free::{LocalError, LocalView, Registration};
impl Engine {
    pub fn refresh_local_games(&self) {
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let view = engine.load_local_view();
            let mut state = engine.lock();
            state.local = view;
            engine.publish(&mut state);
        });
    }
    pub fn add_local_executable(&self, path: Option<String>) {
        let Some(path) = path else {
            return;
        };
        self.local_work(move |engine| {
            let (store, host) = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .zip(engine.shared.ports.installations.as_ref())
                .ok_or(LocalError::Unsupported)?;
            let registration = host.inspect(&path)?;
            let existing = store
                .registrations()
                .map_err(local_storage)?
                .into_iter()
                .find(|r| !r.removed && r.file_identity == registration.file_identity);
            if existing.is_none() {
                store
                    .save_registration(&registration)
                    .map_err(local_storage)?;
            }
            Ok(())
        });
    }
    pub fn remove_local_game(&self, id: String) {
        self.local_work(move |engine| {
            let store = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .ok_or(LocalError::Unsupported)?;
            if let Some(mut registration) = store
                .registrations()
                .map_err(local_storage)?
                .into_iter()
                .find(|r| r.id == id)
            {
                registration.removed = true;
                registration.revision += 1;
                store
                    .save_registration(&registration)
                    .map_err(local_storage)?;
            }
            Ok(())
        });
    }
    pub fn edit_local_game(&self, value: Registration) {
        self.local_work(move |engine| {
            let (store, host) = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .zip(engine.shared.ports.installations.as_ref())
                .ok_or(LocalError::Unsupported)?;
            let mut inspected = host.inspect(&value.executable)?;
            inspected.id = value.id;
            inspected.title = value.title;
            inspected.root = value.root;
            inspected.working_directory = value.working_directory;
            inspected.arguments = value.arguments;
            inspected.gameplay = value.gameplay;
            inspected.launchers = value.launchers;
            inspected.revision = value.revision + 1;
            store.save_registration(&inspected).map_err(local_storage)?;
            Ok(())
        });
    }
    pub fn repair_local_game(&self, id: String, path: Option<String>) {
        let Some(path) = path else {
            return;
        };
        self.local_work(move |engine| {
            let (store, host) = engine
                .shared
                .ports
                .local_games
                .as_ref()
                .zip(engine.shared.ports.installations.as_ref())
                .ok_or(LocalError::Unsupported)?;
            let mut replacement = host.inspect(&path)?;
            let old = store
                .registrations()
                .map_err(local_storage)?
                .into_iter()
                .find(|r| r.id == id)
                .ok_or(LocalError::InvalidTarget)?;
            replacement.id = old.id;
            replacement.title = old.title;
            replacement.revision = old.revision + 1;
            replacement.arguments = old.arguments;
            store
                .save_registration(&replacement)
                .map_err(local_storage)?;
            Ok(())
        });
    }
    fn local_work(&self, work: impl FnOnce(&Engine) -> Result<(), LocalError> + Send + 'static) {
        {
            let mut state = self.lock();
            if state.local.busy {
                return;
            }
            state.local.busy = true;
            state.local.error = None;
            self.publish(&mut state);
        }
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let result = work(&engine);
            let view = engine.load_local_view();
            {
                let mut state = engine.lock();
                state.local = view;
                state.local.error = result.err().map(|e| e.to_string());
                engine.publish(&mut state);
            }
            engine.rescan();
            engine.enrich_local_games();
        });
    }
    fn load_local_view(&self) -> LocalView {
        let Some(store) = &self.shared.ports.local_games else {
            return LocalView::default();
        };
        let result = (|| {
            Ok::<_, StorageError>(LocalView {
                registrations: store.registrations()?,
                candidates: store.candidates()?,
                profiles: store.profiles()?,
                jobs: store.jobs()?,
                settings: { let mut settings=store.backup_settings()?; if settings.destination.is_empty() { settings.destination=self.shared.ports.covers_dir.parent().unwrap_or(std::path::Path::new(".")).join("backups").to_string_lossy().into_owned();store.save_backup_settings(&settings)?; } settings },
                ..LocalView::default()
            })
        })();
        result.unwrap_or_else(|e| LocalView {
            error: Some(e.to_string()),
            ..LocalView::default()
        })
    }
}
fn local_storage(error: StorageError) -> LocalError {
    LocalError::Failed(error.to_string())
}

impl Engine {
    pub fn pick_local_executable(&self, repair: Option<String>) {
        let engine = self.clone();
        self.shared.runtime.spawn_blocking(move || {
            match gamenode_platform::installations::pick_executable() {
                Ok(path) => {
                    if let Some(id) = repair {
                        engine.repair_local_game(id, path);
                    } else {
                        engine.add_local_executable(path);
                    }
                }
                Err(error) => {
                    let mut state = engine.lock();
                    state.local.error = Some(error.to_string());
                    engine.publish(&mut state);
                }
            }
        });
    }
}

impl Engine {
    pub(super) fn queue_local_backup(&self, state: &mut State, key: String, period: String, last_exit: i64) {
        use gamenode_core::drm_free::scheduler::automatic_job;
        let Some(installation)=state.local.registrations.iter().find(|r|r.key()==key) else { return; };
        let Some(profile)=state.local.profiles.iter().find(|p|p.installation==installation.id) else { return; };
        let Some(job)=automatic_job(gamenode_platform::random::session_id(),installation,profile,&state.local.settings,period,last_exit) else { return; };
        if state.local.jobs.iter().any(|j|j.installation==job.installation && j.profile==job.profile && j.revision==job.revision && j.play_period==job.play_period) { return; }
        state.local.jobs.push(job.clone());
        if let Some(store)=self.shared.ports.local_games.clone() { self.shared.storage.submit(move || { if let Err(error)=store.save_job(&job) { tracing::warn!(%error,"could not persist backup job"); } }); }
    }
    pub(super) fn drive_local_backups(&self,state:&mut State,now:i64) {
        use gamenode_core::drm_free::{BackupOperation,BackupRequest,JobState};
        if state.local_backup_busy || self.shared.ports.backups.is_none() { return; }
        let Some(job)=state.local.jobs.iter().find(|j|j.state==JobState::Queued && j.ready_at<=now).cloned() else { return; };
        let Some(profile)=state.local.profiles.iter().find(|p|p.id==job.profile && p.revision==job.revision).cloned() else { return; };
        let Some(installation)=state.local.registrations.iter().find(|r|r.id==job.installation && !r.removed).cloned() else { return; };
        if state.playtime.tracker.running().contains(&installation.key()) { return; }
        if job.play_period.is_some() && (!state.local.settings.automatic || profile.automatic==Some(false)) { return; }
        state.local_backup_busy=true;
        let engine=self.clone();
        self.shared.runtime.spawn_blocking(move || {
            let store=engine.shared.ports.local_games.as_ref().unwrap();
            let mut job=job;
            let result=(|| {
                let processes=engine.shared.ports.processes.list().map_err(|_|LocalError::Failed("cannot confirm game is stopped".into()))?;
                let equal=|a:&str,b:&str| gamenode_core::provider::path::normalize(a).to_lowercase()==gamenode_core::provider::path::normalize(b).to_lowercase();
                if processes.iter().any(|p|installation.gameplay.iter().any(|exe|equal(exe,&p.exe_path))) { return Err(LocalError::Failed("game is running".into())); }
                job.state=JobState::Running;store.save_job(&job).map_err(local_storage)?;
                let request=BackupRequest { installation,profile,destination:job.destination.clone(),operation:BackupOperation::Backup };
                engine.shared.ports.backups.as_ref().unwrap().execute(&request,&std::sync::atomic::AtomicBool::new(false))
            })();
            match result {
                Ok(report)=> { job.state=report.state;job.error=None; },
                Err(error)=>gamenode_core::drm_free::scheduler::failed(&mut job,error.to_string(),now),
            }
            if let Err(error)=store.save_job(&job) { tracing::warn!(%error,"could not save backup outcome"); }
            let view=engine.load_local_view();let mut state=engine.lock();state.local=view;state.local_backup_busy=false;engine.publish(&mut state);
        });
    }
}

impl Engine {
    pub fn enrich_local_games(&self) {
        let (account,credential,registrations)={
            let state=self.lock();
            (state.library.account().map(|a|a.id.clone()),state.machine.verified_credential().cloned(),state.local.registrations.clone())
        };
        if registrations.is_empty() { return; }
        let engine=self.clone();
        self.shared.runtime.spawn_blocking(move || {
            use gamenode_core::drm_free::{identity,Inference};
            let Some((store,host))=engine.shared.ports.local_games.as_ref().zip(engine.shared.ports.installations.as_ref()) else { return; };
            let data=engine.shared.ports.covers_dir.parent().unwrap_or(std::path::Path::new("."));
            let snapshot=gamenode_platform::manifest_cache::ManifestCache::bundled(data).and_then(|cache|cache.load()).ok();
            let existing=store.profiles().unwrap_or_default();
            for registration in registrations.into_iter().filter(|r|!r.removed).take(100) {
                let evidence=host.evidence(&registration).unwrap_or_default();
                let Some(identity)=identity::infer(&evidence) else { continue; };
                if identity.strong && !existing.iter().any(|p|p.installation==registration.id) {
                    if let Some(snapshot)=&snapshot {
                        let profiles=snapshot.index.exact(&identity.title);
                        if let [game]=profiles.as_slice() {
                            if let Ok(profile)=identity::profile(game,&registration,&snapshot.hash,gamenode_platform::random::session_id()) {
                                if let Err(error)=store.save_profile(&profile) { tracing::warn!(%error,"could not save inferred save profile"); }
                            }
                        }
                    }
                }
                let (Some(account),Some(credential))=(account.clone(),credential.clone()) else { continue; };
                let e=engine.clone();let registration=registration.clone();let evidence=evidence.clone();
                engine.shared.runtime.spawn(async move {
                    let response=e.shared.ports.games.search_page(&credential,&identity.title).await;
                    let Ok(page)=response else { return; };
                    let game_id=identity::unique_exact(&identity,&page.games,page.complete==Some(true));
                    let game=game_id.and_then(|id|page.games.iter().find(|g|g.id==id)).map(|g|gamenode_core::library::GameRecord { game_id:g.id,name:g.name.clone(),release_year:g.release_year,cover_url:g.cover_url.clone(),cover_file:None,cover_failed_at:None });
                    let valid={let state=e.lock();state.library.account().is_some_and(|a|a.id==account) && state.local.registrations.iter().any(|r|r.id==registration.id && r.revision==registration.revision && !r.removed)};
                    if !valid { return; }
                    let inference=Inference { installation:registration.id.clone(),revision:registration.revision,title:identity.title,evidence,game_id,account:account.clone() };
                    let persist=e.clone();let saved_game=game.clone();
                    e.shared.runtime.spawn_blocking(move || {
                        if let Some(store)=&persist.shared.ports.local_games { let _=store.save_inference(&inference); }
                        if let Some(game)=saved_game { let _=persist.shared.ports.library.save_games(&[game]); }
                    });
                    let mut state=e.lock();
                    if state.library.account().is_some_and(|a|a.id==account) && state.local.registrations.iter().any(|r|r.id==registration.id && r.revision==registration.revision && !r.removed) {
                        state.library.set_local_inference(registration.key(),game_id,game);e.publish(&mut state);
                    }
                });
            }
            engine.refresh_local_games();
        });
    }
}
