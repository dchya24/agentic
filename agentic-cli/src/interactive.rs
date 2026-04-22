use crate::commands::Commands;
use anyhow::Result;

pub fn run(commands: Commands) -> Result<()> {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║      🤖 Agentic Interactive Mode         ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Type 'help' for commands                ║");
    println!("║  'exit' to quit                          ║");
    println!("╚══════════════════════════════════════════╝\n");

    loop {
        print!("> ");
        std::io::Write::flush(&mut std::io::stdout())?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        match input.to_lowercase().as_str() {
            "exit" | "quit" | "q" => {
                println!("\n👋 Goodbye!");
                break;
            }
            "help" | "h" => {
                print_help();
            }
            "clear" => {
                print!("\x1b[2J\x1b[H");
                std::io::Write::flush(&mut std::io::stdout())?;
            }
            _ => {
                if let Err(e) = commands.run(input) {
                    eprintln!("Error: {}", e);
                }
            }
        }
    }

    Ok(())
}

fn print_help() {
    println!("\n╔══════════════════════════════════════════╗");
    println!("║              Commands                     ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  help, h      - Show this help            ║");
    println!("║  clear        - Clear screen              ║");
    println!("║  exit, q      - Exit interactive mode     ║");
    println!("╚══════════════════════════════════════════╝\n");
}
