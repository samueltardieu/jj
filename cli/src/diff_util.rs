use futures::StreamExt;
use jj_lib::conflicts::{materialize_tree_value, MaterializedTreeValue};
use jj_lib::merge::MergedTreeValue;
use jj_lib::merged_tree::{MergedTree, TreeDiffStream};
use jj_lib::repo::Repo;
use jj_lib::{diff, files, rewrite};
use pollster::FutureExt;
                let tree_diff = from_tree.diff_stream(to_tree, matcher);
                let tree_diff = from_tree.diff_stream(to_tree, matcher);
                let tree_diff = from_tree.diff_stream(to_tree, matcher);
                let tree_diff = from_tree.diff_stream(to_tree, matcher);
                let tree_diff = from_tree.diff_stream(to_tree, matcher);
fn diff_content(path: &RepoPath, value: MaterializedTreeValue) -> Result<Vec<u8>, CommandError> {
    match value {
        MaterializedTreeValue::Absent => Ok(vec![]),
        MaterializedTreeValue::File { mut reader, .. } => {
            let mut contents = vec![];
            reader.read_to_end(&mut contents)?;
            Ok(contents)
        MaterializedTreeValue::Symlink { id: _, target } => Ok(target.into_bytes()),
        MaterializedTreeValue::GitSubmodule(id) => {
        MaterializedTreeValue::Conflict { id: _, contents } => Ok(contents),
        MaterializedTreeValue::Tree(id) => {
            panic!("Unexpected tree with id {id:?} in diff at path {path:?}");
fn basic_diff_file_type(value: &MaterializedTreeValue) -> &'static str {
    match value {
        MaterializedTreeValue::Absent => {
        MaterializedTreeValue::File { executable, .. } => {
                "executable file"
                "regular file"
        MaterializedTreeValue::Symlink { .. } => "symlink",
        MaterializedTreeValue::Tree(_) => "tree",
        MaterializedTreeValue::GitSubmodule(_) => "Git submodule",
        MaterializedTreeValue::Conflict { .. } => "conflict",
    mut tree_diff: TreeDiffStream,
    let store = workspace_command.repo().store();
    async {
        while let Some((path, diff)) = tree_diff.next().await {
            let ui_path = workspace_command.format_file_path(&path);
            let (left_value, right_value) = diff?;
            let left_value = materialize_tree_value(store, &path, left_value).block_on()?;
            let right_value = materialize_tree_value(store, &path, right_value).block_on()?;
            if left_value.is_absent() {
                let description = basic_diff_file_type(&right_value);
                writeln!(
                    formatter.labeled("header"),
                    "Added {description} {ui_path}:"
                )?;
                let right_content = diff_content(&path, right_value)?;
                if right_content.is_empty() {
                    writeln!(formatter.labeled("empty"), "    (empty)")?;
                } else {
                    show_color_words_diff_hunks(&[], &right_content, formatter)?;
            } else if right_value.is_present() {
                let description = match (&left_value, &right_value) {
                    (
                        MaterializedTreeValue::File {
                            executable: left_executable,
                            ..
                        },
                        MaterializedTreeValue::File {
                            executable: right_executable,
                            ..
                        },
                    ) => {
                        if *left_executable && *right_executable {
                            "Modified executable file".to_string()
                        } else if *left_executable {
                            "Executable file became non-executable at".to_string()
                        } else if *right_executable {
                            "Non-executable file became executable at".to_string()
                        } else {
                            "Modified regular file".to_string()
                        }
                    }
                    (
                        MaterializedTreeValue::Conflict { .. },
                        MaterializedTreeValue::Conflict { .. },
                    ) => "Modified conflict in".to_string(),
                    (MaterializedTreeValue::Conflict { .. }, _) => {
                        "Resolved conflict in".to_string()
                    }
                    (_, MaterializedTreeValue::Conflict { .. }) => {
                        "Created conflict in".to_string()
                    }
                    (
                        MaterializedTreeValue::Symlink { .. },
                        MaterializedTreeValue::Symlink { .. },
                    ) => "Symlink target changed at".to_string(),
                    (_, _) => {
                        let left_type = basic_diff_file_type(&left_value);
                        let right_type = basic_diff_file_type(&right_value);
                        let (first, rest) = left_type.split_at(1);
                        format!(
                            "{}{} became {} at",
                            first.to_ascii_uppercase(),
                            rest,
                            right_type
                        )
                    }
                };
                let left_content = diff_content(&path, left_value)?;
                let right_content = diff_content(&path, right_value)?;
                writeln!(formatter.labeled("header"), "{description} {ui_path}:")?;
                show_color_words_diff_hunks(&left_content, &right_content, formatter)?;
                let description = basic_diff_file_type(&left_value);
                writeln!(
                    formatter.labeled("header"),
                    "Removed {description} {ui_path}:"
                )?;
                let left_content = diff_content(&path, left_value)?;
                if left_content.is_empty() {
                    writeln!(formatter.labeled("empty"), "    (empty)")?;
                } else {
                    show_color_words_diff_hunks(&left_content, &[], formatter)?;
                }
        Ok::<(), CommandError>(())
    .block_on()?;
    value: MaterializedTreeValue,
    let mut contents: Vec<u8>;
    match value {
        MaterializedTreeValue::Absent => {
            panic!("Absent path {path:?} in diff should have been handled by caller");
        }
        MaterializedTreeValue::File {
            id,
            executable,
            mut reader,
        } => {
            mode = if executable {
            contents = vec![];
            reader.read_to_end(&mut contents)?;
        MaterializedTreeValue::Symlink { id, target } => {
            contents = target.into_bytes();
        MaterializedTreeValue::GitSubmodule(id) => {
            contents = vec![];
        MaterializedTreeValue::Conflict {
            id: _,
            contents: conflict_data,
        } => {
            contents = conflict_data
        MaterializedTreeValue::Tree(_) => {
            panic!("Unexpected tree in diff at path {path:?}");
        content: contents,
    mut tree_diff: TreeDiffStream,
    let store = workspace_command.repo().store();
    async {
        while let Some((path, diff)) = tree_diff.next().await {
            let path_string = path.to_internal_file_string();
            let (left_value, right_value) = diff?;
            let left_value = materialize_tree_value(store, &path, left_value).block_on()?;
            let right_value = materialize_tree_value(store, &path, right_value).block_on()?;
            if left_value.is_absent() {
                let right_part = git_diff_part(&path, right_value)?;
                formatter.with_label("file_header", |formatter| {
                    writeln!(formatter, "diff --git a/{path_string} b/{path_string}")?;
                    writeln!(formatter, "new file mode {}", &right_part.mode)?;
                    writeln!(formatter, "index 0000000000..{}", &right_part.hash)?;
                    writeln!(formatter, "--- /dev/null")?;
                    writeln!(formatter, "+++ b/{path_string}")
                })?;
                show_unified_diff_hunks(formatter, &[], &right_part.content)?;
            } else if right_value.is_present() {
                let left_part = git_diff_part(&path, left_value)?;
                let right_part = git_diff_part(&path, right_value)?;
                formatter.with_label("file_header", |formatter| {
                    writeln!(formatter, "diff --git a/{path_string} b/{path_string}")?;
                    if left_part.mode != right_part.mode {
                        writeln!(formatter, "old mode {}", &left_part.mode)?;
                        writeln!(formatter, "new mode {}", &right_part.mode)?;
                        if left_part.hash != right_part.hash {
                            writeln!(formatter, "index {}...{}", &left_part.hash, right_part.hash)?;
                        }
                    } else if left_part.hash != right_part.hash {
                        writeln!(
                            formatter,
                            "index {}...{} {}",
                            &left_part.hash, right_part.hash, left_part.mode
                        )?;
                    if left_part.content != right_part.content {
                        writeln!(formatter, "--- a/{path_string}")?;
                        writeln!(formatter, "+++ b/{path_string}")?;
                    }
                    Ok(())
                })?;
                show_unified_diff_hunks(formatter, &left_part.content, &right_part.content)?;
            } else {
                let left_part = git_diff_part(&path, left_value)?;
                formatter.with_label("file_header", |formatter| {
                    writeln!(formatter, "diff --git a/{path_string} b/{path_string}")?;
                    writeln!(formatter, "deleted file mode {}", &left_part.mode)?;
                    writeln!(formatter, "index {}..0000000000", &left_part.hash)?;
                    writeln!(formatter, "+++ /dev/null")
                })?;
                show_unified_diff_hunks(formatter, &left_part.content, &[])?;
            }
        Ok::<(), CommandError>(())
    .block_on()?;
    mut tree_diff: TreeDiffStream,
    formatter.with_label("diff", |formatter| -> io::Result<()> {
        async {
            while let Some((repo_path, diff)) = tree_diff.next().await {
                let (before, after) = diff.unwrap();
                if before.is_present() && after.is_present() {
                    writeln!(
                        formatter.labeled("modified"),
                        "M {}",
                        workspace_command.format_file_path(&repo_path)
                    )?;
                } else if before.is_absent() {
                    writeln!(
                        formatter.labeled("added"),
                        "A {}",
                        workspace_command.format_file_path(&repo_path)
                    )?;
                } else {
                    writeln!(
                        formatter.labeled("removed"),
                        "R {}",
                        workspace_command.format_file_path(&repo_path)
                    )?;
                }
            Ok(())
        .block_on()
    mut tree_diff: TreeDiffStream,

    let store = workspace_command.repo().store();
    async {
        while let Some((repo_path, diff)) = tree_diff.next().await {
            let (left, right) = diff?;
            let left = materialize_tree_value(store, &repo_path, left).block_on()?;
            let right = materialize_tree_value(store, &repo_path, right).block_on()?;
            let path = workspace_command.format_file_path(&repo_path);
            let left_content = diff_content(&repo_path, left)?;
            let right_content = diff_content(&repo_path, right)?;
            max_path_width = max(max_path_width, path.width());
            let stat = get_diff_stat(path, &left_content, &right_content);
            max_diffs = max(max_diffs, stat.added + stat.removed);
            stats.push(stat);
        }
        Ok::<(), CommandError>(())
    .block_on()?;
    mut tree_diff: TreeDiffStream,
        async {
            while let Some((repo_path, diff)) = tree_diff.next().await {
                let (before, after) = diff.unwrap();
                writeln!(
                    formatter.labeled("modified"),
                    "{}{} {}",
                    diff_summary_char(&before),
                    diff_summary_char(&after),
                    workspace_command.format_file_path(&repo_path)
                )?;
            }
            Ok(())
        .block_on()