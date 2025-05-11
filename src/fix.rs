use crate::repo::CommitResolver;
use crate::repo::GitRepo;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use argh::FromArgs;
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;

#[derive(FromArgs, PartialEq, Debug)]
/// Fixup the file given
#[argh(subcommand, name = "fix")]
pub struct Args {
    /// git repo for commits
    #[argh(option)]
    code: PathBuf,
    /// markdown files to be fixed
    #[argh(positional)]
    files: Vec<String>,
}
impl Args {
    pub fn run(&self) -> Result<()> {
        let repo = GitRepo::new(self.code.clone());
        for file in &self.files {
            eprintln!("fix: {file}");
            fix_file(&repo, file)?;
        }
        Ok(())
    }
}

fn fix_file<T: CommitResolver>(repo: &T, path: &str) -> Result<()> {
    let s = std::fs::read_to_string(path).expect("Failed to open a file");
    let lines: Vec<String> = s.split('\n').map(|s| s.to_string()).collect();
    let lines = replace_commit_id_with_change_id(repo, lines)?;
    let lines = remove_generated_lines(lines)?;
    let lines = insert_commit_diff_with_change_id(repo, lines)?;
    let s = lines.join("\n");
    std::fs::File::create(path)?
        .write_all(s.as_bytes())
        .context("Failed to write a file")?;
    Ok(())
}

fn replace_commit_id_with_change_id<T: CommitResolver>(
    commit_resolver: &T,
    lines: Vec<String>,
) -> Result<Vec<String>> {
    let mut lines_updated: Vec<String> = Vec::new();
    for (ln, line) in lines.into_iter().enumerate() {
        if line.contains("ajimi::code") && line.starts_with("<!--") {
            if let Some(commit) = line.split(' ').skip_while(|s| *s != "commit").nth(1) {
                if let Ok(change_id) = commit_resolver.change_id_from_commit_id(commit) {
                    let line_updated = format!("<!-- ajimi::code change_id {change_id} -->");
                    lines_updated.push(line_updated);
                    continue;
                } else {
                    eprintln!("Invalid commit at line {ln}: {}", line);
                    lines_updated.push(line);
                    continue;
                }
            }
        }
        lines_updated.push(line.clone());
    }
    Ok(lines_updated)
}

fn remove_generated_lines(lines: Vec<String>) -> Result<Vec<String>> {
    let mut lines_updated: Vec<String> = Vec::new();
    let mut lines_pending: Vec<String> = Vec::new();
    let mut end_marker_for_pending: Option<String> = None;
    for line in lines {
        if line.starts_with("<!--") && line.contains("ajimi::code change_id") {
            if end_marker_for_pending.is_some() {
                // ajimi::code appeared again without ajimi::end.
                // push all pending lines.
                lines_updated.append(&mut lines_pending);
            }
            lines_updated.push(line.clone()); // first line is kept always.
            end_marker_for_pending = Some(line.replace("ajimi::code", "ajimi::end").to_string());
            continue;
        } else if Some(line.clone()) == end_marker_for_pending {
            // end marker found. drop all pending lines.
            lines_pending.clear();
            end_marker_for_pending = None;
            continue;
        } else if end_marker_for_pending.is_some() {
            lines_pending.push(line.clone());
        } else {
            lines_updated.push(line.clone());
        }
    }
    // tail case for non-terminated block
    lines_updated.append(&mut lines_pending);
    Ok(lines_updated)
}

fn format_patch<T: CommitResolver>(
    input: &str,
    commit_resolver: &T,
    commit_id: Option<&str>,
) -> Result<String> {
    let mut output = String::new();
    let parts = input.split("\n").collect::<Vec<&str>>();
    let parts: Vec<String> = parts
        .chunk_by(|_, b| !b.starts_with("diff --git"))
        .map(|a| a.join("\n"))
        .collect();
    for part in parts {
        if part.trim().is_empty() {
            continue;
        }
        if !part.starts_with("diff --git") {
            return Err(anyhow!("format_patch: Invalid part found: {:?}", part));
        }
        let lines: Vec<&str> = part.split_inclusive("\n").collect();
        let filename = lines[0]
            .split(" ")
            .nth(2)
            .unwrap()
            .strip_prefix("a/")
            .unwrap();
        let lang = if filename.ends_with(".rs") {
            "rust,noplayground"
        } else if filename.ends_with(".gitignore") || filename.ends_with(".lock") {
            "gitconfig"
        } else if filename.ends_with(".toml") {
            "toml"
        } else if filename.ends_with(".sh") {
            "bash_script_file"
        } else {
            return Err(anyhow!("file type unknown for {filename}"));
        };
        output += format!("\n```{lang}\n").as_str();
        output += format!("(注:{filename})\n").as_str();
        let hunks = lines.iter().fold(Vec::new(), |mut acc, line| {
            if line.starts_with("diff --git")
                || line.starts_with("new file")
                || line.starts_with("---")
                || line.starts_with("+++")
                || line.starts_with("index ")
            {
                return acc;
            }
            if line.starts_with("@@") {
                acc.push(Vec::new())
            };
            acc.last_mut().unwrap().push(line.to_string());
            acc
        });
        let mut num_diff_lines = 0;
        let mut context_marker_appeared = HashSet::new();
        for lines in hunks {
            for line in &lines {
                if line.starts_with("@@ ") {
                    if num_diff_lines > 0 && !line.starts_with("@@ -1,") {
                        output += "\n// << 中略 >>\n\n";
                    }
                    let context = line
                        .split_once("@@")
                        .unwrap_or_default()
                        .1
                        .split_once("@@")
                        .unwrap_or_default();
                    let lineinfo = context
                        .0
                        .trim()
                        .split_once(' ')
                        .unwrap_or_default()
                        .1
                        .split_once(',')
                        .unwrap_or_default()
                        .0;
                    let afterlinestart: usize = lineinfo.parse().unwrap_or_default();
                    let context = context.1.strip_prefix(' ').unwrap_or_default().trim_end();
                    if context.ends_with("{") {
                        let line_before_hunk = commit_id
                            .map(|commit_id| {
                                commit_resolver
                                    .line_from_commit(
                                        commit_id,
                                        filename,
                                        afterlinestart.saturating_sub(1),
                                    )
                                    .unwrap_or_default()
                            })
                            .unwrap_or_default();
                        if lines
                            .iter()
                            .find(|line| !line.starts_with("@@ ") && line.len() > 0)
                            .unwrap()
                            .starts_with("    ")
                        {
                            if !context_marker_appeared.contains(&context.to_string()) {
                                output += context;
                                context_marker_appeared.insert(context.to_string());
                                output += "\n";
                                let line_before_hunk = line_before_hunk.trim_end();
                                if context != line_before_hunk {
                                    output += "    // << 中略 >>\n";
                                }
                            }
                        }
                    }
                    continue;
                }
                let diff_type = line.chars().nth(0).unwrap();
                let line = &line[1..].trim_end_matches('\n');
                if line.is_empty() {
                    // empty line changed. just print the new line.
                    output += "\n";
                    continue;
                }
                if line.starts_with("fn ") {
                    context_marker_appeared.insert(line.to_string());
                }
                let pre = match diff_type {
                    '+' => "**",
                    '-' => "~~",
                    ' ' => "",
                    c => todo!("diff_type = {c} is not supported yet. original chunk:\n{part}"),
                };
                let post = pre;
                output += pre;
                output += line;
                output += post;
                output += "\n";
                num_diff_lines += 1;
            }
        }
        output += "```\n";
    }
    Ok(output)
}

fn insert_commit_diff_with_change_id<T: CommitResolver>(
    commit_resolver: &T,
    lines: Vec<String>,
) -> Result<Vec<String>> {
    let mut lines_updated: Vec<String> = Vec::new();
    for line in lines {
        if line.contains("ajimi::code") && line.starts_with("<!--") {
            if let Some(change_id) = line
                .clone()
                .split(' ')
                .skip_while(|s| *s != "change_id")
                .nth(1)
            {
                if let Ok(patch) = commit_resolver.patch_from_change_id(change_id) {
                    lines_updated.push(line);
                    let patch: Vec<String> =
                        patch.trim().split('\n').map(|s| s.to_string()).collect();
                    let (hash, title) = patch
                        .first()
                        .map(|s| {
                            s.split_once(": ")
                                .context(anyhow!(
                                    "Should be : right after the short commit hash: {change_id} : {s}"
                                ))
                                .unwrap().to_owned()
                        })
                        .unwrap_or_default();
                    let patch = &patch[1..];
                    let patch = patch.join("\n");
                    let meta_commit_info = format!("<!-- ajimi::meta::title \"{title}\" -->");
                    lines_updated.push(meta_commit_info);
                    lines_updated.push(format_patch(&patch, commit_resolver, Some(hash))?);
                    let end_marker = format!("<!-- ajimi::end change_id {change_id} -->");
                    lines_updated.push(end_marker);
                }
                continue;
            }
        }
        lines_updated.push(line.clone());
    }
    Ok(lines_updated)
}

#[cfg(test)]
mod test {
    use super::*;
    use anyhow::anyhow;
    use std::collections::HashMap;

    struct MockRepo {
        commits: HashMap<String, String>,
    }
    impl MockRepo {
        fn new(commits: HashMap<String, String>) -> Self {
            Self { commits }
        }
    }
    impl CommitResolver for MockRepo {
        fn change_id_from_commit_id(&self, commit_id: &str) -> Result<String> {
            let stdout = self.commits.get(commit_id).context("commit not found")?;
            if let Some(change_id) = stdout
                .split("\n")
                .find(|s| s.trim().starts_with("Change-Id:"))
            {
                change_id
                    .split(':')
                    .skip(1)
                    .next()
                    .map(|s| s.trim().to_string())
                    .context("Change-Id line found but invalid format")
            } else {
                Err(anyhow!("commit found but does not have Change-Id properly"))
            }
        }
    }

    #[test]
    fn replace_commit_marker_with_change_id() {
        let repo = MockRepo::new(HashMap::from(
            vec![
                (
                    "95186358d01076804d10d840684a1325e281b292",
                    "
commit 95186358d01076804d10d840684a1325e281b292 (HEAD -> wasabi4b, origin/wasabi4b)
Author: hikalium <hikalium@hikalium.com>
Date:   Mon Sep 30 02:17:22 2024 +0900

    SKIP_EXPLAIN: Add scripts/check_all_commits.sh
    
    Change-Id: I5471bb84313e3f50ad0a8d4aab43509ec0732fb6
",
                ),
                (
                    "9f9107d0e653eb0f185e6be012a3a9b92055c5e1",
                    "
commit 9f9107d0e653eb0f185e6be012a3a9b92055c5e1
Author: hikalium <hikalium@hikalium.com>
Date:   Sat Sep 28 11:58:52 2024 +0900

    Cache glyphs in a font to speed up displaying chars
    
    Change-Id: Ifd40ea5f86f75f8ae4f93a0f2153c6ac73d1172b
 ",
                ),
                (
                    "85fd15d0d6c8f897d2b6ee4ee06aeb2342924b95",
                    "
commit 85fd15d0d6c8f897d2b6ee4ee06aeb2342924b95
Author: hikalium <hikalium@hikalium.com>
Date:   Sat Sep 28 11:16:33 2024 +0900

    Impl hexdump
    
    Change-Id: I011d74fe65381a8acc75a3be5c8dad182ad1de18


 ",
                ),
            ]
            .into_iter()
            .map(|e| (e.0.to_string(), e.1.to_string()))
            .collect::<HashMap<String, String>>(),
        ));

        // if there is a commit tag, replace it with change-id.
        assert_eq!(
            replace_commit_id_with_change_id(
                &repo,
                vec!["<!-- ajimi::code commit 85fd15d0d6c8f897d2b6ee4ee06aeb2342924b95 -->"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            )
            .unwrap(),
            vec!["<!-- ajimi::code change_id I011d74fe65381a8acc75a3be5c8dad182ad1de18 -->"]
        );

        // if there is an invalid commit tag, keep the line as is.
        assert_eq!(
            replace_commit_id_with_change_id(
                &repo,
                vec!["<!-- ajimi::code commit broken_commit_id -->"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            )
            .unwrap(),
            vec!["<!-- ajimi::code commit broken_commit_id -->"]
        );

        // if there is a change_id tag, do not modify it.
        assert_eq!(
            replace_commit_id_with_change_id(
                &repo,
                vec!["<!-- ajimi::code change_id I011d74fe65381a8acc75a3be5c8dad182ad1de18 -->"]
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect()
            )
            .unwrap(),
            vec!["<!-- ajimi::code change_id I011d74fe65381a8acc75a3be5c8dad182ad1de18 -->"]
        );
    }

    #[test]
    fn format_patch_samples() {
        let repo = MockRepo::new(HashMap::new());
        let repo = &repo;
        assert_eq!(format_patch("", repo, None).unwrap(), "");
        assert!(format_patch("aaa", repo, None).is_err());
        assert_eq!(
            format_patch(
                r#"
diff --git a/src/main.rs b/src/main.rs
index e7a11a9..2c7001e 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,5 @@
 fn main() {
     println!("Hello, world!");
+    #[allow(clippy::empty_loop)]
+    loop {}
 }
"#,
                repo,
                None
            )
            .unwrap(),
            r#"
```rust,noplayground
(注:src/main.rs)
fn main() {
    println!("Hello, world!");
**    #[allow(clippy::empty_loop)]**
**    loop {}**
}
```
"#
        );
    }
}
