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
    // In-memory store — single source of truth; also persisted to disk.
    // The Mutex serializes all reads and writes so concurrent tasks can't corrupt the file.
    accounts: Mutex<Vec<Account>>,
    pub pending_auths: Mutex<HashMap<String, PendingAuth>>,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("email-mcp");
        std::fs::create_dir_all(&config_dir)?;

        let accounts_path = config_dir.join("accounts.json");
        let accounts = if accounts_path.exists() {
            let data = std::fs::read_to_string(&accounts_path)?;
            serde_json::from_str::<AccountsFile>(&data)
                .map(|f| f.accounts)
                .unwrap_or_default()
        } else {
            vec![]
        };

        Ok(Self {
            config_dir,
            accounts: Mutex::new(accounts),
            pending_auths: Mutex::new(HashMap::new()),
        })
    }

    fn accounts_path(&self) -> PathBuf {
        self.config_dir.join("accounts.json")
    }

    fn persist(accounts: &[Account], path: &PathBuf) -> Result<()> {
        let data = serde_json::to_string_pretty(&AccountsFile {
            accounts: accounts.to_vec(),
        })?;
        std::fs::write(path, data)?;
        Ok(())
    }

    pub fn load_accounts(&self) -> Result<Vec<Account>> {
        Ok(self.accounts.lock().unwrap().clone())
    }

    pub fn upsert_account(&self, account: Account) -> Result<()> {
        let mut accounts = self.accounts.lock().unwrap();
        if let Some(pos) = accounts.iter().position(|a| a.id == account.id) {
            accounts[pos] = account;
        } else {
            accounts.push(account);
        }
        Self::persist(&accounts, &self.accounts_path())
    }

    pub fn get_account(&self, account_id: &str) -> Result<Option<Account>> {
        Ok(self
            .accounts
            .lock()
            .unwrap()
            .iter()
            .find(|a| a.id == account_id)
            .cloned())
    }

    pub fn remove_account(&self, account_id: &str) -> Result<bool> {
        let mut accounts = self.accounts.lock().unwrap();
        let before = accounts.len();
        accounts.retain(|a| a.id != account_id);
        if accounts.len() < before {
            Self::persist(&accounts, &self.accounts_path())?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
