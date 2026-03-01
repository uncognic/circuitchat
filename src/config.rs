use rpassword::prompt_password;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub identity: IdentityConfig,
    pub history: HistoryConfig,
    #[serde(default)]
    pub time: TimeConfig,
    #[serde(default)]
    pub auth: AuthConfig,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthConfig {
    pub enabled: bool,
    pub password: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct IdentityConfig {
    pub persist: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryConfig {
    pub save: bool,
    pub passphrase: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TimeConfig {
    #[serde(rename = "24h")]
    pub hour24: bool,
    pub local: bool,
}
impl Default for AuthConfig {
    fn default() -> Self {
        AuthConfig {
            enabled: false,
            password: String::new(),
        }
    }
}
impl Default for Config {
    fn default() -> Self {
        Config {
            identity: IdentityConfig { persist: false },
            history: HistoryConfig {
                save: false,
                passphrase: String::new(),
            },
            time: TimeConfig {
                hour24: true,
                local: false,
            },
            auth: AuthConfig {
                enabled: false,
                password: String::new(),
            },
        }
    }
}



pub fn config_path() -> Result<PathBuf, Box<dyn Error>> {
    let exe_dir = std::env::current_exe()?
        .parent()
        .ok_or("could not determine exe directory")?
        .to_path_buf();
    Ok(exe_dir.join("circuitchat.toml"))
}

pub fn load_or_create() -> Result<Config, Box<dyn Error>> {
    let path = config_path()?;

    if path.exists() {
        let contents = std::fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&contents)?;
        let raw: toml::Value = toml::from_str(&contents)?;
        if config.history.save && !config.identity.persist {
            eprintln!("warning: history.save = true has no effect without identity.persist = true");
        }

        let mut updated = false;

        if raw.get("time").is_none() {
            updated = true;
        }

        if raw.get("auth").is_none() {
            updated = true;
        }

        if updated {
            let contents = toml::to_string_pretty(&config)?;
            std::fs::write(&path, contents)?;
            println!("updated config with new fields at {}", path.display());
        }


        Ok(config)
    } else {
        let config = Config::default();
        let contents = toml::to_string_pretty(&config)?;
        std::fs::write(&path, contents)?;
        println!("created default config at {}", path.display());
        Ok(config)
    }
}

pub fn resolve_passphrase(config: &Config) -> Result<Option<String>, Box<dyn Error>> {
    if !config.identity.persist {
        return Ok(None);
    }

    if !config.history.passphrase.is_empty() {
        return Ok(Some(config.history.passphrase.clone()));
    }

    let db_path = crate::storage::db_path()?;
    let first_run = !db_path.exists();

    let passphrase = prompt_password("enter passphrase: ")?;
    if passphrase.is_empty() {
        return Err("passphrase cannot be empty when persist is enabled".into());
    }

    if first_run {
        let confirm = prompt_password("confirm passphrase: ")?;
        if passphrase != confirm {
            return Err("passphrases do not match".into());
        }
    }

    Ok(Some(passphrase))
}
pub fn resolve_auth_password(config: &Config) -> Result<Option<String>, Box<dyn Error>> {
    if !config.auth.enabled {
        return Ok(None);
    }
    if !config.auth.password.is_empty() {
        return Ok(Some(config.auth.password.clone()));
    }
    let password = rpassword::prompt_password("enter session password: ")?;
    if password.is_empty() {
        return Err("session password cannot be empty when auth is enabled".into());
    }
    Ok(Some(password))
}
