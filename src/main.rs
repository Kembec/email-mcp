use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::task::JoinSet;

mod config;
mod gmail;
mod icloud;
mod mcp;
mod oauth;
mod outlook;
mod tools;

pub use config::AppState;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    eprintln!("email-mcp starting");

    let state = Arc::new(AppState::new()?);
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let stdout = Arc::new(tokio::sync::Mutex::new(tokio::io::stdout()));
    let mut tasks: JoinSet<()> = JoinSet::new();

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let state = state.clone();
        let stdout = stdout.clone();
        tasks.spawn(async move {
            if let Some(response) = mcp::handle_line(&line, &state).await {
                let mut out = stdout.lock().await;
                let _ = out.write_all(response.as_bytes()).await;
                let _ = out.write_all(b"\n").await;
                let _ = out.flush().await;
            }
        });
    }

    // Wait for all in-flight requests to complete before exiting
    while tasks.join_next().await.is_some() {}

    Ok(())
}
