use anyhow::Result;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::config::Config;

use super::{atty_check, create_agent, send_and_wait, setup_logging, spawn_agent_worker};

pub async fn run(
    config_path: &Path,
    data_dir: &Path,
    message: Option<String>,
    session_id: &str,
) -> Result<()> {
    setup_logging();
    let config = Config::load(config_path)?;
    let agent = create_agent(&config, data_dir).await?;
    let inbound_tx = spawn_agent_worker(agent);

    // Single-shot mode
    if let Some(msg) = message {
        let output = send_and_wait(&inbound_tx, &msg, session_id).await?;
        println!("{}", output.content);
        return Ok(());
    }

    // REPL mode
    let is_tty = atty_check();
    if is_tty {
        println!(
            "UniClaw v{} | {} | {}",
            env!("CARGO_PKG_VERSION"),
            config.llm.model,
            std::env::consts::ARCH
        );
        println!("Type 'exit' or Ctrl+C to quit.\n");
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();

    loop {
        if is_tty {
            print!("You> ");
            io::stdout().flush()?;
        }

        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "exit" || trimmed == "quit" {
            if is_tty {
                println!("Goodbye!");
            }
            break;
        }

        match send_and_wait(&inbound_tx, trimmed, session_id).await {
            Ok(output) => {
                if is_tty {
                    println!("UniClaw> {}\n", output.content);
                } else {
                    println!("{}", output.content);
                }
            }
            Err(e) => eprintln!("Error: {e}"),
        }
    }

    Ok(())
}
