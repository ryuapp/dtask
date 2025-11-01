use clap::Parser;
use deno_task_shell::{
    KillSignal, ShellPipeReader, ShellPipeWriter, ShellState, execute_with_pipes, parser::parse,
};
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(name = "dtask")]
#[command(about = "Execute tasks defined in deno.json/deno.jsonc", long_about = None)]
struct Args {
    /// Task name to execute
    task: String,

    /// Additional arguments to pass to the task
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Load deno.json/deno.jsonc
    match load_deno_config() {
        Ok(config) => {
            // Extract tasks
            match parse_tasks(&config) {
                Ok(tasks) => {
                    // Find the requested task
                    if let Some(command) = tasks.get(&args.task) {
                        // Build final command with extra arguments
                        let final_command = if args.args.is_empty() {
                            command.clone()
                        } else {
                            format!("{} {}", command, args.args.join(" "))
                        };

                        // Execute the task
                        match execute_task(&final_command).await {
                            Ok(status) => {
                                std::process::exit(status);
                            }
                            Err(e) => {
                                eprintln!("Error executing task '{}': {}", args.task, e);
                                std::process::exit(1);
                            }
                        }
                    } else {
                        eprintln!("Task '{}' not found in deno.json/deno.jsonc", args.task);
                        eprintln!(
                            "Available tasks: {}",
                            tasks.keys().cloned().collect::<Vec<_>>().join(", ")
                        );
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error parsing tasks: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Error loading deno.json/deno.jsonc: {}", e);
            std::process::exit(1);
        }
    }
}

/// Load deno.json or deno.jsonc file
fn load_deno_config() -> Result<Value, String> {
    // Check for deno.json first, then deno.jsonc
    for filename in &["deno.json", "deno.jsonc"] {
        let path = Path::new(filename);
        if path.exists() {
            let content = fs::read_to_string(path)
                .map_err(|e| format!("Failed to read {}: {}", filename, e))?;

            // Remove comments from jsonc
            let json_str = if filename.ends_with(".jsonc") {
                remove_json_comments(&content)
            } else {
                content
            };

            return serde_json::from_str(&json_str)
                .map_err(|e| format!("Failed to parse {}: {}", filename, e));
        }
    }

    Err("Neither deno.json nor deno.jsonc found in current directory".to_string())
}

/// Remove single-line and multi-line comments from JSON
fn remove_json_comments(content: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '/' {
            // Skip single-line comment
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            if i < chars.len() {
                result.push('\n');
                i += 1;
            }
        } else if i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '*' {
            // Skip multi-line comment
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                if chars[i] == '\n' {
                    result.push('\n');
                }
                i += 1;
            }
            if i + 1 < chars.len() {
                i += 2; // Skip */
            }
        } else if chars[i] == '"' {
            // Handle strings (don't remove comments inside strings)
            result.push(chars[i]);
            i += 1;
            while i < chars.len() {
                if chars[i] == '"' && (i == 0 || chars[i - 1] != '\\') {
                    result.push(chars[i]);
                    i += 1;
                    break;
                }
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Parse tasks from deno.json config
fn parse_tasks(config: &Value) -> Result<std::collections::HashMap<String, String>, String> {
    let mut tasks = std::collections::HashMap::new();

    if let Some(tasks_obj) = config.get("tasks") {
        if let Some(tasks_map) = tasks_obj.as_object() {
            for (name, value) in tasks_map {
                if let Some(command) = value.as_str() {
                    tasks.insert(name.clone(), command.to_string());
                } else {
                    return Err(format!(
                        "Task '{}' is not a string. Tasks must be strings.",
                        name
                    ));
                }
            }
        } else {
            return Err("'tasks' field must be an object".to_string());
        }
    } else {
        return Err("No 'tasks' field found in deno.json/deno.jsonc".to_string());
    }

    Ok(tasks)
}

/// Execute a task command using deno_task_shell
async fn execute_task(command: &str) -> Result<i32, String> {
    // Parse the command string
    let list = parse(command).map_err(|e| format!("Failed to parse command: {}", e))?;

    // Prepare environment variables
    let env_vars: HashMap<OsString, OsString> = std::env::vars_os().collect();

    // Prepare working directory
    let cwd =
        std::env::current_dir().map_err(|e| format!("Failed to get current directory: {}", e))?;

    // Prepare state
    let kill_signal = KillSignal::default();
    let custom_commands = HashMap::new();

    // Create shell state
    let state = ShellState::new(env_vars, cwd, custom_commands, kill_signal);

    // Execute the command with stdin, stdout, stderr connected to the terminal
    let exit_code = execute_with_pipes(
        list,
        state,
        ShellPipeReader::stdin(),
        ShellPipeWriter::stdout(),
        ShellPipeWriter::stderr(),
    )
    .await;

    Ok(exit_code)
}
