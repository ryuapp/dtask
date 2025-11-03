use clap::Parser;
use colored::Colorize;
use deno_config::deno_json::ConfigFile;
use deno_task_shell::{
    KillSignal, ShellPipeReader, ShellPipeWriter, ShellState, execute_with_pipes, parser::parse,
};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use url::Url;

#[derive(Parser, Debug)]
#[command(name = "dtask")]
#[command(about = "Execute tasks defined in deno.json/deno.jsonc")]
struct Args {
    /// Task name to execute (omit to list available tasks)
    task: Option<String>,

    /// Additional arguments to pass to the task
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Load and parse deno.json/deno.jsonc
    match load_deno_config() {
        Ok(tasks) => {
            // If no task specified, show available tasks
            if args.task.is_none() {
                println!("{}", "Available tasks:".green());
                for (name, command) in &tasks {
                    println!("- {}", name.cyan());
                    println!("    {}", command);
                }
                return;
            }

            let task_name = args.task.unwrap();
            // Find the requested task
            if let Some(command) = tasks.get(&task_name) {
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
                        eprintln!("Error executing task '{}': {}", task_name, e);
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("Task not found: {}", task_name);
                eprintln!("{}", "Available tasks:".green());
                for (name, command) in &tasks {
                    eprintln!("- {}", name.cyan());
                    eprintln!("    {}", command);
                }
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error loading deno.json/deno.jsonc: {}", e);
            std::process::exit(1);
        }
    }
}

/// Load deno.json or deno.jsonc file and extract tasks with insertion order preserved
fn load_deno_config() -> Result<IndexMap<String, String>, String> {
    // Check for deno.json first, then deno.jsonc
    for filename in &["deno.json", "deno.jsonc"] {
        let path = Path::new(filename);
        if path.exists() {
            let content = fs::read_to_string(path)
                .map_err(|e| format!("Failed to read {}: {}", filename, e))?;

            // Parse using ConfigFile from deno_config
            let specifier = Url::from_file_path(std::env::current_dir().unwrap().join(filename))
                .map_err(|_| "Failed to create URL specifier".to_string())?;

            let config = ConfigFile::new(&content, specifier)
                .map_err(|e| format!("Failed to parse {}: {}", filename, e))?;

            // Extract tasks using deno_config's proper API
            let tasks_config = config
                .to_tasks_config()
                .map_err(|e| format!("Failed to extract tasks: {}", e))?;

            match tasks_config {
                Some(tasks) => {
                    // Convert TaskDefinition to simple String commands
                    let mut result = IndexMap::new();
                    for (name, task_def) in tasks {
                        if let Some(command) = task_def.command {
                            result.insert(name, command);
                        }
                    }
                    if result.is_empty() {
                        return Err("No tasks defined in deno.json/deno.jsonc".to_string());
                    }
                    return Ok(result);
                }
                None => continue,
            }
        }
    }

    Err("Neither deno.json nor deno.jsonc found in current directory".to_string())
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
