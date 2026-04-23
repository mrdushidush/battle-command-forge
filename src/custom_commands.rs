/// Custom commands from `.battlecommand/commands/*.md` files.
///
/// Each .md file becomes a command:
///   .battlecommand/commands/deploy.md -> /deploy
///
/// File format:
/// ```markdown
/// # Deploy
/// Description: Deploy the current project
/// Model: qwen2.5-coder:7b
///
/// ## Prompt
/// Deploy the application by creating a Dockerfile and docker-compose.yml...
/// ```
use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CustomCommand {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub prompt: String,
}

/// Load all custom commands from `.battlecommand/commands/`.
pub async fn load_commands(workspace: &str) -> Result<Vec<CustomCommand>> {
    let dir = format!("{}/.battlecommand/commands", workspace);
    if !Path::new(&dir).exists() {
        return Ok(Vec::new());
    }

    let mut commands = Vec::new();
    let mut entries = tokio::fs::read_dir(&dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().map(|e| e == "md").unwrap_or(false) {
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                if let Some(cmd) = parse_command_file(&path, &content) {
                    commands.push(cmd);
                }
            }
        }
    }

    commands.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(commands)
}

fn parse_command_file(path: &Path, content: &str) -> Option<CustomCommand> {
    let name = path.file_stem()?.to_str()?.to_string();
    let mut description = String::new();
    let mut model = None;
    let mut prompt = String::new();
    let mut in_prompt = false;

    for line in content.lines() {
        if line.starts_with("Description:") {
            description = line.trim_start_matches("Description:").trim().to_string();
        } else if line.starts_with("Model:") {
            model = Some(line.trim_start_matches("Model:").trim().to_string());
        } else if line.trim() == "## Prompt" {
            in_prompt = true;
        } else if in_prompt {
            prompt.push_str(line);
            prompt.push('\n');
        }
    }

    if prompt.is_empty() {
        prompt = content.to_string();
    }

    Some(CustomCommand {
        name,
        description,
        model,
        prompt: prompt.trim().to_string(),
    })
}

/// Create an example custom command.
pub async fn create_example_command(workspace: &str) -> Result<()> {
    let dir = format!("{}/.battlecommand/commands", workspace);
    tokio::fs::create_dir_all(&dir).await?;

    let example = r#"# Review
Description: Run a comprehensive code review on the project
Model: qwen2.5-coder:7b

## Prompt
Review all Python files in the tasks/ directory. For each file:
1. Check code quality and style
2. Identify potential bugs
3. Suggest improvements
4. Rate each file 1-10

Output a summary table with file names and scores.
"#;

    tokio::fs::write(format!("{}/review.md", dir), example).await?;
    println!("Created example command: .battlecommand/commands/review.md");
    Ok(())
}

/// Format commands for help display.
pub fn format_commands_help(commands: &[CustomCommand]) -> String {
    if commands.is_empty() {
        return "No custom commands. Create .battlecommand/commands/<name>.md to add one."
            .to_string();
    }
    let mut output = String::from("Custom Commands:\n");
    for cmd in commands {
        output.push_str(&format!(
            "  /{:<15} {}\n",
            cmd.name,
            if cmd.description.is_empty() {
                "(no description)"
            } else {
                &cmd.description
            }
        ));
    }
    output
}
