//! Platform process inspection. Linux tracking is intentionally unavailable in the MVP.

#[cfg(not(windows))]
mod unsupported {
    use gamenode_core::ports::{ProcessError, ProcessInfo, Processes};
    #[derive(Default)]
    pub struct HostProcesses;
    impl HostProcesses {
        pub fn new() -> Self {
            Self
        }
    }
    impl Processes for HostProcesses {
        fn list(&self) -> Result<Vec<ProcessInfo>, ProcessError> {
            Err(ProcessError::Unsupported)
        }
        fn watch(
            &self,
            _: &ProcessInfo,
            _: Box<dyn FnOnce(std::time::SystemTime) + Send>,
        ) -> Result<(), ProcessError> {
            Err(ProcessError::Unsupported)
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn tracking_is_unavailable() {
            assert_eq!(HostProcesses.list(), Err(ProcessError::Unsupported));
            let process = ProcessInfo {
                pid: 1,
                created_at: std::time::UNIX_EPOCH,
                exe_path: "/bin/test".into(),
                parent: None,
            };
            assert_eq!(
                HostProcesses.watch(&process, Box::new(|_| panic!("unavailable"))),
                Err(ProcessError::Unsupported)
            );
        }
    }
}
#[cfg(not(windows))]
pub use unsupported::HostProcesses;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::HostProcesses;
