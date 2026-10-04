use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::store::agent_dir;

pub struct Location {
    pub root: PathBuf,
    pub tasks: PathBuf,
}

pub fn locate(start: &Path) -> Location {
    locate_in(start, &agent_dir())
}

/// Nearest ancestor of `start` that owns an OmO task store. Newer OmO keeps it in
/// `<agent>/projects/<name>-<sha256(path)[..12]>/senpi-task`, older OmO in `<project>/.omo/senpi-task`;
/// when both exist the one whose `tasks/` changed last wins.
pub fn locate_in(start: &Path, agent: &Path) -> Location {
    for dir in start.ancestors() {
        let legacy = legacy_store(dir);
        let found = [project_store(agent, dir), Some(legacy)]
            .into_iter()
            .flatten()
            .filter(|p| p.is_dir())
            .max_by_key(|p| modified(&p.join("tasks")));
        if let Some(tasks) = found {
            return Location {
                root: dir.to_path_buf(),
                tasks,
            };
        }
    }
    Location {
        root: start.to_path_buf(),
        tasks: legacy_store(start),
    }
}

pub fn project_store(agent: &Path, project: &Path) -> Option<PathBuf> {
    let name = project.file_name()?.to_string_lossy();
    let digest = Sha256::digest(project.to_string_lossy().as_bytes());
    let hash: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    Some(agent.join("projects").join(format!("{name}-{hash}")).join("senpi-task"))
}

fn legacy_store(project: &Path) -> PathBuf {
    project.join(".omo").join("senpi-task")
}

fn modified(p: &Path) -> Option<SystemTime> {
    p.metadata().and_then(|m| m.modified()).ok()
}

#[cfg(test)]
#[path = "locate_tests.rs"]
mod tests;
