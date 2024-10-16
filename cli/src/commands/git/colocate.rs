// Copyright 2025 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

use crate::cli_util::CommandHelper;
use crate::cli_util::WorkspaceCommandHelper;
use crate::command_error::user_error;
use crate::command_error::CommandError;
use crate::ui::Ui;

use super::export;
use super::export::GitExportArgs;

/// Transform a non-colocated git repository into a colocated one,
/// and vice-versa.
#[derive(clap::Args, Clone, Debug)]
pub struct GitColocateArgs {
    /// Transform a colocated repository into a non-colocated one.
    #[arg(long)]
    undo: bool,
}

pub fn cmd_git_colocate(
    ui: &mut Ui,
    command: &CommandHelper,
    args: &GitColocateArgs,
) -> Result<(), CommandError> {
    let workspace_command = command.workspace_helper(ui)?;
    if workspace_command.git_backend().is_none() {
        return Err(user_error("The repo is not backed by a git repo"));
    }
    if args.undo {
        undo_git_colocate(&workspace_command)
    } else {
        do_git_colocate(&workspace_command)?;
        export::cmd_git_export(ui, command, &GitExportArgs {})
    }
}

/// Returns a tuple with:
///   - the colocated git repo path
///   - the uncolocated repo path
///   - the .jj/repo/store/git_target path
///   - the .jj/.gitignore path
fn git_repo_paths(
    workspace_command: &WorkspaceCommandHelper,
) -> Result<(PathBuf, PathBuf, PathBuf, PathBuf), CommandError> {
    let workspace_root = workspace_command.workspace_root();
    let store_path = workspace_command
        .repo_path()
        .to_owned()
        .clone()
        .join("store");
    Ok((
        workspace_root.join(".git"),
        store_path.clone().join("git"),
        store_path.join("git_target"),
        workspace_root.join(".jj").join(".gitignore"),
    ))
}

fn do_git_colocate(workspace_command: &WorkspaceCommandHelper) -> Result<(), CommandError> {
    let (colocated_path, uncolocated_path, git_target_path, gitignore_path) =
        git_repo_paths(workspace_command)?;
    if fs::exists(&colocated_path)? {
        return Err(user_error("The repo is already colocated"));
    }
    fs::rename(uncolocated_path, &colocated_path)?;
    git2::Repository::open_bare(&colocated_path)?
        .config()?
        .remove("core.bare")?;
    fs::write(git_target_path, "../../../.git")?;
    if !fs::exists(&gitignore_path)? {
        fs::write(gitignore_path, "/*\n")?;
    }
    Ok(())
}

fn undo_git_colocate(workspace_command: &WorkspaceCommandHelper) -> Result<(), CommandError> {
    let (colocated_path, uncolocated_path, git_target_path, _) = git_repo_paths(workspace_command)?;
    if !fs::exists(&colocated_path)? {
        return Err(user_error("The repo is not colocated"));
    }
    fs::rename(colocated_path, &uncolocated_path)?;
    git2::Repository::open_ext::<_, &OsStr, _>(uncolocated_path, git2::RepositoryOpenFlags::NO_DOTGIT, vec![])?
        .config()?
        .set_bool("core.bare", true)?;
    fs::write(git_target_path, "git")?;
    Ok(())
}
