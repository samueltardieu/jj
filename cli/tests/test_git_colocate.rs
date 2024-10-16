// Copyright 2024 The Jujutsu Authors
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

use crate::common::{get_stderr_string, get_stdout_string, TestEnvironment};

#[test]
fn test_git_colocate_empty() {
    let test_env = TestEnvironment::default();
    let (stdout, stderr) = test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);
    insta::assert_snapshot!(stdout, @"");
    insta::assert_snapshot!(stderr, @r###"
    Initialized repo in "repo"
    "###);

    let workspace_root = test_env.env_root().join("repo");
    let (stdout, stderr) = test_env.jj_cmd_ok(&workspace_root, &["git", "colocate"]);
    insta::assert_snapshot!(stdout, @"");
    insta::assert_snapshot!(stderr, @r#"
    Nothing changed.
    "#);

    let assert = test_env.jj_cmd(&workspace_root, &["git", "colocate"]).assert().code(1);
    insta::assert_snapshot!(test_env.normalize_output(&get_stdout_string(&assert)), @"");
    insta::assert_snapshot!(test_env.normalize_output(&get_stderr_string(&assert)), @r#"
    Error: The repo is already colocated
    "#);
}

#[test]
fn test_git_uncolocate_empty() {
    let test_env = TestEnvironment::default();
    let (stdout, stderr) = test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "--colocate", "repo"]);
    insta::assert_snapshot!(stdout, @"");
    insta::assert_snapshot!(stderr, @r###"
    Initialized repo in "repo"
    "###);

    let workspace_root = test_env.env_root().join("repo");
    let (stdout, stderr) = test_env.jj_cmd_ok(&workspace_root, &["git", "colocate", "--undo"]);
    insta::assert_snapshot!(stdout, @"");
    insta::assert_snapshot!(stderr, @"");

    let assert = test_env.jj_cmd(&workspace_root, &["git", "colocate", "--undo"]).assert().code(1);
    insta::assert_snapshot!(test_env.normalize_output(&get_stdout_string(&assert)), @"");
    insta::assert_snapshot!(test_env.normalize_output(&get_stderr_string(&assert)), @r#"
    Error: The repo is not colocated
    "#);
}
