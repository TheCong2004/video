// SPDX-License-Identifier: AGPL-3.0-or-later
//! Native PKCE, verified Promax exchange and user-bound encrypted session storage.
//! No browser cookies, implicit-token callback or email-only bootstrap is used.
use crate::workflows::{WorkflowClient, WorkflowError};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Safe failures; upstream response bodies and tokens must never be displayed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AuthError {
    /// Invalid trusted endpoint or public key configuration.
    Configuration,
    /// Listener could not be bound.
    Callback,
    /// User cancelled or the five-minute callback expired.
    Cancelled,
    /// Invalid one-use callback or refused identity exchange.
    Rejected,
    /// Network/timeout, not a successful login.
    Unavailable,
    /// No native session, or rotated refresh could not be confirmed.
    SignedOut,
    /// Encrypted storage unavailable, corrupted or belongs to another OS user.
    Storage,
}

/// Display metadata only. Not a bearer credential.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub struct Account {
    pub user_id: String,
    pub tenant_id: String,
    pub roles: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Session {
    access: String,
    refresh: String,
    expires: i64,
    user: String,
    tenant: String,
    roles: Vec<String>,
    origin: String,
}
impl Session {
    fn account(&self) -> Account {
        Account {
            user_id: self.user.clone(),
            tenant_id: self.tenant.clone(),
            roles: self.roles.clone(),
        }
    }
}

/// Owns a single PKCE verifier and loopback listener. Not Debug/Clone by design.
pub struct LoginAttempt {
    listener: TcpListener,
    path: String,
    authority: String,
    verifier: String,
    authorize: String,
    deadline: Instant,
}
impl LoginAttempt {
    /// Create a one-use Google authorization URL. Public config, no service key.
    pub fn new(supabase_origin: &str, public_key: &str) -> Result<Self, AuthError> {
        validate_identity_config(supabase_origin, public_key)?;
        let listener = TcpListener::bind("127.0.0.1:47832")
            .or_else(|_| TcpListener::bind("127.0.0.1:47833"))
            .or_else(|_| TcpListener::bind("127.0.0.1:0"))
            .map_err(|_| AuthError::Callback)?;
        Self::from_listener(supabase_origin, listener)
    }
    fn from_listener(origin: &str, listener: TcpListener) -> Result<Self, AuthError> {
        listener
            .set_nonblocking(true)
            .map_err(|_| AuthError::Callback)?;
        let authority = format!(
            "127.0.0.1:{}",
            listener
                .local_addr()
                .map_err(|_| AuthError::Callback)?
                .port()
        );
        let verifier = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        let path = format!(
            "/auth/callback/{}",
            URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
        );
        let redirect = format!("http://{authority}{path}");
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut authorize = url::Url::parse(&format!(
            "{}/auth/v1/authorize",
            origin.trim_end_matches('/')
        ))
        .map_err(|_| AuthError::Configuration)?;
        authorize
            .query_pairs_mut()
            .append_pair("provider", "google")
            .append_pair("redirect_to", &redirect)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "s256");
        Ok(Self {
            listener,
            path,
            authority,
            verifier,
            authorize: authorize.into(),
            deadline: Instant::now() + Duration::from_secs(300),
        })
    }
    /// Give this URL only to the chosen browser profile; never log it.
    pub fn authorize_url(&self) -> &str {
        &self.authorize
    }
    /// Wait on a bounded loopback listener. Random path and PKCE bind this flow.
    /// Only a GET auth-code callback is accepted; no fragment tokens or POST.
    pub fn wait(self, cancelled: &AtomicBool) -> Result<LoginCode, AuthError> {
        while Instant::now() < self.deadline && !cancelled.load(Ordering::Acquire) {
            let (mut stream, _) = match self.listener.accept() {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(40));
                    continue;
                }
                Err(_) => return Err(AuthError::Callback),
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            let mut bytes = Vec::new();
            let mut chunk = [0; 1024];
            while bytes.len() < 8192 && !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                }
            }
            let parsed = parse_callback(&bytes, &self.authority, &self.path);
            let status = if parsed.is_ok() {
                "200 OK"
            } else {
                "400 Bad Request"
            };
            // Fixed text only: neither auth code nor upstream error is reflected.
            let body = "<!doctype html><meta charset=utf-8><title>Concat</title><p>Return to Concat to check the sign-in result. You may close this tab.</p>";
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            if let Ok(code) = parsed {
                if cancelled.load(Ordering::Acquire) {
                    return Err(AuthError::Cancelled);
                }
                let code = code.ok_or(AuthError::Rejected)?;
                return Ok(LoginCode {
                    code,
                    verifier: self.verifier,
                });
            }
        }
        Err(AuthError::Cancelled)
    }
}
/// A one-use code bound to its verifier. Intentionally opaque to the UI.
pub struct LoginCode {
    code: String,
    verifier: String,
}

fn parse_callback(bytes: &[u8], authority: &str, path: &str) -> Result<Option<String>, AuthError> {
    if bytes.len() > 8192 || !bytes.ends_with(b"\r\n\r\n") {
        return Err(AuthError::Rejected);
    }
    let request = std::str::from_utf8(bytes).map_err(|_| AuthError::Rejected)?;
    let mut lines = request.split("\r\n");
    let mut start = lines.next().unwrap_or("").split_whitespace();
    if start.next() != Some("GET") {
        return Err(AuthError::Rejected);
    }
    let target = start.next().ok_or(AuthError::Rejected)?;
    if !target.starts_with('/')
        || target.starts_with("//")
        || start.next() != Some("HTTP/1.1")
        || start.next().is_some()
    {
        return Err(AuthError::Rejected);
    }
    let mut host = None;
    for line in lines.filter(|s| !s.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(AuthError::Rejected)?;
        if name.eq_ignore_ascii_case("host") {
            if host.replace(value.trim()).is_some() {
                return Err(AuthError::Rejected);
            }
        }
        if name.eq_ignore_ascii_case("origin")
            || name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && value.trim() != "0")
        {
            return Err(AuthError::Rejected);
        }
    }
    if host != Some(authority) {
        return Err(AuthError::Rejected);
    }
    let url =
        url::Url::parse(&format!("http://{authority}{target}")).map_err(|_| AuthError::Rejected)?;
    if url.path() != path || url.fragment().is_some() {
        return Err(AuthError::Rejected);
    }
    let codes: Vec<_> = url
        .query_pairs()
        .filter(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .collect();
    if url
        .query_pairs()
        .any(|(k, _)| k == "access_token" || k == "refresh_token")
    {
        return Err(AuthError::Rejected);
    }
    if url.query_pairs().any(|(k, _)| k == "error") {
        return if codes.is_empty() {
            Ok(None)
        } else {
            Err(AuthError::Rejected)
        };
    }
    if codes.len() != 1 {
        return Err(AuthError::Rejected);
    }
    let code = codes.into_iter().next().ok_or(AuthError::Rejected)?;
    if code.is_empty() || code.len() > 2048 || code.chars().any(char::is_control) {
        return Err(AuthError::Rejected);
    }
    Ok(Some(code))
}

fn validate_identity_config(origin: &str, key: &str) -> Result<(), AuthError> {
    let url = url::Url::parse(origin).map_err(|_| AuthError::Configuration)?;
    if url.scheme() != "https"
        || url.host().is_none()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || key.is_empty()
        || key.len() > 4096
        || !key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._".contains(&c))
    {
        return Err(AuthError::Configuration);
    }
    // Never accept Supabase server/admin credentials as a desktop public key.
    if !key.starts_with("sb_publishable_") {
        return Err(AuthError::Configuration);
    }
    Ok(())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs() as i64)
        .unwrap_or(i64::MAX)
}
fn safe_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 16384
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-._~+/=".contains(&c))
}
fn field(value: &Value, key: &str) -> Result<String, AuthError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or(AuthError::Rejected)
}
fn parse_session(data: &Value, origin: &str) -> Result<Session, AuthError> {
    let access = data
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|s| safe_token(s))
        .ok_or(AuthError::Rejected)?
        .to_owned();
    let refresh = data
        .get("refreshToken")
        .and_then(Value::as_str)
        .filter(|s| safe_token(s))
        .ok_or(AuthError::Rejected)?
        .to_owned();
    let expires = chrono::DateTime::parse_from_rfc3339(
        data.get("expiresAt")
            .and_then(Value::as_str)
            .ok_or(AuthError::Rejected)?,
    )
    .map_err(|_| AuthError::Rejected)?
    .timestamp();
    if expires <= now() + 10 || expires > now() + 32 * 86400 {
        return Err(AuthError::Rejected);
    }
    let principal = data.get("principal").ok_or(AuthError::Rejected)?;
    let roles = principal
        .get("roles")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty() && v.len() <= 3)
        .ok_or(AuthError::Rejected)?;
    let roles = roles
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| matches!(*s, "owner" | "admin" | "member"))
                .map(str::to_owned)
                .ok_or(AuthError::Rejected)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Session {
        access,
        refresh,
        expires,
        user: field(principal, "userId")?,
        tenant: field(principal, "tenantId")?,
        roles,
        origin: origin.into(),
    })
}

fn request(
    origin: &str,
    path: &str,
    key: Option<&str>,
    bearer: Option<&str>,
    body: &Value,
) -> Result<Value, AuthError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(12))
        .redirects(0)
        .build();
    let mut req = agent
        .post(&format!("{origin}{path}"))
        .set("Content-Type", "application/json")
        .set("Accept", "application/json");
    if let Some(key) = key {
        req = req.set("apikey", key);
    }
    if let Some(token) = bearer {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let response = match req.send_bytes(&serde_json::to_vec(body).map_err(|_| AuthError::Rejected)?)
    {
        Ok(r) if r.status() == 200 => r,
        Err(ureq::Error::Status(400 | 401 | 403, _)) => return Err(AuthError::Rejected),
        _ => return Err(AuthError::Unavailable),
    };
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(131073)
        .read_to_end(&mut bytes)
        .map_err(|_| AuthError::Unavailable)?;
    if bytes.len() > 131072 {
        return Err(AuthError::Rejected);
    }
    serde_json::from_slice(&bytes).map_err(|_| AuthError::Rejected)
}

enum State {
    Unloaded,
    SignedOut,
    SignedIn(Session),
}
/// Shared, serialized session owner; refresh is done off the UI thread.
pub struct AuthManager {
    origin: String,
    path: PathBuf,
    state: Mutex<State>,
}
impl AuthManager {
    /// Session is bound to the normalized Promax origin and this OS account.
    pub fn new(origin: &str, config: &Path) -> Result<Self, AuthError> {
        WorkflowClient::new(origin, Some("validation-only".into()))
            .map_err(|_| AuthError::Configuration)?;
        let origin = url::Url::parse(origin.trim())
            .map_err(|_| AuthError::Configuration)?
            .as_str()
            .trim_end_matches('/')
            .to_owned();
        let hash = format!("{:x}", Sha256::digest(origin.as_bytes()));
        Ok(Self {
            origin,
            path: config.join("studio").join(format!("session-{hash}.dpapi")),
            state: Mutex::new(State::Unloaded),
        })
    }
    /// Finish a one-use code exchange, then verify identity with Promax.
    /// Cancellation is checked before publishing or persisting a session.
    pub fn complete_login(
        &self,
        identity_origin: &str,
        key: &str,
        code: LoginCode,
        cancelled: &AtomicBool,
    ) -> Result<Account, AuthError> {
        validate_identity_config(identity_origin, key)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(AuthError::Cancelled);
        }
        let identity = request(
            identity_origin.trim_end_matches('/'),
            "/auth/v1/token?grant_type=pkce",
            Some(key),
            None,
            &json!({"auth_code":code.code,"code_verifier":code.verifier}),
        )?;
        let access = identity
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|s| safe_token(s))
            .ok_or(AuthError::Rejected)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(AuthError::Cancelled);
        }
        let result = request(
            &self.origin,
            "/api/v1/auth/supabase/exchange",
            None,
            None,
            &json!({"accessToken":access,"deviceType":"desktop"}),
        )?;
        if result.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(AuthError::Rejected);
        }
        let session = parse_session(result.get("data").ok_or(AuthError::Rejected)?, &self.origin)?;
        let mut state = self.state.lock().map_err(|_| AuthError::Storage)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(AuthError::Cancelled);
        }
        let account = session.account();
        persist(&self.path, &session)?;
        *state = State::SignedIn(session);
        Ok(account)
    }
    fn load(&self, state: &mut State) -> Result<(), AuthError> {
        if matches!(state, State::Unloaded) {
            *state = State::SignedOut;
            if self.path.is_file() {
                let bytes = read_bounded(&self.path)?;
                let plain = unprotect(&bytes, &self.origin)?;
                let session: Session =
                    serde_json::from_slice(&plain).map_err(|_| AuthError::Storage)?;
                if session.origin != self.origin
                    || !safe_token(&session.access)
                    || !safe_token(&session.refresh)
                {
                    return Err(AuthError::Storage);
                }
                *state = State::SignedIn(session);
            }
        }
        Ok(())
    }
    /// Restore/refresh a native session; never silently bootstraps by email.
    pub fn account(&self) -> Result<Account, AuthError> {
        {
            let mut state = self.state.lock().map_err(|_| AuthError::Storage)?;
            self.load(&mut state)?;
        }
        let client = self.client().map_err(|e| match e {
            WorkflowError::Authentication => AuthError::SignedOut,
            _ => AuthError::Unavailable,
        })?;
        // A decrypted cached token is not proof it is still accepted by Promax.
        let verified = client.get("/api/v1/auth/me").map_err(|e| match e {
            WorkflowError::Authentication | WorkflowError::Forbidden => AuthError::SignedOut,
            _ => AuthError::Unavailable,
        })?;
        let state = self.state.lock().map_err(|_| AuthError::Storage)?;
        match &*state {
            State::SignedIn(s)
                if field(&verified, "userId")? == s.user
                    && field(&verified, "tenantId")? == s.tenant =>
            {
                Ok(s.account())
            }
            State::SignedIn(_) => Err(AuthError::Rejected),
            _ => Err(AuthError::SignedOut),
        }
    }
    /// Returns a client with a fresh token. Only one refresh can rotate at once.
    pub fn client(&self) -> Result<WorkflowClient, WorkflowError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| WorkflowError::Authentication)?;
        self.load(&mut state)
            .map_err(|_| WorkflowError::Authentication)?;
        if let State::SignedIn(session) = &*state {
            if session.expires <= now() + 60 {
                let result = request(
                    &self.origin,
                    "/api/v1/auth/refresh",
                    None,
                    None,
                    &json!({"refreshToken":session.refresh}),
                );
                let refreshed = result.and_then(|v| {
                    if v.get("ok").and_then(Value::as_bool) != Some(true) {
                        return Err(AuthError::Rejected);
                    }
                    let next =
                        parse_session(v.get("data").ok_or(AuthError::Rejected)?, &self.origin)?;
                    if next.user != session.user || next.tenant != session.tenant {
                        return Err(AuthError::Rejected);
                    }
                    Ok(next)
                });
                // Any ambiguous rotating refresh invalidates local state. Never replay.
                *state = State::SignedOut;
                let _ = fs::remove_file(&self.path);
                let next = refreshed.map_err(|_| WorkflowError::Authentication)?;
                persist(&self.path, &next).map_err(|_| WorkflowError::Authentication)?;
                *state = State::SignedIn(next);
            }
        }
        match &*state {
            State::SignedIn(session) => {
                WorkflowClient::new(&self.origin, Some(session.access.clone()))
            }
            _ => Err(WorkflowError::Authentication),
        }
    }
    /// Clear local session first, then request server revocation exactly once.
    /// False means remote revocation was not confirmed; local logout still occurred.
    pub fn logout(&self) -> Result<bool, AuthError> {
        let mut state = self.state.lock().map_err(|_| AuthError::Storage)?;
        let loaded = self.load(&mut state).is_ok();
        let previous = std::mem::replace(&mut *state, State::SignedOut);
        let removed = if self.path.exists() {
            fs::remove_file(&self.path).is_ok()
        } else {
            true
        };
        drop(state);
        let remote = match previous {
            State::SignedIn(session) => Ok(request(
                &self.origin,
                "/api/v1/auth/logout",
                None,
                Some(&session.access),
                &json!({}),
            )
            .map(|v| v.get("ok").and_then(Value::as_bool) == Some(true))
            .unwrap_or(false)),
            _ => Ok(loaded),
        };
        if !removed {
            return Err(AuthError::Storage);
        }
        remote
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, AuthError> {
    let file = fs::File::open(path).map_err(|_| AuthError::Storage)?;
    let mut bytes = Vec::new();
    file.take(131073)
        .read_to_end(&mut bytes)
        .map_err(|_| AuthError::Storage)?;
    if bytes.len() > 131072 {
        return Err(AuthError::Storage);
    }
    Ok(bytes)
}
fn persist(path: &Path, session: &Session) -> Result<(), AuthError> {
    let plain = serde_json::to_vec(session).map_err(|_| AuthError::Storage)?;
    let encrypted = protect(&plain, &session.origin)?;
    fs::create_dir_all(path.parent().ok_or(AuthError::Storage)?).map_err(|_| AuthError::Storage)?;
    let temporary = path.with_extension(format!(
        "{}.tmp",
        URL_SAFE_NO_PAD.encode(rand::random::<[u8; 16]>())
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| AuthError::Storage)?;
        file.write_all(&encrypted)
            .and_then(|_| file.sync_all())
            .map_err(|_| AuthError::Storage)?;
        fs::rename(&temporary, path).map_err(|_| AuthError::Storage)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn crypt(bytes: &[u8], origin: &str, encrypt: bool) -> Result<Vec<u8>, AuthError> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
    };
    if bytes.is_empty() || bytes.len() > 131072 {
        return Err(AuthError::Storage);
    }
    let entropy = Sha256::digest(format!("Concat Studio session v1:{origin}").as_bytes());
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    // SAFETY: input/entropy buffers live throughout the call. DPAPI allocates the
    // output with LocalAlloc, which is released exactly once with LocalFree.
    // No machine-wide flag: decryption is bound to the current Windows user.
    unsafe {
        let ok = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(AuthError::Storage);
        }
        if output.pbData.is_null() || output.cbData == 0 || output.cbData > 131072 {
            if !output.pbData.is_null() {
                LocalFree(output.pbData.cast());
            }
            return Err(AuthError::Storage);
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}
#[cfg(windows)]
fn protect(bytes: &[u8], origin: &str) -> Result<Vec<u8>, AuthError> {
    crypt(bytes, origin, true)
}
#[cfg(windows)]
fn unprotect(bytes: &[u8], origin: &str) -> Result<Vec<u8>, AuthError> {
    crypt(bytes, origin, false)
}
// Never fall back to plaintext on an unsupported platform.
#[cfg(not(windows))]
fn protect(_: &[u8], _: &str) -> Result<Vec<u8>, AuthError> {
    Err(AuthError::Storage)
}
#[cfg(not(windows))]
fn unprotect(_: &[u8], _: &str) -> Result<Vec<u8>, AuthError> {
    Err(AuthError::Storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn token_response(origin: &str, user: &str, expires: i64) -> Session {
        Session {
            access: "fixture-access".into(),
            refresh: "fixture-refresh".into(),
            expires,
            user: user.into(),
            tenant: "fixture-tenant".into(),
            roles: vec!["member".into()],
            origin: origin.into(),
        }
    }
    fn response(user: &str) -> Value {
        json!({"ok":true,"data":{"accessToken":"rotated-fixture-access","refreshToken":"rotated-fixture-refresh","expiresAt":chrono::DateTime::from_timestamp(now()+3600,0).unwrap().to_rfc3339(),"principal":{"userId":user,"tenantId":"fixture-tenant","roles":["member"]}}})
    }
    fn fixture(reply: Value, status: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 2048];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                assert_ne!(n, 0);
                bytes.extend_from_slice(&chunk[..n]);
                let s = String::from_utf8_lossy(&bytes);
                if let Some(end) = s.find("\r\n\r\n") {
                    let length = s[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let body = reply.to_string();
            write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            String::from_utf8(bytes).unwrap()
        });
        (origin, worker)
    }
    fn temporary() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "concat-auth-fixture-{}-{}",
            std::process::id(),
            URL_SAFE_NO_PAD.encode(rand::random::<[u8; 16]>())
        ));
        fs::create_dir(&root).unwrap();
        root
    }
    #[test]
    fn legitimate_denial_finishes_instead_of_waiting_five_minutes() {
        let denied=b"GET /auth/callback/nonce?error=access_denied HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n";
        assert_eq!(
            parse_callback(denied, "127.0.0.1:1234", "/auth/callback/nonce"),
            Ok(None)
        );
    }
    #[test]
    fn session_envelope_requires_valid_expiry_principal_and_safe_tokens() {
        let good = response("fixture-user");
        assert!(parse_session(&good["data"], "https://promax.example").is_ok());
        for changed in [
            json!({"accessToken":"unsafe\r\nheader"}),
            json!({"refreshToken":""}),
            json!({"expiresAt":"1970-01-01T00:00:00Z"}),
            json!({"principal":{"userId":"u","tenantId":"t","roles":["superadmin"]}}),
        ] {
            let mut bad = good["data"].clone();
            for (k, v) in changed.as_object().unwrap() {
                bad[k] = v.clone();
            }
            assert!(parse_session(&bad, "https://promax.example").is_err());
        }
    }
    #[cfg(windows)]
    #[test]
    fn encrypted_session_restores_only_after_server_verifies_identity() {
        let (origin, worker) = fixture(
            json!({"ok":true,"data":{"userId":"fixture-user","tenantId":"fixture-tenant","roles":["member"]}}),
            "200 OK",
        );
        let root = temporary();
        let manager = AuthManager::new(&origin, &root).unwrap();
        persist(
            &manager.path,
            &token_response(&manager.origin, "fixture-user", now() + 3600),
        )
        .unwrap();
        let bytes = fs::read(&manager.path).unwrap();
        assert!(!bytes.windows(14).any(|v| v == b"fixture-access"));
        assert_eq!(manager.account().unwrap().user_id, "fixture-user");
        let request = worker.join().unwrap();
        assert!(request.starts_with("GET /api/v1/auth/me "));
        assert!(request.contains("Bearer fixture-access"));
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn restoring_revoked_or_different_identity_never_claims_ready() {
        for (reply, status) in [
            (
                json!({"ok":true,"data":{"userId":"other-user","tenantId":"fixture-tenant"}}),
                "200 OK",
            ),
            (json!({"secret":"not-for-display"}), "401 Unauthorized"),
        ] {
            let (origin, worker) = fixture(reply, status);
            let root = temporary();
            let manager = AuthManager::new(&origin, &root).unwrap();
            persist(
                &manager.path,
                &token_response(&origin, "fixture-user", now() + 3600),
            )
            .unwrap();
            assert!(matches!(
                manager.account(),
                Err(AuthError::Rejected | AuthError::SignedOut)
            ));
            worker.join().unwrap();
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[cfg(windows)]
    #[test]
    fn concurrent_refresh_rotates_once_and_persists_new_session() {
        let (origin, worker) = fixture(response("fixture-user"), "200 OK");
        let root = temporary();
        let manager = std::sync::Arc::new(AuthManager::new(&origin, &root).unwrap());
        persist(
            &manager.path,
            &token_response(&origin, "fixture-user", now() + 30),
        )
        .unwrap();
        let threads: Vec<_> = (0..5)
            .map(|_| {
                let manager = manager.clone();
                std::thread::spawn(move || manager.client().is_ok())
            })
            .collect();
        for thread in threads {
            assert!(thread.join().unwrap());
        }
        let req = worker.join().unwrap();
        assert!(req.starts_with("POST /api/v1/auth/refresh "));
        assert!(req.contains("fixture-refresh"));
        let plain = unprotect(&read_bounded(&manager.path).unwrap(), &origin).unwrap();
        let saved: Session = serde_json::from_slice(&plain).unwrap();
        assert_eq!(saved.access, "rotated-fixture-access");
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn refresh_cannot_switch_tenant_or_replay_after_ambiguous_result() {
        for reply in [
            response("other-user"),
            json!({"ok":false,"secret":"not-for-display"}),
        ] {
            let (origin, worker) = fixture(reply, "200 OK");
            let root = temporary();
            let manager = AuthManager::new(&origin, &root).unwrap();
            persist(
                &manager.path,
                &token_response(&origin, "fixture-user", now() + 30),
            )
            .unwrap();
            assert!(matches!(
                manager.client(),
                Err(WorkflowError::Authentication)
            ));
            worker.join().unwrap();
            assert!(!manager.path.exists());
            assert!(matches!(
                manager.client(),
                Err(WorkflowError::Authentication)
            ));
            fs::remove_dir_all(root).unwrap();
        }
    }
    #[cfg(windows)]
    #[test]
    fn logout_clears_local_state_when_remote_revocation_is_unconfirmed() {
        let (origin, worker) = fixture(json!({"secret":"not-for-display"}), "500 Failed");
        let root = temporary();
        let manager = AuthManager::new(&origin, &root).unwrap();
        persist(
            &manager.path,
            &token_response(&origin, "fixture-user", now() + 3600),
        )
        .unwrap();
        assert_eq!(manager.logout().unwrap(), false);
        let req = worker.join().unwrap();
        assert!(req.starts_with("POST /api/v1/auth/logout "));
        assert!(!manager.path.exists());
        assert!(matches!(
            manager.client(),
            Err(WorkflowError::Authentication)
        ));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn callback_is_bound_to_host_random_path_and_single_code() {
        let valid =
            b"GET /auth/callback/nonce?code=fixture-code HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n";
        assert_eq!(
            parse_callback(valid, "127.0.0.1:1234", "/auth/callback/nonce").unwrap(),
            Some("fixture-code".into())
        );
        for request in [
            "GET /auth/callback/wrong?code=a HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n",
            "GET /auth/callback/nonce?code=a&code=b HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n",
            "GET /auth/callback/nonce?access_token=a HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n",
            "GET /auth/callback/nonce?code=a HTTP/1.1\r\nHost: attacker.invalid\r\n\r\n",
            "GET /auth/callback/nonce?code=a HTTP/1.1\r\nHost: 127.0.0.1:1234\r\nOrigin: https://attacker.invalid\r\n\r\n",
            "POST /auth/callback/nonce?code=a HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n",
        ] {
            assert_eq!(
                parse_callback(request.as_bytes(), "127.0.0.1:1234", "/auth/callback/nonce"),
                Err(AuthError::Rejected)
            );
        }
    }
    #[test]
    fn s256_matches_rfc7636_vector_and_verifier_is_not_in_url() {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(
            b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
        ));
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let attempt = LoginAttempt::from_listener("https://identity.example", listener).unwrap();
        assert!(!attempt.authorize_url().contains(&attempt.verifier));
        assert!(
            attempt
                .authorize_url()
                .contains("code_challenge_method=s256")
        );
        assert_eq!(attempt.verifier.len(), 43);
    }
    #[test]
    fn cancellation_closes_listener_without_login() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let attempt = LoginAttempt::from_listener("https://identity.example", listener).unwrap();
        assert!(matches!(
            attempt.wait(&AtomicBool::new(true)),
            Err(AuthError::Cancelled)
        ));
        assert!(TcpListener::bind(address).is_ok());
    }
    #[test]
    fn rejects_private_key_and_insecure_identity_config() {
        assert_eq!(
            validate_identity_config("https://identity.example", "sb_secret_private"),
            Err(AuthError::Configuration)
        );
        assert_eq!(
            validate_identity_config("http://127.0.0.1", "sb_publishable_fixture"),
            Err(AuthError::Configuration)
        );
        assert_eq!(
            validate_identity_config("https://identity.example?token=x", "sb_publishable_fixture"),
            Err(AuthError::Configuration)
        );
    }
    #[cfg(windows)]
    #[test]
    fn dpapi_ciphertext_is_user_and_origin_bound() {
        let data = b"synthetic-session-not-a-real-token";
        let encrypted = protect(data, "https://promax.example").unwrap();
        assert!(!encrypted.windows(data.len()).any(|v| v == data));
        assert_eq!(
            unprotect(&encrypted, "https://promax.example").unwrap(),
            data
        );
        assert_eq!(
            unprotect(&encrypted, "https://other.example"),
            Err(AuthError::Storage)
        );
        assert_eq!(
            unprotect(b"corrupted", "https://promax.example"),
            Err(AuthError::Storage)
        );
    }
}
