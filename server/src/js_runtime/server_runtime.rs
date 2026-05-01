use std::future::Future;
use std::sync::OnceLock;

static SERVER_RUNTIME_HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();

pub fn init(handle: tokio::runtime::Handle) {
    let _ = SERVER_RUNTIME_HANDLE.set(handle);
}

pub async fn spawn_on_server_runtime<F, T>(future: F) -> Result<T, String>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    // JS workers run inside dedicated short-lived/current-thread Tokio runtimes.
    // Host calls that may touch server services or DB pools must execute on the
    // long-lived server runtime so runtime-bound IO resources are not recycled
    // into the wrong executor.
    let handle = SERVER_RUNTIME_HANDLE
        .get()
        .ok_or_else(|| "server runtime handle is not initialized".to_owned())?;

    let mut task = AbortOnDrop {
        handle: handle.spawn(future),
    };

    (&mut task.handle)
        .await
        .map_err(|e| format!("server runtime task failed: {e}"))
}

struct AbortOnDrop<T> {
    handle: tokio::task::JoinHandle<T>,
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if !self.handle.is_finished() {
            self.handle.abort();
        }
    }
}
