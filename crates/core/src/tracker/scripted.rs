//! Deterministic process adapter for engine and tracker tests. No host process access.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::SystemTime;

use crate::ports::{ProcessError, ProcessInfo, Processes};

type Identity = (u32, SystemTime);
type ExitCallback = Box<dyn FnOnce(SystemTime) + Send>;
#[derive(Default)]
struct State {
    processes: BTreeMap<Identity, ProcessInfo>,
    watches: BTreeMap<Identity, Vec<ExitCallback>>,
}
#[derive(Default)]
pub struct ScriptedProcesses(Mutex<State>);

impl ScriptedProcesses {
    pub fn watch_count(&self) -> usize {
        self.0.lock().unwrap().watches.values().map(Vec::len).sum()
    }

    pub fn start(&self, process: ProcessInfo) {
        self.0
            .lock()
            .unwrap()
            .processes
            .insert((process.pid, process.created_at), process);
    }

    pub fn exit(&self, process: &ProcessInfo, at: SystemTime) {
        let callbacks = {
            let mut state = self.0.lock().unwrap();
            let identity = (process.pid, process.created_at);
            state.processes.remove(&identity);
            state.watches.remove(&identity).unwrap_or_default()
        };
        for callback in callbacks {
            callback(at);
        }
    }
}
impl Processes for ScriptedProcesses {
    fn list(&self) -> Result<Vec<ProcessInfo>, ProcessError> {
        Ok(self.0.lock().unwrap().processes.values().cloned().collect())
    }
    fn watch(&self, process: &ProcessInfo, on_exit: ExitCallback) -> Result<(), ProcessError> {
        let mut state = self.0.lock().unwrap();
        let identity = (process.pid, process.created_at);
        if !state.processes.contains_key(&identity) {
            return Err(ProcessError::Failed);
        }
        state.watches.entry(identity).or_default().push(on_exit);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, UNIX_EPOCH};
    #[test]
    fn start_exit_and_reused_pid_have_separate_lifetimes() {
        let double = ScriptedProcesses::default();
        let first = ProcessInfo {
            pid: 9,
            created_at: UNIX_EPOCH,
            exe_path: "/games/a/x".into(),
            parent: None,
        };
        double.start(first.clone());
        assert_eq!(
            double.list().unwrap().as_slice(),
            std::slice::from_ref(&first)
        );
        let exits = Arc::new(Mutex::new(Vec::new()));
        let observed = exits.clone();
        double
            .watch(
                &first,
                Box::new(move |at| observed.lock().unwrap().push(at)),
            )
            .unwrap();
        let at = UNIX_EPOCH + Duration::from_secs(10);
        double.exit(&first, at);
        assert!(double.list().unwrap().is_empty());
        let second = ProcessInfo {
            created_at: at,
            ..first.clone()
        };
        double.start(second.clone());
        double.exit(&first, at);
        assert_eq!(double.list().unwrap(), [second]);
        assert_eq!(*exits.lock().unwrap(), [at]);
    }
}
