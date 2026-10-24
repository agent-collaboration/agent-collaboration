//! Claude 2.1.310 Zte/R1e and local-settings root qualification.
use super::*;
use std::{fs, io::Read, path::Path};

pub(super) struct ClaudeProjectContext {
    pub skill_directories: Vec<PathBuf>,
    /// Native merges the legacy cwd local file first, canonical file last.
    pub local_settings_files: Vec<PathBuf>,
}

pub(super) fn read_project_context(
    home: &Path,
    workspace: &Path,
) -> Result<ClaudeProjectContext, CatalogueReadFailure> {
    let mut directories = Vec::new();
    let mut current = workspace;
    let mut git_root = None;
    loop {
        if current == home {
            break;
        }
        directories.push(current.join(".claude"));
        let marker = current.join(".git");
        match fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.is_dir() || metadata.is_file() => {
                git_root = Some(current.to_owned());
                break;
            }
            Ok(_) => return Err(CatalogueReadFailure::InvalidScope),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(CatalogueReadFailure::Unavailable),
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent;
    }
    let canonical_root = match &git_root {
        Some(root) => canonical_worktree_root(root)?,
        None => workspace.to_owned(),
    };
    // R1e adds a linked worktree's common-root sources after its own roots.
    if git_root
        .as_ref()
        .is_some_and(|root| root != &canonical_root)
    {
        let common = canonical_root.join(".claude");
        if !directories.contains(&common) {
            directories.push(common);
        }
    }
    let local_root = if canonical_root != workspace
        && canonical_root != home
        && local_root_owned_by_current_user(&canonical_root)?
    {
        canonical_root
    } else {
        workspace.to_owned()
    };
    let mut local_settings_files = vec![workspace.join(".claude/settings.local.json")];
    if local_root != workspace {
        local_settings_files.push(local_root.join(".claude/settings.local.json"));
    }
    Ok(ClaudeProjectContext {
        skill_directories: directories,
        local_settings_files,
    })
}

fn canonical_worktree_root(root: &Path) -> Result<PathBuf, CatalogueReadFailure> {
    let marker = root.join(".git");
    if marker.is_dir() {
        return Ok(root.to_owned());
    }
    let text = read_pointer(&marker)?;
    let Some(pointer) = text.trim().strip_prefix("gitdir:") else {
        return Ok(root.to_owned());
    };
    let git_dir = match fs::canonicalize(root.join(pointer.trim())) {
        Ok(path) => path,
        Err(_) => return Ok(root.to_owned()),
    };
    let common_file = git_dir.join("commondir");
    let common = match read_pointer(&common_file) {
        Ok(text) => match fs::canonicalize(git_dir.join(text.trim())) {
            Ok(path) => path,
            Err(_) => return Ok(root.to_owned()),
        },
        Err(CatalogueReadFailure::Unavailable) => return Ok(root.to_owned()),
        Err(reason) => return Err(reason),
    };
    // Qn only recognizes the linked-worktree layout with a matching back
    // pointer. Submodule/unknown layouts retain their physical root natively.
    if git_dir.parent() != Some(common.join("worktrees").as_path()) {
        return Ok(root.to_owned());
    }
    let back = read_pointer(&git_dir.join("gitdir"))?;
    if fs::canonicalize(git_dir.join(back.trim())).ok().as_deref() != Some(marker.as_path()) {
        return Ok(root.to_owned());
    }
    if common.file_name().is_some_and(|name| name == ".git") {
        return common
            .parent()
            .map(Path::to_owned)
            .ok_or(CatalogueReadFailure::InvalidScope);
    }
    // Native Qn also recognizes separate git dirs. This shape needs its own
    // path-containment qualification before it can supply another setup root.
    Err(CatalogueReadFailure::InvalidScope)
}

fn read_pointer(path: &Path) -> Result<String, CatalogueReadFailure> {
    if fs::symlink_metadata(path)
        .map_err(|_| CatalogueReadFailure::Unavailable)?
        .file_type()
        .is_symlink()
    {
        return Err(CatalogueReadFailure::InvalidScope);
    }
    let file = fs::File::open(path).map_err(|_| CatalogueReadFailure::Unavailable)?;
    let metadata = file
        .metadata()
        .map_err(|_| CatalogueReadFailure::Unavailable)?;
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err(CatalogueReadFailure::InvalidNativeData);
    }
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| CatalogueReadFailure::Unavailable)?;
    if bytes.len() > 4096 {
        return Err(CatalogueReadFailure::InvalidNativeData);
    }
    let text = String::from_utf8(bytes).map_err(|_| CatalogueReadFailure::InvalidNativeData)?;
    if text
        .chars()
        .any(|character| character == '\0' || character == '\u{1b}')
    {
        return Err(CatalogueReadFailure::InvalidNativeData);
    }
    Ok(text)
}

#[cfg(unix)]
fn local_root_owned_by_current_user(root: &Path) -> Result<bool, CatalogueReadFailure> {
    use std::os::unix::fs::MetadataExt;
    let uid = rustix::process::geteuid().as_raw();
    for path in [root.to_owned(), root.join(".git"), root.join(".claude")] {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.uid() != uid => return Ok(false),
            Ok(_) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound && path == root.join(".claude") => {
            }
            Err(_) => return Err(CatalogueReadFailure::Unavailable),
        }
    }
    Ok(true)
}

#[cfg(not(unix))]
fn local_root_owned_by_current_user(_root: &Path) -> Result<bool, CatalogueReadFailure> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_capabilities_claude_nested_workspace_uses_physical_skill_root_and_owned_canonical_local_settings()
     {
        let fixture = tempfile::tempdir_in("/tmp").expect("fixture");
        let root = fs::canonicalize(fixture.path()).expect("physical root");
        let home = root.join("home");
        let repository = root.join("repository");
        let workspace = repository.join("nested/workspace");
        fs::create_dir_all(repository.join(".git")).expect("git directory");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&workspace).expect("workspace");
        let context = read_project_context(&home, &workspace).expect("native setup");
        assert_eq!(
            context.skill_directories,
            vec![
                workspace.join(".claude"),
                repository.join("nested/.claude"),
                repository.join(".claude")
            ]
        );
        assert_eq!(
            context.local_settings_files,
            vec![
                workspace.join(".claude/settings.local.json"),
                repository.join(".claude/settings.local.json")
            ]
        );
    }

    #[test]
    fn native_capabilities_claude_linked_worktree_adds_common_skill_root_and_checks_back_pointer() {
        let fixture = tempfile::tempdir_in("/tmp").expect("fixture");
        let root = fs::canonicalize(fixture.path()).expect("physical root");
        let home = root.join("home");
        let main = root.join("main");
        let worktree = root.join("worktree");
        let gitdir = main.join(".git/worktrees/linked");
        fs::create_dir_all(&gitdir).expect("git worktree directory");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&worktree).expect("worktree");
        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .expect("git pointer");
        fs::write(gitdir.join("commondir"), "../..\n").expect("common pointer");
        fs::write(
            gitdir.join("gitdir"),
            format!("{}\n", worktree.join(".git").display()),
        )
        .expect("back pointer");
        let context = read_project_context(&home, &worktree).expect("qualified native worktree");
        assert_eq!(
            context.skill_directories,
            vec![worktree.join(".claude"), main.join(".claude")]
        );
        assert_eq!(
            context.local_settings_files,
            vec![
                worktree.join(".claude/settings.local.json"),
                main.join(".claude/settings.local.json")
            ]
        );
        fs::write(gitdir.join("gitdir"), "/not-the-worktree/.git\n").expect("wrong back pointer");
        let context =
            read_project_context(&home, &worktree).expect("native falls back to physical root");
        assert_eq!(context.skill_directories, vec![worktree.join(".claude")]);
        assert_eq!(
            context.local_settings_files,
            vec![worktree.join(".claude/settings.local.json")]
        );
    }
}
