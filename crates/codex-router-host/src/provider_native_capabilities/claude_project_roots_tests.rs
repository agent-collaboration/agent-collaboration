//! Native Zte/R1e outcomes through real linked-worktree files and Host projection.
use super::*;

struct LinkedProjectFiles {
    _directory: tempfile::TempDir,
    home: PathBuf,
    main: PathBuf,
    worktree: PathBuf,
    git_dir: PathBuf,
}

impl LinkedProjectFiles {
    fn new() -> Self {
        let directory = tempfile::tempdir_in("/tmp").expect("fixture");
        let root = fs::canonicalize(directory.path()).expect("physical fixture");
        let home = root.join("home");
        let main = root.join("main");
        let worktree = root.join("worktree");
        let git_dir = main.join(".git/worktrees/linked");
        for path in [&home, &worktree, &git_dir] {
            fs::create_dir_all(path).expect("fixture directory");
        }
        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .expect("worktree pointer");
        fs::write(git_dir.join("commondir"), "../..\n").expect("common pointer");
        fs::write(
            git_dir.join("gitdir"),
            format!("{}\n", worktree.join(".git").display()),
        )
        .expect("matching back pointer");
        Self {
            _directory: directory,
            home,
            main,
            worktree,
            git_dir,
        }
    }

    fn write(&self, common: bool, relative: &str) {
        let root = if common { &self.main } else { &self.worktree };
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("native kind directory");
        fs::write(path, "Native fixture source").expect("native file");
    }

    fn read_names(&self) -> Vec<String> {
        let section = read_native_claude_skill_section(
            &self.home,
            &self.home.join(".claude"),
            &self.worktree,
            CatalogueSection::Supported {
                items: Vec::new(),
                errors: Vec::new(),
                enumeration: SourceEnumeration::Accounted,
            },
            &CancellationToken::new(),
        )
        .expect("real filesystem catalogue");
        let CatalogueSection::Supported {
            items,
            enumeration,
            errors,
        } = section
        else {
            panic!("supported catalogue")
        };
        assert_eq!(enumeration, SourceEnumeration::Accounted);
        assert!(
            errors.is_empty(),
            "unexpected native source errors: {errors:?}"
        );
        assert!(
            items
                .iter()
                .all(|item| item.native_scope == NativeScope::Workspace)
        );
        let mut names = items
            .into_iter()
            .map(|item| item.name.as_str().to_owned())
            .collect::<Vec<_>>();
        names.sort();
        names
    }
}

#[test]
fn native_capabilities_claude_per_kind_physical_skills_suppress_common_skills() {
    let files = LinkedProjectFiles::new();
    files.write(false, ".claude/skills/physical-skill/SKILL.md");
    files.write(true, ".claude/skills/common-skill/SKILL.md");

    let names = files.read_names();

    assert_eq!(names, ["physical-skill"]);
}

#[test]
fn native_capabilities_claude_per_kind_common_only_skills_remain_visible() {
    let files = LinkedProjectFiles::new();
    files.write(true, ".claude/skills/common-skill/SKILL.md");

    let names = files.read_names();

    assert_eq!(names, ["common-skill"]);
}

#[test]
fn native_capabilities_claude_per_kind_physical_skills_keep_common_commands() {
    let files = LinkedProjectFiles::new();
    files.write(false, ".claude/skills/physical-skill/SKILL.md");
    files.write(true, ".claude/skills/common-skill/SKILL.md");
    files.write(true, ".claude/commands/common-command.md");

    let names = files.read_names();

    assert_eq!(names, ["common-command", "physical-skill"]);
}

#[test]
fn native_capabilities_claude_per_kind_physical_commands_keep_common_skills() {
    let files = LinkedProjectFiles::new();
    files.write(false, ".claude/commands/physical-command.md");
    files.write(true, ".claude/commands/common-command.md");
    files.write(true, ".claude/skills/common-skill/SKILL.md");

    let names = files.read_names();

    assert_eq!(names, ["common-skill", "physical-command"]);
}

#[test]
fn native_capabilities_claude_per_kind_invalid_back_pointer_keeps_physical_sources_only() {
    let files = LinkedProjectFiles::new();
    files.write(false, ".claude/skills/physical-skill/SKILL.md");
    files.write(true, ".claude/skills/common-skill/SKILL.md");
    files.write(true, ".claude/commands/common-command.md");
    fs::write(files.git_dir.join("gitdir"), "/not-this-worktree/.git\n")
        .expect("invalid back pointer");

    let names = files.read_names();

    assert_eq!(names, ["physical-skill"]);
}
