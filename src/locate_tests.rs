use std::fs;
use std::path::Path;

use super::*;

#[test]
fn project_store_matches_omo_naming() {
    let dir = project_store(Path::new("/agent"), Path::new("/home/pawis/miti-workspace/omo-scope"));
    assert_eq!(
        dir,
        Some(PathBuf::from("/agent/projects/omo-scope-42da2a11aad3/senpi-task"))
    );
    assert_eq!(project_store(Path::new("/agent"), Path::new("/")), None);
}

#[test]
fn locate_prefers_nearest_store_of_either_layout() {
    let base = std::env::temp_dir().join(format!("omo-scope-locate-{}", std::process::id()));
    let agent = base.join("agent");
    let outer = base.join("ws");
    let inner = outer.join("repo");
    let deep = inner.join("src");
    fs::create_dir_all(&deep).unwrap();
    fs::create_dir_all(outer.join(".omo/senpi-task/tasks")).unwrap();

    let loc = locate_in(&deep, &agent);
    assert_eq!(loc.root, outer);
    assert_eq!(loc.tasks, outer.join(".omo/senpi-task"));

    let fresh = project_store(&agent, &inner).unwrap();
    fs::create_dir_all(fresh.join("tasks")).unwrap();
    let loc = locate_in(&deep, &agent);
    assert_eq!(loc.root, inner);
    assert_eq!(loc.tasks, fresh);

    fs::remove_dir_all(&base).unwrap();
    let loc = locate_in(&deep, &agent);
    assert_eq!(loc.root, deep);
    assert_eq!(loc.tasks, deep.join(".omo/senpi-task"));
}
