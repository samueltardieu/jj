// Copyright 2020 The Jujutsu Authors
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

use bstr::ByteVec as _;
use clap_complete::ArgValueCandidates;
use itertools::Itertools as _;
use jj_lib::object_id::ObjectId;
use jj_lib::rewrite::merge_commit_trees;
use tracing::instrument;

use crate::cli_util::CommandHelper;
use crate::cli_util::RevisionArg;
use crate::command_error::CommandError;
use crate::complete;
use crate::formatter::PlainTextFormatter;
use crate::ui::Ui;

/// Apply the reverse of given revisions on top of another revision
///
/// The description of the new revisions can be customized with the
/// `templates.backout_description` config variable.
#[derive(clap::Args, Clone, Debug)]
pub(crate) struct BackoutArgs {
    /// The revision(s) to apply the reverse of
    #[arg(
        long, short,
        default_value = "@",
        value_name = "REVSETS",
        add = ArgValueCandidates::new(complete::all_revisions),
    )]
    revisions: Vec<RevisionArg>,
    /// The revision to apply the reverse changes on top of
    // TODO: It seems better to default this to `@-`. Maybe the working
    // copy should be rebased on top?
    #[arg(
        long, short,
        default_value = "@",
        value_name = "REVSETS",
        add = ArgValueCandidates::new(complete::all_revisions),
    )]
    destination: Vec<RevisionArg>,
}

#[instrument(skip_all)]
pub(crate) fn cmd_backout(
    ui: &mut Ui,
    command: &CommandHelper,
    args: &BackoutArgs,
) -> Result<(), CommandError> {
    let mut workspace_command = command.workspace_helper(ui)?;
    let to_back_out: Vec<_> = workspace_command
        .parse_union_revsets(ui, &args.revisions)?
        .evaluate_to_commits()?
        .try_collect()?; // in reverse topological order
    if to_back_out.is_empty() {
        writeln!(ui.status(), "No revisions to back out.")?;
        return Ok(());
    }
    let mut parents = vec![];
    for revision_str in &args.destination {
        let destination = workspace_command.resolve_single_rev(ui, revision_str)?;
        parents.push(destination);
    }
    let transaction_description = if to_back_out.len() == 1 {
        format!("back out commit {}", to_back_out[0].id().hex())
    } else {
        format!(
            "back out commit {} and {} more",
            to_back_out[0].id().hex(),
            to_back_out.len() - 1
        )
    };
    let commits_to_back_out_with_new_commit_descriptions = {
        let template_text = command
            .settings()
            .get_string("templates.backout_description")?;
        let template = workspace_command.parse_commit_template(ui, &template_text)?;

        to_back_out
            .into_iter()
            .map(|commit| {
                let mut output = Vec::new();
                template
                    .format(&commit, &mut PlainTextFormatter::new(&mut output))
                    .expect("write() to vec backed formatter should never fail");
                // Template output is usually UTF-8, but it can contain file content.
                let commit_description = output.into_string_lossy();
                (commit, commit_description)
            })
            .collect_vec()
    };
    let mut tx = workspace_command.start_transaction();
    let mut new_base_tree = merge_commit_trees(tx.repo(), &parents)?;

    for (commit_to_back_out, new_commit_description) in
        commits_to_back_out_with_new_commit_descriptions
    {
        let old_base_tree = commit_to_back_out.parent_tree(tx.repo())?;
        let old_tree = commit_to_back_out.tree()?;
        let new_tree = new_base_tree.merge(&old_tree, &old_base_tree)?;
        let new_parent_ids = parents.iter().map(|commit| commit.id().clone()).collect();
        let new_commit = tx
            .repo_mut()
            .new_commit(new_parent_ids, new_tree.id())
            .set_description(new_commit_description)
            .write()?;
        parents = vec![new_commit];
        new_base_tree = new_tree;
    }
    tx.finish(ui, transaction_description)?;

    Ok(())
}
