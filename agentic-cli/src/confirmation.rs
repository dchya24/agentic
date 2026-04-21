use core_agentic::{ConfirmationRequest, RiskLevel};

pub enum ConfirmationResponse {
    Yes,
    No,
    Always,
    Quit,
}

pub fn prompt_confirmation(request: &ConfirmationRequest) -> Option<ConfirmationResponse> {
    let risk_str = match request.risk_level {
        RiskLevel::Low => "LOW",
        RiskLevel::Medium => "MEDIUM",
        RiskLevel::High => "HIGH",
        RiskLevel::Critical => "CRITICAL",
    };

    println!();
    println!("┌─ ⚠️  CONFIRMATION REQUIRED ──────────────────────────────────┐");
    println!(
        "│ Risk Level: {}                                           │",
        risk_str
    );
    println!(
        "│ Action: {}                                     │",
        truncate(&request.action, 50)
    );
    println!(
        "│ Description: {}                                 │",
        truncate(&request.description, 50)
    );
    println!("├───────────────────────────────────────────────────────────┤");
    println!("│ [y] Yes  [n] No  [a] Always  [q] Quit                     │");
    println!("└───────────────────────────────────────────────────────────┘");
    print!("> ");

    loop {
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            return None;
        }

        let input = input.trim().to_lowercase();
        match input.as_str() {
            "y" | "yes" => return Some(ConfirmationResponse::Yes),
            "n" | "no" => return Some(ConfirmationResponse::No),
            "a" | "always" => return Some(ConfirmationResponse::Always),
            "q" | "quit" => return Some(ConfirmationResponse::Quit),
            _ => {
                print!("Invalid input. Enter (y/n/a/q): ");
            }
        }
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
