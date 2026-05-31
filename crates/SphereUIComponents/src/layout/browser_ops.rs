use gpui::Context;

use std::path::PathBuf;

use crate::components::file_browser::read_directory;

use super::StudioLayout;
impl StudioLayout {
    /// Run a single-level directory scan on the GPUI background executor,
    /// then push the result back into `file_browser.index` on the UI
    /// thread. Never blocks render — this is the only place `read_dir`
    /// is allowed to happen at runtime.
    pub(super) fn spawn_directory_load(cx: &mut Context<Self>, path: PathBuf) {
        let started = std::time::Instant::now();
        let path_for_log = path.clone();
        eprintln!("[indexer] load requested: {}", path_for_log.display());
        cx.spawn(async move |this, cx| {
            let scan_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { read_directory(&scan_path) })
                .await;
            let elapsed = started.elapsed();
            let _ = this.update(cx, move |this, cx| {
                match result {
                    (entries, None) => {
                        eprintln!(
                            "[indexer] load completed: {} ({} entries, {} ms)",
                            path.display(),
                            entries.len(),
                            elapsed.as_millis()
                        );
                        this.file_browser.apply_loaded(path, entries);
                    }
                    (_, Some(error)) => {
                        eprintln!(
                            "[indexer] load failed: {} -> {} ({} ms)",
                            path.display(),
                            error,
                            elapsed.as_millis()
                        );
                        this.file_browser.apply_error(path, error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}
