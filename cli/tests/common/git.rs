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

use std::path::Path;
use std::path::PathBuf;

pub const GIT_USER: &str = "Someone";
pub const GIT_EMAIL: &str = "someone@example.org";

fn git_config() -> Vec<bstr::BString> {
    vec![
        format!("user.name = {GIT_USER}").into(),
        format!("user.email = {GIT_EMAIL}").into(),
        "init.defaultBranch = master".into(),
    ]
}

fn open_options() -> gix::open::Options {
    gix::open::Options::isolated().config_overrides(git_config())
}

pub fn open(directory: impl Into<PathBuf>) -> gix::Repository {
    gix::open_opts(directory, open_options()).unwrap()
}

pub fn init(directory: impl AsRef<Path>) -> gix::Repository {
    gix::ThreadSafeRepository::init_opts(
        directory,
        gix::create::Kind::WithWorktree,
        gix::create::Options::default(),
        open_options(),
    )
    .unwrap()
    .to_thread_local()
}

pub fn init_bare(directory: impl AsRef<Path>) -> gix::Repository {
    gix::ThreadSafeRepository::init_opts(
        directory,
        gix::create::Kind::Bare,
        gix::create::Options::default(),
        open_options(),
    )
    .unwrap()
    .to_thread_local()
}

pub fn clone(dest_path: &Path, url: &str) -> gix::Repository {
    let mut prepare_fetch = gix::clone::PrepareFetch::new(
        url,
        dest_path,
        gix::create::Kind::WithWorktree,
        gix::create::Options::default(),
        open_options(),
    )
    .unwrap();
    let (mut prepare_checkout, _outcome) = prepare_fetch
        .fetch_then_checkout(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .unwrap();
    let (repo, _outcome) = prepare_checkout
        .main_worktree(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .unwrap();

    repo
}

/// Writes out gitlink entry pointing to the `target_repo`.
pub fn create_gitlink(src_repo: impl AsRef<Path>, target_repo: impl AsRef<Path>) {
    let git_link_path = src_repo.as_ref().join(".git");
    std::fs::write(
        git_link_path,
        format!("gitdir: {}\n", target_repo.as_ref().display()),
    )
    .unwrap();
}

pub fn remove_config_value(mut repo: gix::Repository, section: &str, key: &str) {
    let mut config = repo.config_snapshot_mut();
    let Ok(mut section) = config.section_mut(section, None) else {
        return;
    };
    section.remove(key);

    let mut file = std::fs::File::create(config.meta().path.as_ref().unwrap()).unwrap();
    config
        .write_to_filter(&mut file, |section| section.meta() == config.meta())
        .unwrap();
}

pub struct CommitResult {
    pub tree_id: gix::ObjectId,
    pub commit_id: gix::ObjectId,
}

pub fn add_commit(
    repo: &gix::Repository,
    reference: &str,
    filename: &str,
    content: &[u8],
    message: &str,
    parents: &[gix::ObjectId],
) -> CommitResult {
    let blob_oid = repo.write_blob(content).unwrap();

    let parent_tree_editor = parents.first().map(|commit_id| {
        repo.find_commit(*commit_id)
            .unwrap()
            .tree()
            .unwrap()
            .edit()
            .unwrap()
    });
    let empty_tree_editor_fn = || {
        repo.edit_tree(gix::ObjectId::empty_tree(repo.object_hash()))
            .unwrap()
    };

    let mut tree_editor = parent_tree_editor.unwrap_or_else(empty_tree_editor_fn);
    tree_editor
        .upsert(filename, gix::object::tree::EntryKind::Blob, blob_oid)
        .unwrap();
    let tree_id = tree_editor.write().unwrap().detach();
    let commit_id = write_commit(repo, reference, tree_id, message, parents);
    CommitResult { tree_id, commit_id }
}

pub fn write_commit(
    repo: &gix::Repository,
    reference: &str,
    tree_id: gix::ObjectId,
    message: &str,
    parents: &[gix::ObjectId],
) -> gix::ObjectId {
    let signature = signature();
    repo.commit_as(
        &signature,
        &signature,
        reference,
        message,
        tree_id,
        parents.iter().copied(),
    )
    .unwrap()
    .detach()
}

pub fn set_head_to_id(repo: &gix::Repository, target: gix::ObjectId) {
    repo.edit_reference(gix::refs::transaction::RefEdit {
        change: gix::refs::transaction::Change::Update {
            log: gix::refs::transaction::LogChange::default(),
            expected: gix::refs::transaction::PreviousValue::Any,
            new: gix::refs::Target::Object(target),
        },
        name: "HEAD".try_into().unwrap(),
        deref: false,
    })
    .unwrap();
}

pub fn set_symbolic_reference(repo: &gix::Repository, reference: &str, target: &str) {
    use gix::refs::transaction;
    let change = transaction::Change::Update {
        log: transaction::LogChange {
            mode: transaction::RefLog::AndReference,
            force_create_reflog: true,
            message: "create symbolic reference".into(),
        },
        expected: transaction::PreviousValue::Any,
        new: gix::refs::Target::Symbolic(target.try_into().unwrap()),
    };

    let ref_edit = transaction::RefEdit {
        change,
        name: reference.try_into().unwrap(),
        deref: false,
    };
    repo.edit_reference(ref_edit).unwrap();
}

fn signature() -> gix::actor::Signature {
    gix::actor::Signature {
        name: bstr::BString::from(GIT_USER),
        email: bstr::BString::from(GIT_EMAIL),
        time: gix::date::Time::new(0, 0),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum GitStatusInfo {
    Index(IndexStatus),
    Worktree(WorktreeStatus),
}

#[derive(Debug, PartialEq, Eq)]
pub enum IndexStatus {
    Addition,
    Deletion,
    Rename,
    Modification,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WorktreeStatus {
    Removed,
    Added,
    Modified,
    TypeChange,
    Renamed,
    Copied,
    IntentToAdd,
    Conflict,
    Ignored,
}

impl<'lhs, 'rhs> From<gix::diff::index::ChangeRef<'lhs, 'rhs>> for IndexStatus {
    fn from(value: gix::diff::index::ChangeRef<'lhs, 'rhs>) -> Self {
        match value {
            gix::diff::index::ChangeRef::Addition { .. } => IndexStatus::Addition,
            gix::diff::index::ChangeRef::Deletion { .. } => IndexStatus::Deletion,
            gix::diff::index::ChangeRef::Rewrite { .. } => IndexStatus::Rename,
            gix::diff::index::ChangeRef::Modification { .. } => IndexStatus::Modification,
        }
    }
}

impl From<Option<gix::status::index_worktree::iter::Summary>> for WorktreeStatus {
    fn from(value: Option<gix::status::index_worktree::iter::Summary>) -> Self {
        match value {
            Some(gix::status::index_worktree::iter::Summary::Removed) => WorktreeStatus::Removed,
            Some(gix::status::index_worktree::iter::Summary::Added) => WorktreeStatus::Added,
            Some(gix::status::index_worktree::iter::Summary::Modified) => WorktreeStatus::Modified,
            Some(gix::status::index_worktree::iter::Summary::TypeChange) => {
                WorktreeStatus::TypeChange
            }
            Some(gix::status::index_worktree::iter::Summary::Renamed) => WorktreeStatus::Renamed,
            Some(gix::status::index_worktree::iter::Summary::Copied) => WorktreeStatus::Copied,
            Some(gix::status::index_worktree::iter::Summary::IntentToAdd) => {
                WorktreeStatus::IntentToAdd
            }
            Some(gix::status::index_worktree::iter::Summary::Conflict) => WorktreeStatus::Conflict,
            None => WorktreeStatus::Ignored,
        }
    }
}

impl From<gix::status::Item> for GitStatusInfo {
    fn from(value: gix::status::Item) -> Self {
        match value {
            gix::status::Item::TreeIndex(change) => GitStatusInfo::Index(change.into()),
            gix::status::Item::IndexWorktree(item) => {
                GitStatusInfo::Worktree(item.summary().into())
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct GitStatus {
    path: String,
    status: GitStatusInfo,
}

impl From<gix::status::Item> for GitStatus {
    fn from(value: gix::status::Item) -> Self {
        let path = value.location().to_string();
        let status = value.into();
        GitStatus { path, status }
    }
}

pub fn status(repo: &gix::Repository) -> Vec<GitStatus> {
    let mut status: Vec<GitStatus> = repo
        .status(gix::progress::Discard)
        .unwrap()
        .untracked_files(gix::status::UntrackedFiles::Files)
        .dirwalk_options(|options| {
            options.emit_ignored(Some(gix::dir::walk::EmissionMode::Matching))
        })
        .into_iter(None)
        .unwrap()
        .map(Result::unwrap)
        .map(|x| x.into())
        .collect();

    status.sort_by(|a, b| a.path.cmp(&b.path));
    status
}

pub struct IndexManager<'a> {
    index: gix::index::File,
    repo: &'a gix::Repository,
    added_entries: Vec<(gix::ObjectId, &'a str)>,
}

impl<'a> IndexManager<'a> {
    pub fn new(repo: &'a gix::Repository) -> IndexManager<'a> {
        let index = gix::index::File::from_state(
            gix::index::State::new(repo.object_hash()),
            repo.index_path(),
        );
        IndexManager {
            index,
            repo,
            added_entries: Vec::new(),
        }
    }

    pub fn add_file(&mut self, name: &'a str, data: &[u8]) {
        std::fs::write(self.repo.work_dir().unwrap().join(name), data).unwrap();
        let blob_oid = self.repo.write_blob(data).unwrap().detach();

        self.added_entries.push((blob_oid, name));
        self.index.dangerously_push_entry(
            gix::index::entry::Stat::default(),
            blob_oid,
            gix::index::entry::Flags::from_stage(gix::index::entry::Stage::Unconflicted),
            gix::index::entry::Mode::FILE,
            name.as_bytes().into(),
        );
    }

    pub fn sync_index(&mut self) {
        self.index.sort_entries();
        self.index.verify_entries().unwrap();
        self.index
            .write(gix::index::write::Options::default())
            .unwrap();
    }
}
