use clap::Parser;
use deno_task_shell::{
    KillSignal, ShellPipeReader, ShellPipeWriter, ShellState, execute_with_pipes, parser::parse,
};
use jsonc_parser::parse_to_serde_value;
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

            // Parse using jsonc-parser with serde feature to get serde_json::Value directly
            let value = parse_to_serde_value(&content, &Default::default())
                .map_err(|e| format!("Failed to parse {}: {}", filename, e))?
                .ok_or_else(|| format!("Empty file: {}", filename))?;

            return Ok(value);
        }
    }

    Err("Neither deno.json nor deno.jsonc found in current directory".to_string())
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
