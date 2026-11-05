//! Serializes engine work requested by UI callbacks away from Slint's event loop.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use crate::engine::Engine;
use gamenode_core::drm_free::Registration;
use gamenode_core::library::{Input as LibInput, LaunchKind};

type Command = Box<dyn FnOnce(&Engine) + Send + 'static>;

#[derive(Clone)]
pub struct UiCommands {
    sender: mpsc::Sender<Command>,
}

impl UiCommands {
    pub fn new(engine: Engine) -> Self {
        let (sender, receiver) = mpsc::channel::<Command>();
        std::thread::Builder::new()
            .name("gamenode-ui-commands".into())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    command(&engine);
                }
            })
            .expect("could not start UI command worker");
        Self { sender }
    }

    /// Enqueue work in callback order; no engine lock or view computation runs on the UI thread.
    pub fn submit(&self, command: impl FnOnce(&Engine) + Send + 'static) {
        let _ = self.sender.send(Box::new(command));
    }

    /// Queue a periodic command only when its prior run is no longer pending.
    pub fn submit_coalesced(
        &self,
        pending: &Arc<AtomicBool>,
        command: impl FnOnce(&Engine) + Send + 'static,
    ) {
        if pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let pending = pending.clone();
        self.submit(move |engine| {
            command(engine);
            pending.store(false, Ordering::Release);
        });
    }

    /// Wait until all commands submitted before this call have finished.
    pub fn flush(&self) {
        let (done, wait) = mpsc::channel();
        self.submit(move |_| {
            let _ = done.send(());
        });
        let _ = wait.recv();
    }

    pub fn dispatch(&self, input: gamenode_core::Input) {
        self.submit(move |e| e.dispatch(input));
    }
    pub fn library_input(&self, input: LibInput) {
        self.submit(move |e| e.library_input(input));
    }
    pub fn resolve_local_claim(&self, transfer: bool) {
        self.submit(move |e| e.resolve_local_claim(transfer));
    }
    pub fn cancel_local_claim(&self) {
        self.submit(|e| e.cancel_local_claim());
    }
    pub fn pick_local_executable(&self, repair: Option<String>) {
        self.submit(move |e| e.pick_local_executable(repair));
    }
    pub fn remove_local_game(&self, id: String) {
        self.submit(move |e| e.remove_local_game(id));
    }
    pub fn edit_local_game(&self, value: Registration) {
        self.submit(move |e| e.edit_local_game(value));
    }
    pub fn backup_local_now(&self, id: String) {
        self.submit(move |e| e.backup_local_now(id));
    }
    pub fn set_local_backup_settings(&self, automatic: bool, destination: String) {
        self.submit(move |e| e.set_local_backup_settings(automatic, destination));
    }
    pub fn set_local_profile_automatic(&self, id: String, automatic: bool) {
        self.submit(move |e| e.set_local_profile_automatic(id, automatic));
    }
    pub fn configure_local_saves(&self, id: String, location: String) {
        self.submit(move |e| e.configure_local_saves(id, location));
    }
    pub fn pick_local_saves(&self, id: String, folder: bool) {
        self.submit(move |e| e.pick_local_saves(id, folder));
    }
    pub fn choose_local_save_profile(&self, id: String, title: String) {
        self.submit(move |e| e.choose_local_save_profile(id, title));
    }
    pub fn watch_local_folder(&self, path: String) {
        self.submit(move |e| e.watch_local_folder(path));
    }
    pub fn pick_local_watch_folder(&self) {
        self.submit(|e| e.pick_local_watch_folder());
    }
    pub fn remove_local_watch(&self, id: String) {
        self.submit(move |e| e.remove_local_watch(id));
    }
    pub fn set_local_candidate_executable(&self, fingerprint: String, executable: String) {
        self.submit(move |e| e.set_local_candidate_executable(fingerprint, executable));
    }
    pub fn rescan_local_folders(&self) {
        self.submit(|e| e.rescan_local_folders());
    }
    pub fn select_local_candidate(&self, fingerprint: String) {
        self.submit(move |e| e.select_local_candidate(fingerprint));
    }
    pub fn review_selected_local_candidates(&self, add: bool) {
        self.submit(move |e| e.review_selected_local_candidates(add));
    }
    pub fn reset_local_ignores(&self) {
        self.submit(|e| e.reset_local_ignores());
    }
    pub fn load_local_history(&self, id: String) {
        self.submit(move |e| e.load_local_history(id));
    }
    pub fn preview_local_restore(&self, key: String) {
        self.submit(move |e| e.preview_local_restore(key));
    }
    pub fn confirm_local_restore(&self) {
        self.submit(|e| e.confirm_local_restore());
    }
    pub fn rescan(&self) {
        self.submit(|e| e.rescan());
    }
    pub fn dismiss_launch(&self) {
        self.submit(|e| e.dismiss_launch());
    }
    pub fn open_entry(&self, key: String, kind: LaunchKind) {
        self.submit(move |e| e.open_entry(key, kind));
    }
}
