use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

use crate::db::Database;

const API_BASE: &str = "https://api.netmon.app";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthState {
    pub authenticated: bool,
    pub user_id: Option<String>,
    pub email: Option<String>,
    pub plan: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    #[serde(rename = "userId")]
    _user_id: Option<String>,
}

pub struct AuthManager {
    db: Arc<Mutex<Database>>,
    device_info: DeviceInfo,
    pkce_verifier: Mutex<Option<String>>,
}

impl AuthManager {
    pub fn new(db: Arc<Mutex<Database>>) -> Arc<Self> {
        let device_info = {
            let db = db.lock().unwrap();
            db.get_or_create_device_info()
        };

        Arc::new(Self {
            db,
            device_info,
            pkce_verifier: Mutex::new(None),
        })
    }

    pub fn device_info(&self) -> &DeviceInfo {
        &self.device_info
    }

    pub fn get_auth_state(&self) -> AuthState {
        let db = self.db.lock().unwrap();
        match db.get_auth_tokens() {
            Some(tokens) => AuthState {
                authenticated: true,
                user_id: Some(tokens.user_id),
                email: Some(tokens.email),
                plan: Some(tokens.plan),
            },
            None => AuthState {
                authenticated: false,
                user_id: None,
                email: None,
                plan: None,
            },
        }
    }

    pub fn start_oauth(&self, provider: &str) -> Result<String, String> {
        // Generate PKCE code_verifier and code_challenge
        let verifier = generate_pkce_verifier();
        let challenge = generate_pkce_challenge(&verifier);

        // Store verifier for later exchange
        *self.pkce_verifier.lock().unwrap() = Some(verifier);

        let url = format!(
            "{}/auth/{}?code_challenge={}&device_id={}",
            API_BASE, provider, challenge, self.device_info.device_id
        );

        Ok(url)
    }

    pub fn handle_callback(&self, code: &str) -> Result<AuthState, String> {
        let verifier = self
            .pkce_verifier
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| "No PKCE verifier found".to_string())?;

        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(format!("{}/auth/token", API_BASE))
            .json(&serde_json::json!({
                "code": code,
                "code_verifier": verifier,
                "device_id": self.device_info.device_id,
            }))
            .send()
            .map_err(|e| format!("Token exchange failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(format!("Token exchange failed ({}): {}", status, body));
        }

        let token_resp: TokenResponse = resp
            .json()
            .map_err(|e| format!("Failed to parse token response: {}", e))?;

        self.store_tokens(&token_resp)?;
        Ok(self.get_auth_state())
    }

    pub fn login_email(&self, email: &str, password: &str) -> Result<AuthState, String> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(format!("{}/auth/login", API_BASE))
            .json(&serde_json::json!({
                "email": email,
                "password": password,
            }))
            .send()
            .map_err(|e| format!("Login failed: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(format!("Login failed: {}", body));
        }

        let token_resp: TokenResponse = resp
            .json()
            .map_err(|e| format!("Failed to parse login response: {}", e))?;

        self.store_tokens(&token_resp)?;
        Ok(self.get_auth_state())
    }

    pub fn register_email(&self, email: &str, password: &str) -> Result<AuthState, String> {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(format!("{}/auth/register", API_BASE))
            .json(&serde_json::json!({
                "email": email,
                "password": password,
            }))
            .send()
            .map_err(|e| format!("Registration failed: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(format!("Registration failed: {}", body));
        }

        let token_resp: TokenResponse = resp
            .json()
            .map_err(|e| format!("Failed to parse registration response: {}", e))?;

        self.store_tokens(&token_resp)?;
        Ok(self.get_auth_state())
    }

    pub fn refresh_token(&self) -> Result<String, String> {
        let db = self.db.lock().unwrap();
        let tokens = db
            .get_auth_tokens()
            .ok_or_else(|| "Not authenticated".to_string())?;
        drop(db);

        let client = reqwest::blocking::Client::new();
        let resp = client
            .post(format!("{}/auth/refresh", API_BASE))
            .json(&serde_json::json!({
                "refresh_token": tokens.refresh_token,
                "device_id": self.device_info.device_id,
            }))
            .send()
            .map_err(|e| format!("Token refresh failed: {}", e))?;

        if !resp.status().is_success() {
            return Err("Token refresh failed".to_string());
        }

        let token_resp: TokenResponse = resp
            .json()
            .map_err(|e| format!("Failed to parse refresh response: {}", e))?;

        let db = self.db.lock().unwrap();
        db.update_access_token(&token_resp.access_token)
            .map_err(|e| e.to_string())?;

        Ok(token_resp.access_token)
    }

    pub fn get_access_token(&self) -> Option<String> {
        let db = self.db.lock().unwrap();
        let tokens = db.get_auth_tokens()?;

        // Check if token is close to expiry (within 5 minutes)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        if tokens.expires_at - now < 300 {
            drop(db);
            // Try to refresh
            return self.refresh_token().ok();
        }

        Some(tokens.access_token)
    }

    pub fn logout(&self) -> Result<(), String> {
        let db = self.db.lock().unwrap();
        db.clear_auth_tokens().map_err(|e| e.to_string())
    }

    fn store_tokens(&self, resp: &TokenResponse) -> Result<(), String> {
        // Decode JWT to extract claims (we just need the payload, not full verification)
        let parts: Vec<&str> = resp.access_token.split('.').collect();
        if parts.len() != 3 {
            return Err("Invalid JWT format".to_string());
        }

        let payload_bytes = URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|e| format!("Failed to decode JWT payload: {}", e))?;
        let payload: serde_json::Value = serde_json::from_slice(&payload_bytes)
            .map_err(|e| format!("Failed to parse JWT payload: {}", e))?;

        let user_id = payload["sub"].as_str().unwrap_or("").to_string();
        let email = payload["email"].as_str().unwrap_or("").to_string();
        let plan = payload["plan"].as_str().unwrap_or("free").to_string();
        let exp = payload["exp"].as_i64().unwrap_or(0);

        let db = self.db.lock().unwrap();
        db.store_auth_tokens(
            &user_id,
            &email,
            &plan,
            &resp.access_token,
            resp.refresh_token.as_deref().unwrap_or(""),
            exp,
        )
        .map_err(|e| e.to_string())
    }
}

fn generate_pkce_verifier() -> String {
    let bytes: Vec<u8> = (0..32).map(|_| rand::random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

fn generate_pkce_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(hash)
}
