#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Store error: {0}")]
    Store(#[from] operator_store::StoreError),
    #[error("Core error: {0}")]
    Core(#[from] operator_core::error::CoreError),
    #[error("System exec error: {0}")]
    SystemExec(#[from] operator_exec_system::executor::SystemExecError),
    #[error("IPC error: {0}")]
    Ipc(#[from] operator_ipc::IpcError),
    #[error("Validation failed: {0}")]
    Validation(String),
    #[error("Policy denied: {0}")]
    PolicyDenied(String),
    #[error("Cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}
