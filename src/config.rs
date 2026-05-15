use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Gmail,
    Outlook,
    ICloud,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Gmail => write!(f, "gmail"),
            Provider::Outlook => write!(f, "outlook"),
            Provider::ICloud => write!(f, "icloud"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub provider: Provider,
    pub email: Option<String>,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    pub icloud_password: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct AccountsFile {
    accounts: Vec<Account>,
}

pub struct PendingAuth {
    pub account_id: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub provider: Provider,
    pub client_id: String,
    pub client_secret: Option<String>,
}

pub struct AppState {
    pub config_dir: PathBuf,
    pub pending_auths: Mutex<HashMap<String, PendingAuth>>,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("email-mcp");
        std::fs::create_dir_all(&config_dir)?;
        Ok(Self {
            config_dir,
            pending_auths: Mutex::new(HashMap::new()),
        })
    }

    fn accounts_path(&self) -> PathBuf {
        self.config_dir.join("accounts.json")
    }

    pub fn load_accounts(&self) -> Result<Vec<Account>> {
        let path = self.accounts_path();
        if !path.exists() {
            return Ok(vec![]);
        }
        let data = std::fs::read_to_string(&path)?;
        let file: AccountsFile = serde_json::from_str(&data)?;
        Ok(file.accounts)
    }

    pub fn save_accounts(&self, accounts: &[Account]) -> Result<()> {
        let file = AccountsFile {
            accounts: accounts.to_vec(),
        };
        let data = serde_json::to_string_pretty(&file)?;
        std::fs::write(self.accounts_path(), data)?;
        Ok(())
    }

    pub fn upsert_account(&self, account: Account) -> Result<()> {
        let mut accounts = self.load_accounts()?;
        if let Some(pos) = accounts.iter().position(|a| a.id == account.id) {
            accounts[pos] = account;
        } else {
            accounts.push(account);
        }
        self.save_accounts(&accounts)
    }

    pub fn get_account(&self, account_id: &str) -> Result<Option<Account>> {
        Ok(self.load_accounts()?.into_iter().find(|a| a.id == account_id))
    }

    pub fn remove_account(&self, account_id: &str) -> Result<bool> {
        let mut accounts = self.load_accounts()?;
        let before = accounts.len();
        accounts.retain(|a| a.id != account_id);
        if accounts.len() < before {
            self.save_accounts(&accounts)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
