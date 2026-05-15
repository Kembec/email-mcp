use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

mod config;
mod gmail;
mod mcp;
mod oauth;
mod outlook;
mod tools;

pub use config::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("email-mcp starting");

    let state = Arc::new(AppState::new()?);
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let stdout = Arc::new(tokio::sync::Mutex::new(tokio::io::stdout()));

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let state = state.clone();
        let stdout = stdout.clone();
        tokio::spawn(async move {
            if let Some(response) = mcp::handle_line(&line, &state).await {
                let mut out = stdout.lock().await;
                let _ = out.write_all(response.as_bytes()).await;
                let _ = out.write_all(b"\n").await;
                let _ = out.flush().await;
            }
        });
    }

    Ok(())
}
