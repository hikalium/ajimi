use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug)]
pub struct CommitMetadata {
    pub hash: String,
    pub title: String,
    pub change_id: String,
}

pub struct GitRepo {
    path: PathBuf,
}
impl GitRepo {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}
impl CommitResolver for GitRepo {
    fn change_id_from_commit_id(&self, commit_id: &str) -> Result<String> {
        let output = std::process::Command::new("git")
            .args([
                "-C",
                self.path
                    .as_os_str()
                    .to_str()
                    .expect("path is not valid in utf-8"),
                "log",
                "-1",
                commit_id,
            ])
            .output()
            .expect("Failed to get Change-Id from a commit");
        let stdout =
            String::from_utf8(output.stdout).expect("Failed to parse git output as a UTF-8 string");
        let stderr =
            String::from_utf8(output.stderr).expect("Failed to parse git output as a UTF-8 string");
        if !output.status.success() {
            Err(anyhow!("git cmd failed: {stderr}"))
        } else if let Some(change_id) = stdout
            .split("\n")
            .find(|s| s.trim().starts_with("Change-Id:"))
        {
            change_id
                .split(':')
                .nth(1)
                .map(|s| s.trim().to_string())
                .context("Change-Id line found but invalid format")
        } else {
            Err(anyhow!("commit found but does not have Change-Id properly"))
        }
    }
    fn patch_from_change_id(&self, change_id: &str) -> Result<String> {
        let output = std::process::Command::new("git")
            .args([
                "-C",
                self.path
                    .as_os_str()
                    .to_str()
                    .expect("path is not valid in utf-8"),
                "log",
                "-1",
                "-p",
                "--grep",
                change_id,
                r#"--pretty=%h: %s"#,
            ])
            .output()
            .expect("Failed to get a diff");
        let stdout =
            String::from_utf8(output.stdout).expect("Failed to parse git output as a UTF-8 string");
        let stderr =
            String::from_utf8(output.stderr).expect("Failed to parse git output as a UTF-8 string");
        if !output.status.success() {
            Err(anyhow!("git cmd failed: {stderr}"))
        } else {
            Ok(stdout.to_string())
        }
    }
    fn all_commit_summary_in_tree(&self) -> Result<Vec<CommitMetadata>> {
        let output = std::process::Command::new("git")
            .args([
                "-C",
                self.path
                    .as_os_str()
                    .to_str()
                    .expect("path is not valid in utf-8"),
                "log",
                r#"--pretty=COMMIT:%H:%s%n%B"#,
            ])
            .output()
            .expect("Failed to get git log");
        let stdout =
            String::from_utf8(output.stdout).expect("Failed to parse git output as a UTF-8 string");
        let stderr =
            String::from_utf8(output.stderr).expect("Failed to parse git output as a UTF-8 string");
        if !output.status.success() {
            Err(anyhow!("git cmd failed: {stderr}"))
        } else {
            let lines: Vec<String> = stdout
                .to_string()
                .split("\n")
                .filter(|e| e.starts_with("Change-Id:") || e.starts_with("COMMIT:"))
                .map(|s| s.to_string())
                .collect();
            Ok(lines
                .chunks_exact(2)
                .map(|e| {
                    let commit_info = &e[0];
                    let mut it = commit_info.split(":").into_iter();
                    let hash = it.by_ref().skip(1).next();
                    let title = it.collect::<Vec<&str>>().join(":");
                    let change_id_line = &e[1];
                    CommitMetadata {
                        hash: hash.unwrap_or_default().trim().to_string(),
                        title,
                        change_id: change_id_line
                            .split(":")
                            .last()
                            .unwrap_or_default()
                            .trim()
                            .to_string(),
                    }
                })
                .collect())
        }
    }
    fn line_from_commit(&self, commit_id: &str, file: &str, line_number: usize) -> Result<String> {
        if line_number < 1 {
            return Err(anyhow!("line_number < 1"));
        }
        let query = format!("{commit_id}:{file}");
        let output = std::process::Command::new("git")
            .args([
                "-C",
                self.path
                    .as_os_str()
                    .to_str()
                    .expect("path is not valid in utf-8"),
                "show",
                &query,
            ])
            .output()
            .expect("Failed to get git log");
        let stdout =
            String::from_utf8(output.stdout).expect("Failed to parse git output as a UTF-8 string");
        let stderr =
            String::from_utf8(output.stderr).expect("Failed to parse git output as a UTF-8 string");
        if !output.status.success() {
            Err(anyhow!("git cmd failed: {stderr}"))
        } else {
            let lines: Vec<String> = stdout
                .to_string()
                .split("\n")
                .map(|s| s.to_string())
                .collect();
            lines
                .get(line_number - 1)
                .ok_or(anyhow!("Line out of range"))
                .cloned()
        }
    }
}

pub trait CommitResolver {
    fn change_id_from_commit_id(&self, commit_id: &str) -> Result<String>;
    fn patch_from_change_id(&self, _change_id: &str) -> Result<String> {
        Ok("
+SAMPLE_CHANGE
 SAMPLE_CHANGE
-SAMPLE_CHANGE
"
        .to_string())
    }
    fn line_from_commit(
        &self,
        _change_id: &str,
        _file: &str,
        _line_number: usize,
    ) -> Result<String> {
        unimplemented!()
    }
    fn all_commit_summary_in_tree(&self) -> Result<Vec<CommitMetadata>> {
        unimplemented!()
    }
}
