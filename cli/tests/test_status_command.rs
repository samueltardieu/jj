// Copyright 2022 The Jujutsu Authors
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

use crate::common::TestEnvironment;

fn create_commit(
    test_env: &TestEnvironment,
    repo_path: &Path,
    name: &str,
    parents: &[&str],
    files: &[(&str, &str)],
) {
    if parents.is_empty() {
        test_env.jj_cmd_ok(repo_path, &["new", "root()", "-m", name]);
    } else {
        let mut args = vec!["new", "-m", name];
        args.extend(parents);
        test_env.jj_cmd_ok(repo_path, &args);
    }
    for (name, content) in files {
        std::fs::write(repo_path.join(name), content).unwrap();
    }
    test_env.jj_cmd_ok(repo_path, &["bookmark", "create", "-r@", name]);
}

#[test]
fn test_status_copies() {
    let test_env = TestEnvironment::default();
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);
    let repo_path = test_env.env_root().join("repo");

    std::fs::write(repo_path.join("copy-source"), "copy1\ncopy2\ncopy3\n").unwrap();
    std::fs::write(repo_path.join("rename-source"), "rename").unwrap();
    test_env.jj_cmd_ok(&repo_path, &["new"]);
    std::fs::write(
        repo_path.join("copy-source"),
        "copy1\ncopy2\ncopy3\nsource\n",
    )
    .unwrap();
    std::fs::write(
        repo_path.join("copy-target"),
        "copy1\ncopy2\ncopy3\ntarget\n",
    )
    .unwrap();
    std::fs::remove_file(repo_path.join("rename-source")).unwrap();
    std::fs::write(repo_path.join("rename-target"), "rename").unwrap();

    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);
    insta::assert_snapshot!(stdout, @r###"
    Working copy changes:
    M copy-source
    C {copy-source => copy-target}
    R {rename-source => rename-target}
    Working copy : rlvkpnrz a96c3086 (no description set)
    Parent commit: qpvuntsm e3e2c703 (no description set)
    "###);
}

#[test]
fn test_status_merge() {
    let test_env = TestEnvironment::default();
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);
    let repo_path = test_env.env_root().join("repo");

    std::fs::write(repo_path.join("file"), "base").unwrap();
    test_env.jj_cmd_ok(&repo_path, &["new", "-m=left"]);
    test_env.jj_cmd_ok(&repo_path, &["bookmark", "create", "-r@", "left"]);
    test_env.jj_cmd_ok(&repo_path, &["new", "@-", "-m=right"]);
    std::fs::write(repo_path.join("file"), "right").unwrap();
    test_env.jj_cmd_ok(&repo_path, &["new", "left", "@"]);

    // The output should mention each parent, and the diff should be empty (compared
    // to the auto-merged parents)
    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);
    insta::assert_snapshot!(stdout, @r###"
    The working copy has no changes.
    Working copy : mzvwutvl a538c72d (empty) (no description set)
    Parent commit: rlvkpnrz d3dd19f1 left | (empty) left
    Parent commit: zsuskuln 705a356d right
    "###);
}

// See https://github.com/jj-vcs/jj/issues/2051.
#[test]
fn test_status_ignored_gitignore() {
    let test_env = TestEnvironment::default();
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);
    let repo_path = test_env.env_root().join("repo");

    std::fs::create_dir(repo_path.join("untracked")).unwrap();
    std::fs::write(repo_path.join("untracked").join("inside_untracked"), "test").unwrap();
    std::fs::write(
        repo_path.join("untracked").join(".gitignore"),
        "!inside_untracked\n",
    )
    .unwrap();
    std::fs::write(repo_path.join(".gitignore"), "untracked/\n!dummy\n").unwrap();

    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);
    insta::assert_snapshot!(stdout, @r###"
    Working copy changes:
    A .gitignore
    Working copy : qpvuntsm 3cef2183 (no description set)
    Parent commit: zzzzzzzz 00000000 (empty) (no description set)
    "###);
}

#[test]
fn test_status_filtered() {
    let test_env = TestEnvironment::default();
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);
    let repo_path = test_env.env_root().join("repo");

    std::fs::write(repo_path.join("file_1"), "file_1").unwrap();
    std::fs::write(repo_path.join("file_2"), "file_2").unwrap();

    // The output filtered to file_1 should not list the addition of file_2.
    let stdout = test_env.jj_cmd_success(&repo_path, &["status", "file_1"]);
    insta::assert_snapshot!(stdout, @r###"
    Working copy changes:
    A file_1
    Working copy : qpvuntsm c8fb8395 (no description set)
    Parent commit: zzzzzzzz 00000000 (empty) (no description set)
    "###);
}

// See <https://github.com/jj-vcs/jj/issues/3108>
// See <https://github.com/jj-vcs/jj/issues/4147>
#[test]
fn test_status_display_relevant_working_commit_conflict_hints() {
    let test_env = TestEnvironment::default();
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);

    let repo_path = test_env.env_root().join("repo");
    let conflicted_path = repo_path.join("conflicted.txt");

    // PARENT: Write the initial file
    std::fs::write(&conflicted_path, "initial contents").unwrap();
    test_env.jj_cmd_ok(&repo_path, &["describe", "--message", "Initial contents"]);

    // CHILD1: New commit on top of <PARENT>
    test_env.jj_cmd_ok(
        &repo_path,
        &["new", "--message", "First part of conflicting change"],
    );
    std::fs::write(&conflicted_path, "Child 1").unwrap();

    // CHILD2: New commit also on top of <PARENT>
    test_env.jj_cmd_ok(
        &repo_path,
        &[
            "new",
            "--message",
            "Second part of conflicting change",
            "@-",
        ],
    );
    std::fs::write(&conflicted_path, "Child 2").unwrap();

    // CONFLICT: New commit that is conflicted by merging <CHILD1> and <CHILD2>
    test_env.jj_cmd_ok(&repo_path, &["new", "--message", "boom", "all:(@-)+"]);
    // Adding more descendants to ensure we correctly find the root ancestors with
    // conflicts, not just the parents.
    test_env.jj_cmd_ok(&repo_path, &["new", "--message", "boom-cont"]);
    test_env.jj_cmd_ok(&repo_path, &["new", "--message", "boom-cont-2"]);

    let stdout = test_env.jj_cmd_success(&repo_path, &["log", "-r", "::"]);

    insta::assert_snapshot!(stdout, @r###"
    @  yqosqzyt test.user@example.com 2001-02-03 08:05:13 dcb25635 conflict
    │  (empty) boom-cont-2
    ×  royxmykx test.user@example.com 2001-02-03 08:05:12 664a4c6c conflict
    │  (empty) boom-cont
    ×    mzvwutvl test.user@example.com 2001-02-03 08:05:11 c5a4e9cb conflict
    ├─╮  (empty) boom
    │ ○  kkmpptxz test.user@example.com 2001-02-03 08:05:10 1e8c2956
    │ │  First part of conflicting change
    ○ │  zsuskuln test.user@example.com 2001-02-03 08:05:11 2c8b19fd
    ├─╯  Second part of conflicting change
    ○  qpvuntsm test.user@example.com 2001-02-03 08:05:08 aade7195
    │  Initial contents
    ◆  zzzzzzzz root() 00000000
    "###);

    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);

    insta::assert_snapshot!(stdout, @r###"
    The working copy has no changes.
    There are unresolved conflicts at these paths:
    conflicted.txt    2-sided conflict
    Working copy : yqosqzyt dcb25635 (conflict) (empty) boom-cont-2
    Parent commit: royxmykx 664a4c6c (conflict) (empty) boom-cont
    To resolve the conflicts, start by updating to the first one:
      jj new mzvwutvl
    Then use `jj resolve`, or edit the conflict markers in the file directly.
    Once the conflicts are resolved, you may want to inspect the result with `jj diff`.
    Then run `jj squash` to move the resolution into the conflicted commit.
    "###);

    // Resolve conflict
    test_env.jj_cmd_ok(&repo_path, &["new", "--message", "fixed 1"]);
    std::fs::write(&conflicted_path, "first commit to fix conflict").unwrap();

    // Add one more commit atop the commit that resolves the conflict.
    test_env.jj_cmd_ok(&repo_path, &["new", "--message", "fixed 2"]);
    std::fs::write(&conflicted_path, "edit not conflict").unwrap();

    // wc is now conflict free, parent is also conflict free
    let stdout = test_env.jj_cmd_success(&repo_path, &["log", "-r", "::"]);

    insta::assert_snapshot!(stdout, @r###"
    @  kpqxywon test.user@example.com 2001-02-03 08:05:18 d313f2e1
    │  fixed 2
    ○  znkkpsqq test.user@example.com 2001-02-03 08:05:17 23e58975
    │  fixed 1
    ×  yqosqzyt test.user@example.com 2001-02-03 08:05:13 dcb25635 conflict
    │  (empty) boom-cont-2
    ×  royxmykx test.user@example.com 2001-02-03 08:05:12 664a4c6c conflict
    │  (empty) boom-cont
    ×    mzvwutvl test.user@example.com 2001-02-03 08:05:11 c5a4e9cb conflict
    ├─╮  (empty) boom
    │ ○  kkmpptxz test.user@example.com 2001-02-03 08:05:10 1e8c2956
    │ │  First part of conflicting change
    ○ │  zsuskuln test.user@example.com 2001-02-03 08:05:11 2c8b19fd
    ├─╯  Second part of conflicting change
    ○  qpvuntsm test.user@example.com 2001-02-03 08:05:08 aade7195
    │  Initial contents
    ◆  zzzzzzzz root() 00000000
    "###);

    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);

    insta::assert_snapshot!(stdout, @r###"
    Working copy changes:
    M conflicted.txt
    Working copy : kpqxywon d313f2e1 fixed 2
    Parent commit: znkkpsqq 23e58975 fixed 1
    "###);

    // Step back one.
    // wc is still conflict free, parent has a conflict.
    test_env.jj_cmd_ok(&repo_path, &["edit", "@-"]);
    let stdout = test_env.jj_cmd_success(&repo_path, &["log", "-r", "::"]);

    insta::assert_snapshot!(stdout, @r###"
    ○  kpqxywon test.user@example.com 2001-02-03 08:05:18 d313f2e1
    │  fixed 2
    @  znkkpsqq test.user@example.com 2001-02-03 08:05:17 23e58975
    │  fixed 1
    ×  yqosqzyt test.user@example.com 2001-02-03 08:05:13 dcb25635 conflict
    │  (empty) boom-cont-2
    ×  royxmykx test.user@example.com 2001-02-03 08:05:12 664a4c6c conflict
    │  (empty) boom-cont
    ×    mzvwutvl test.user@example.com 2001-02-03 08:05:11 c5a4e9cb conflict
    ├─╮  (empty) boom
    │ ○  kkmpptxz test.user@example.com 2001-02-03 08:05:10 1e8c2956
    │ │  First part of conflicting change
    ○ │  zsuskuln test.user@example.com 2001-02-03 08:05:11 2c8b19fd
    ├─╯  Second part of conflicting change
    ○  qpvuntsm test.user@example.com 2001-02-03 08:05:08 aade7195
    │  Initial contents
    ◆  zzzzzzzz root() 00000000
    "###);

    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);

    insta::assert_snapshot!(stdout, @r###"
    Working copy changes:
    M conflicted.txt
    Working copy : znkkpsqq 23e58975 fixed 1
    Parent commit: yqosqzyt dcb25635 (conflict) (empty) boom-cont-2
    Conflict in parent commit has been resolved in working copy
    "###);

    // Step back to all the way to `root()+` so that wc has no conflict, even though
    // there is a conflict later in the tree. So that we can confirm
    // our hinting logic doesn't get confused.
    test_env.jj_cmd_ok(&repo_path, &["edit", "root()+"]);
    let stdout = test_env.jj_cmd_success(&repo_path, &["log", "-r", "::"]);

    insta::assert_snapshot!(stdout, @r###"
    ○  kpqxywon test.user@example.com 2001-02-03 08:05:18 d313f2e1
    │  fixed 2
    ○  znkkpsqq test.user@example.com 2001-02-03 08:05:17 23e58975
    │  fixed 1
    ×  yqosqzyt test.user@example.com 2001-02-03 08:05:13 dcb25635 conflict
    │  (empty) boom-cont-2
    ×  royxmykx test.user@example.com 2001-02-03 08:05:12 664a4c6c conflict
    │  (empty) boom-cont
    ×    mzvwutvl test.user@example.com 2001-02-03 08:05:11 c5a4e9cb conflict
    ├─╮  (empty) boom
    │ ○  kkmpptxz test.user@example.com 2001-02-03 08:05:10 1e8c2956
    │ │  First part of conflicting change
    ○ │  zsuskuln test.user@example.com 2001-02-03 08:05:11 2c8b19fd
    ├─╯  Second part of conflicting change
    @  qpvuntsm test.user@example.com 2001-02-03 08:05:08 aade7195
    │  Initial contents
    ◆  zzzzzzzz root() 00000000
    "###);

    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);

    insta::assert_snapshot!(stdout, @r###"
    Working copy changes:
    A conflicted.txt
    Working copy : qpvuntsm aade7195 Initial contents
    Parent commit: zzzzzzzz 00000000 (empty) (no description set)
    "###);
}

#[test]
fn test_status_simplify_conflict_sides() {
    let test_env = TestEnvironment::default();
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);
    let repo_path = test_env.env_root().join("repo");

    // Creates a 4-sided conflict, with fileA and fileB having different conflicts:
    // fileA: A - B + C - B + B - B + B
    // fileB: A - A + A - A + B - C + D
    create_commit(
        &test_env,
        &repo_path,
        "base",
        &[],
        &[("fileA", "base\n"), ("fileB", "base\n")],
    );
    create_commit(&test_env, &repo_path, "a1", &["base"], &[("fileA", "1\n")]);
    create_commit(&test_env, &repo_path, "a2", &["base"], &[("fileA", "2\n")]);
    create_commit(&test_env, &repo_path, "b1", &["base"], &[("fileB", "1\n")]);
    create_commit(&test_env, &repo_path, "b2", &["base"], &[("fileB", "2\n")]);
    create_commit(&test_env, &repo_path, "conflictA", &["a1", "a2"], &[]);
    create_commit(&test_env, &repo_path, "conflictB", &["b1", "b2"], &[]);
    create_commit(
        &test_env,
        &repo_path,
        "conflict",
        &["conflictA", "conflictB"],
        &[],
    );

    insta::assert_snapshot!(test_env.jj_cmd_success(&repo_path, &["status"]),
    @r###"
    The working copy has no changes.
    There are unresolved conflicts at these paths:
    fileA    2-sided conflict
    fileB    2-sided conflict
    Working copy : nkmrtpmo 83c4b9e7 conflict | (conflict) (empty) conflict
    Parent commit: kmkuslsw 4601566f conflictA | (conflict) (empty) conflictA
    Parent commit: lylxulpl 6f8d8381 conflictB | (conflict) (empty) conflictB
    To resolve the conflicts, start by updating to one of the first ones:
      jj new lylxulpl
      jj new kmkuslsw
    Then use `jj resolve`, or edit the conflict markers in the file directly.
    Once the conflicts are resolved, you may want to inspect the result with `jj diff`.
    Then run `jj squash` to move the resolution into the conflicted commit.
    "###);
}

#[test]
fn test_status_untracked_files() {
    let test_env = TestEnvironment::default();
    test_env.add_config(r#"snapshot.auto-track = "none()""#);

    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);
    let repo_path = test_env.env_root().join("repo");

    std::fs::write(repo_path.join("always-untracked-file"), "...").unwrap();
    std::fs::write(repo_path.join("initially-untracked-file"), "...").unwrap();
    std::fs::create_dir(repo_path.join("sub")).unwrap();
    std::fs::write(repo_path.join("sub").join("always-untracked"), "...").unwrap();
    std::fs::write(repo_path.join("sub").join("initially-untracked"), "...").unwrap();

    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);
    insta::assert_snapshot!(stdout.replace('\\', "/"), @r"
    Untracked paths:
    ? always-untracked-file
    ? initially-untracked-file
    ? sub/always-untracked
    ? sub/initially-untracked
    Working copy : qpvuntsm 230dd059 (empty) (no description set)
    Parent commit: zzzzzzzz 00000000 (empty) (no description set)
    ");

    test_env.jj_cmd_success(
        &repo_path,
        &[
            "file",
            "track",
            "initially-untracked-file",
            "sub/initially-untracked",
        ],
    );

    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);
    insta::assert_snapshot!(stdout.replace('\\', "/"), @r"
    Working copy changes:
    A initially-untracked-file
    A sub/initially-untracked
    Untracked paths:
    ? always-untracked-file
    ? sub/always-untracked
    Working copy : qpvuntsm 99798fcd (no description set)
    Parent commit: zzzzzzzz 00000000 (empty) (no description set)
    ");

    test_env.jj_cmd_ok(&repo_path, &["new"]);

    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);
    insta::assert_snapshot!(stdout.replace('\\', "/"), @r"
    Untracked paths:
    ? always-untracked-file
    ? sub/always-untracked
    Working copy : mzvwutvl 30e53c74 (empty) (no description set)
    Parent commit: qpvuntsm 99798fcd (no description set)
    ");

    test_env.jj_cmd_success(
        &repo_path,
        &[
            "file",
            "untrack",
            "initially-untracked-file",
            "sub/initially-untracked",
        ],
    );
    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);
    insta::assert_snapshot!(stdout.replace('\\', "/"), @r"
    Working copy changes:
    D initially-untracked-file
    D sub/initially-untracked
    Untracked paths:
    ? always-untracked-file
    ? initially-untracked-file
    ? sub/always-untracked
    ? sub/initially-untracked
    Working copy : mzvwutvl bb362aaf (no description set)
    Parent commit: qpvuntsm 99798fcd (no description set)
    ");

    test_env.jj_cmd_ok(&repo_path, &["new"]);

    let stdout = test_env.jj_cmd_success(&repo_path, &["status"]);
    insta::assert_snapshot!(stdout.replace('\\', "/"), @r"
    Untracked paths:
    ? always-untracked-file
    ? initially-untracked-file
    ? sub/always-untracked
    ? sub/initially-untracked
    Working copy : yostqsxw 8e8c02fe (empty) (no description set)
    Parent commit: mzvwutvl bb362aaf (no description set)
    ");
}
