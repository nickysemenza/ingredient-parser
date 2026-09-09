//! Prevent simultaneous writes to the same durable run, including path aliases.
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub struct Operations(Arc<Mutex<HashSet<PathBuf>>>);

pub struct RunGuard {
    paths: Vec<PathBuf>,
    operations: Operations,
}

fn identity(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return path.canonicalize().map_err(|error| error.to_string());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path.file_name().ok_or("Choose a run file path")?;
    Ok(parent
        .canonicalize()
        .map_err(|error| error.to_string())?
        .join(name))
}

impl Operations {
    pub fn acquire(&self, paths: &[&str]) -> Result<RunGuard, String> {
        let paths = paths
            .iter()
            .map(|path| identity(Path::new(path)))
            .collect::<Result<Vec<_>, _>>()?;
        let paths: Vec<_> = paths
            .into_iter()
            .flat_map(|path| [path.clone(), path.with_extension("review.json")])
            .collect();
        let mut active = self
            .0
            .lock()
            .map_err(|_| "Run operation state is unavailable")?;
        if paths.iter().any(|path| active.contains(path)) {
            return Err("This run is busy. Wait for its current operation to finish.".into());
        }
        active.extend(paths.iter().cloned());
        Ok(RunGuard {
            paths,
            operations: self.clone(),
        })
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.operations.0.lock() {
            for path in &self.paths {
                active.remove(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_conflict_and_guards_release_after_errors() -> Result<(), String> {
        let operations = Operations::default();
        let guard = operations.acquire(&["Cargo.toml"])?;
        assert!(operations.acquire(&["./Cargo.toml"]).is_err());
        drop(guard);
        assert!(operations.acquire(&["./Cargo.toml"]).is_ok());
        Ok(())
    }

    #[test]
    fn different_run_extensions_share_a_sidecar_lock() -> Result<(), String> {
        let operations = Operations::default();
        let _guard = operations.acquire(&["book.json"])?;
        assert!(operations.acquire(&["book.run"]).is_err());
        Ok(())
    }

    #[test]
    fn pending_output_paths_are_also_locked() -> Result<(), String> {
        let operations = Operations::default();
        let _guard = operations.acquire(&["not-created-run.json"])?;
        assert!(operations.acquire(&["./not-created-run.json"]).is_err());
        Ok(())
    }
}
