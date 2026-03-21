use log::info;
use std::io::{self, Write};

use crate::token::super_token::roll_super_token;

pub async fn run() {
    let should_continue = prompt_yes_or_no(
        "This action will delete the current super token (id=1) and generate a new one. Continue? [y/n]: ",
    );
    if !should_continue {
        info!("Super token rotation cancelled by user.");
        return;
    }

    match roll_super_token().await {
        Ok((token, root_password)) => {
            info!("Super token rotated successfully.");
            info!("Super Token: {}", token);
            info!("Root Password: {}", root_password);
        }
        Err(e) => {
            panic!("Failed to rotate super token: {e}");
        }
    }
}

fn prompt_yes_or_no(prompt: &str) -> bool {
    loop {
        print!("{prompt}");
        if let Err(e) = io::stdout().flush() {
            println!("Failed to flush stdout: {e}. Please type y or n.");
        }

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                let normalized = input.trim().to_ascii_lowercase();
                match normalized.as_str() {
                    "y" | "yes" => return true,
                    "n" | "no" => return false,
                    _ => {
                        println!("Invalid input. Please type y or n.");
                    }
                }
            }
            Err(e) => {
                println!("Failed to read input: {e}. Please type y or n.");
            }
        }
    }
}
