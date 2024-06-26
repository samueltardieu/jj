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

use insta::assert_snapshot;

use crate::common::TestEnvironment;

pub mod common;

#[test]
fn test_debug_config_schema() {
    let test_env = TestEnvironment::default();
    let stdout = test_env.jj_cmd_success(test_env.env_root(), &["debug", "config-schema"]);
    // Validate partial snapshot, redacting any lines nested 2+ indent levels.
    insta::with_settings!({filters => vec![(r"(?m)(^        .*$\r?\n)+", "        [...]\n")]}, {
        assert_snapshot!(stdout, @r###"
        {
            "$schema": "http://json-schema.org/draft-07/schema",
            "title": "Jujutsu config",
            "type": "object",
            "description": "User configuration for Jujutsu VCS. See https://github.com/martinvonz/jj/blob/main/docs/config.md for details",
            "properties": {
                [...]
            }
        }
        "###)
    });
}

#[test]
fn test_debug_index() {
    let test_env = TestEnvironment::default();
    test_env.jj_cmd_success(test_env.env_root(), &["init", "repo", "--git"]);
    let workspace_path = test_env.env_root().join("repo");
    let stdout = test_env.jj_cmd_success(&workspace_path, &["debug", "index"]);
    insta::with_settings!(
    {filters => vec![
        (r"    Name: [0-9a-z]+", "    Name: [hash]"),
    ]},
    {
        assert_snapshot!(stdout, @r###"
        Number of commits: 2
        Number of merges: 0
        Max generation number: 1
        Number of heads: 1
        Number of changes: 2
        Stats per level:
          Level 0:
            Number of commits: 2
            Name: [hash]
        "###)
    });
}
