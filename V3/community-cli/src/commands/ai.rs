//! Hiran AI — chat and status via OpenAI-compatible HTTP endpoint.
//!
//! Talks to the public Hiran inference service (llama-server / LM Studio / Ollama).
//! No local model management, no deploy, no quantize — just chat.

use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use reqwest::Client;
use serde_json::{json, Value};
use std::io::{self, Write};

use crate::config::Config;
use crate::ui;

#[derive(Subcommand)]
pub enum AiCmd {
    /// Check if the Hiran AI endpoint is reachable
    Status,
    /// Ask a single question to Hiran
    Ask {
        /// Your question
        question: String,
    },
    /// Interactive chat session with Hiran
    Chat,
}

pub async fn run(cfg: &Config, cmd: AiCmd) -> Result<()> {
    if cfg.ai.url.is_empty() {
        ui::print_header("Hiran AI");
        ui::print_info("AI endpoint is not configured (optional feature).");
        ui::print_info("Set with: zion config set ai.url <endpoint>");
        println!();
        return Ok(());
    }
    match cmd {
        AiCmd::Status => ai_status(cfg).await,
        AiCmd::Ask { question } => {
            ui::print_header("Hiran AI");
            match ask(cfg, &question).await {
                Ok(answer) => println!("  ◉ {}", answer),
                Err(e) => ui::print_err(&format!("Hiran error: {}", e)),
            }
            println!();
            Ok(())
        }
        AiCmd::Chat => chat_session(cfg).await,
    }
}

async fn ai_status(cfg: &Config) -> Result<()> {
    ui::print_header("Hiran AI Status");
    ui::print_row("Endpoint", &cfg.ai.url);
    ui::print_row("Model", &cfg.ai.model);

    let url = format!("{}/health", cfg.ai.url.trim_end_matches('/'));
    let resp = Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            ui::print_ok("Hiran AI is online and healthy.");
        }
        Ok(r) => {
            ui::print_warn(&format!("Endpoint responded with HTTP {}", r.status()));
        }
        Err(e) => {
            ui::print_err(&format!("Cannot reach Hiran AI: {}", e));
        }
    }

    // Also try /v1/models for richer info
    let models_url = format!("{}/v1/models", cfg.ai.url.trim_end_matches('/'));
    let models_resp = Client::new()
        .get(&models_url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;

    if let Ok(r) = models_resp {
        if r.status().is_success() {
            if let Ok(v) = r.json::<Value>().await {
                if let Some(models) = v.get("data").and_then(|d| d.as_array()) {
                    println!();
                    ui::print_section("Available Models");
                    for m in models {
                        let id = m.get("id").and_then(|i| i.as_str()).unwrap_or("?");
                        println!("  • {}", id);
                    }
                }
            }
        }
    }

    println!();
    Ok(())
}

async fn ask(cfg: &Config, question: &str) -> Result<String> {
    let url = format!("{}/v1/chat/completions", cfg.ai.url.trim_end_matches('/'));
    let body = json!({
        "model": cfg.ai.model,
        "messages": [
            { "role": "user", "content": question }
        ],
        "temperature": 0.7,
        "max_tokens": 1024
    });

    let resp: Value = Client::new()
        .post(&url)
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await?
        .json()
        .await?;

    let answer = resp["choices"][0]["message"]["content"]
        .as_str()
        .or_else(|| resp["response"].as_str())
        .or_else(|| resp["message"].as_str())
        .or_else(|| resp["text"].as_str())
        .unwrap_or("(no response)")
        .to_string();

    Ok(answer)
}

async fn chat_session(cfg: &Config) -> Result<()> {
    ui::print_header("Hiran AI Chat");
    ui::print_info(&format!("Endpoint: {}", cfg.ai.url));
    ui::print_info("Type 'exit' or 'quit' to leave. 'clear' resets history.");
    println!();

    let mut history: Vec<Value> = Vec::new();
    let stdin = io::stdin();

    loop {
        print!("{} ", "you>".cyan().bold());
        io::stdout().flush()?;

        let mut input = String::new();
        stdin.read_line(&mut input)?;
        let input = input.trim().to_string();

        if input.is_empty() {
            continue;
        }
        if input == "exit" || input == "quit" {
            ui::print_info("Goodbye!");
            break;
        }
        if input == "clear" {
            history.clear();
            ui::print_info("History cleared.");
            continue;
        }

        history.push(json!({ "role": "user", "content": input }));

        let url = format!("{}/v1/chat/completions", cfg.ai.url.trim_end_matches('/'));
        let body = json!({
            "model": cfg.ai.model,
            "messages": history,
            "temperature": 0.7,
            "max_tokens": 1024
        });

        print!("{} ", "hiran>".yellow().bold());
        io::stdout().flush()?;

        let resp = Client::new()
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .await;

        match resp {
            Ok(r) => {
                if !r.status().is_success() {
                    ui::print_err(&format!("HTTP {}", r.status()));
                    continue;
                }
                let v: Value = r.json().await?;
                let answer = v["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or("(no response)")
                    .to_string();
                println!("{}", answer);
                println!();
                history.push(json!({ "role": "assistant", "content": answer }));
            }
            Err(e) => {
                ui::print_err(&format!("Request failed: {}", e));
            }
        }
    }

    Ok(())
}
