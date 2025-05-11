use crate::repo::CommitResolver;
use crate::repo::GitRepo;
use anyhow::anyhow;
use anyhow::Result;
use argh::FromArgs;
use regex::Regex;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

#[derive(FromArgs, PartialEq, Debug)]
/// Check the files
#[argh(subcommand, name = "check")]
pub struct Args {
    /// git repo for commits
    #[argh(option)]
    code: PathBuf,
    /// files to fix
    #[argh(positional)]
    files: Vec<String>,
}
impl Args {
    fn extract_codeblock_start_markers(
        paths: &Vec<String>,
    ) -> Result<Vec<(String, usize, String)>> {
        let mut results = Vec::new();

        for path_str in paths {
            let path = Path::new(&path_str);
            let file =
                File::open(path).map_err(|_| anyhow!("Failed to open file: {}", path_str))?;
            let reader = BufReader::new(file);
            let mut code_block_stack = Vec::new();

            for (line_number, line_result) in reader.lines().enumerate() {
                let line = line_result.map_err(|_| {
                    anyhow!(
                        "Failed to read line {} in file: {}",
                        line_number + 1,
                        path_str
                    )
                })?;

                if line.starts_with("```") {
                    if code_block_stack.is_empty() {
                        code_block_stack.push(line_number + 1);
                        results.push((path_str.clone(), line_number + 1, line));
                    } else {
                        code_block_stack.pop();
                    }
                }
            }

            if !code_block_stack.is_empty() {
                return Err(anyhow!(
                    "Unclosed code block in file: {}, started at line: {}",
                    path_str,
                    code_block_stack[0]
                ));
            }
        }

        Ok(results)
    }
    fn verify_codeblock_start_markers(&self) -> Result<()> {
        let codeblock_start_markers = Self::extract_codeblock_start_markers(&self.files)?;
        let mut prev_file_name = None;
        let mut is_fix_needed = false;
        for (file, line_num, line) in codeblock_start_markers {
            let lang = line.strip_prefix("```").unwrap_or_default();
            let is_first_codeblock = if let Some(prev_file_name) = prev_file_name {
                prev_file_name != file
            } else {
                true
            };
            prev_file_name = Some(file.clone());
            match lang {
                "rust,noplayground" | "rust" | "bash" | "txt" | "toml" | "bash_script_file"
                | "gitconfig" => continue,
                "" => {
                    if is_first_codeblock {
                        continue;
                    } else {
                        is_fix_needed = true;
                        println!("{file}:{line_num}: {line}")
                    }
                }
                _ => {
                    is_fix_needed = true;
                    println!("Unknown block lang: {file}:{line_num}: {line}")
                }
            }
        }
        if is_fix_needed {
            Err(anyhow!("Found some issues. Please fix them and try again!"))
        } else {
            println!("PASS. It tastes good!");
            Ok(())
        }
    }
    fn verify_generated_code(&self) -> Result<()> {
        let mut is_fix_needed = false;

        let mut id_to_book_path: HashMap<String, String> = HashMap::new();
        let mut change_ids_in_book = Vec::new();
        eprintln!("checking {} files...", self.files.len());
        for file in &self.files {
            let lines = fs::read_to_string(file)?;
            let mut lines: Vec<String> = lines
                .split("\n")
                .filter(|s| s.contains("ajimi::code change_id"))
                .map(|s| {
                    s.split(" ")
                        .skip_while(|s| s != &"change_id")
                        .skip(1)
                        .next()
                        .unwrap_or("invalid")
                        .to_string()
                })
                .collect();
            id_to_book_path.extend(
                lines
                    .iter()
                    .map(|id| (id.to_string(), file.to_string()))
                    .collect::<Vec<(String, String)>>(),
            );
            change_ids_in_book.extend(lines.drain(..));
        }
        println!(
            "Total: {} ajimi change_ids found in the book.",
            change_ids_in_book.len()
        );
        let repo = GitRepo::new(self.code.clone());
        let change_ids_in_repo = repo.all_commit_summary_in_tree()?;
        println!(
            "Total: {} ajimi change_ids found in the repo.",
            change_ids_in_repo.len()
        );
        let mut repo_order_map = HashMap::new();
        for (i, e) in change_ids_in_repo.iter().rev().enumerate() {
            repo_order_map.insert(&e.change_id, (i, e));
        }
        let mut next_expected_order = 0;
        let mut found_ids: HashSet<String> = HashSet::new();
        for id_in_book in change_ids_in_book {
            let book_path = id_to_book_path
                .get(&id_in_book)
                .map(|s| s.as_str())
                .unwrap_or("?");
            if let Some((order, _)) = repo_order_map.get(&id_in_book) {
                if *order < next_expected_order {
                    println!("{id_in_book} @ {book_path}: order should not go back");
                    is_fix_needed = true;
                } else {
                    next_expected_order = *order + 1;
                }
                found_ids.insert(id_in_book);
            } else {
                println!("{id_in_book} @ {book_path}: change_id not found in the code");
                is_fix_needed = true;
            }
        }
        for e in change_ids_in_repo.iter().rev() {
            if !found_ids.contains(&e.change_id) && !e.title.contains("SKIP_EXPLAIN: ") {
                println!(
                    "change in code but book: <!-- ajimi::code change_id {} -->",
                    e.change_id
                );
                println!("  {}", e.title);
                is_fix_needed = true;
            }
        }
        if is_fix_needed {
            Err(anyhow!("Found some issues. Please fix them and try again!"))
        } else {
            println!("PASS. It tastes good!");
            Ok(())
        }
    }
    fn extract_image_source_comments(
        paths: &Vec<String>,
    ) -> Result<Vec<(String, usize, Option<String>, String)>> {
        let mut results = Vec::new();
        let re = Regex::new(r"!\[(.*?)\]\((.*?)\)").unwrap();

        for path_str in paths {
            let path = Path::new(&path_str);
            let content = fs::read_to_string(path)
                .map_err(|_| anyhow!("Failed to open file: {}", path_str))?;
            let lines: Vec<&str> = content.lines().collect();

            for i in 1..lines.len() {
                if !re.is_match(lines[i]) {
                    continue;
                }
                let tag = if lines[i - 1].starts_with("<!-- ") {
                    Some(lines[i - 1].to_string())
                } else {
                    None
                };
                results.push((path_str.to_string(), i - 1, tag, lines[i].to_string()))
            }
        }
        Ok(results)
    }
    fn verify_image_source_comments(&self) -> Result<()> {
        let mut is_fix_needed = false;
        let markers = Self::extract_image_source_comments(&self.files)?;
        for (file, line_num, line, imgline) in markers {
            if line.is_none() || imgline.contains("![]") {
                println!("{file}:{line_num}: {line:?}: {imgline}");
                is_fix_needed = true;
            }
        }
        if is_fix_needed {
            Err(anyhow!("Found some issues. Please fix them and try again!"))
        } else {
            println!("PASS. It tastes good!");
            Ok(())
        }
    }
    pub fn run(&self) -> Result<()> {
        self.verify_generated_code()?;
        self.verify_codeblock_start_markers()?;
        self.verify_image_source_comments()?;
        Ok(())
    }
}
