//! The local configuration center.
//!
//! The configuration center is a small HTTP server bound to `127.0.0.1` on
//! a random port. It serves a single page that lists, creates, edits,
//! deletes and activates provider profiles.
//!
//! Security properties:
//!
//! * The server binds only to the loopback interface.
//! * Every mutating request must carry the instance's bearer token in an
//!   `Authorization` header, and must come from the same origin.
//! * The token never appears in a URL, in the page, or in the state file
//!   that other processes read.
//! * Responses are marked `no-store` and carry a restrictive
//!   `Content-Security-Policy`.
//! * The server stops itself after an idle period.

mod page;
mod state;

pub use page::render_page;
pub use state::ConfigCenterState;
pub use state::ConfigCenterStateError;
pub use state::ConfigCenterStatePath;
pub use state::ServerInstance;

use super::ProviderProfile;
use super::ProviderProfileRepository;
use super::ProviderProfileStoreError;
use super::ProviderSecretStore;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;
use tiny_http::Header;
use tiny_http::Method;
use tiny_http::Request;
use tiny_http::Response;
use tiny_http::Server;
use tiny_http::StatusCode;

/// The response type the configuration center builds.
type HttpResponse = tiny_http::Response<std::io::Cursor<Vec<u8>>>;

/// Maximum size of a request body, in bytes.
const MAX_BODY_BYTES: usize = 64 * 1024;

/// How long the server stays alive without any request.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(900);

/// Name of the cookie that carries the page's session token.
const SESSION_COOKIE_NAME: &str = "codex_config_session";

/// Errors produced by the configuration center.
#[derive(Debug)]
pub enum ConfigCenterError {
    /// The server could not be started.
    Io(io::Error),
    /// The state file could not be read or written.
    State(ConfigCenterStateError),
    /// The request was rejected.
    Rejected(String),
}

impl std::fmt::Display for ConfigCenterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "configuration center I/O error: {err}"),
            Self::State(err) => write!(f, "configuration center state error: {err}"),
            Self::Rejected(message) => write!(f, "request rejected: {message}"),
        }
    }
}

impl std::error::Error for ConfigCenterError {}

impl From<io::Error> for ConfigCenterError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<ConfigCenterStateError> for ConfigCenterError {
    fn from(err: ConfigCenterStateError) -> Self {
        Self::State(err)
    }
}

/// A running configuration center.
pub struct ConfigCenter {
    /// The HTTP server.
    server: Arc<Server>,
    /// The port the server listens on.
    port: u16,
    /// The bearer token required for mutating requests.
    bearer_token: String,
    /// The session token handed to the page as an HttpOnly cookie.
    ///
    /// It never appears in the page body, so a script in the page cannot
    /// read it. The bearer token remains the out-of-band credential for the
    /// CLI that owns the server.
    session_token: String,
    /// The CSRF token embedded in the page.
    csrf_token: String,
    /// The profile repository.
    repository: ProviderProfileRepository,
    /// The credential store.
    secret_store: ProviderSecretStore,
    /// The moment of the last authenticated request.
    last_activity: Arc<std::sync::Mutex<Instant>>,
    /// How long the server stays alive without any request.
    idle_timeout: Duration,
    /// Number of requests served, used by tests.
    request_count: Arc<AtomicU64>,
    /// Set when a shutdown has been requested.
    shutdown_requested: Arc<std::sync::atomic::AtomicBool>,
}

impl ConfigCenter {
    /// Starts a configuration center bound to a random loopback port.
    pub fn start(
        code_home: &std::path::Path,
        idle_timeout: Duration,
    ) -> Result<Self, ConfigCenterError> {
        let idle_timeout = idle_timeout.max(Duration::from_secs(1));
        let server = Server::http("127.0.0.1:0")
            .map_err(|err| ConfigCenterError::Io(io::Error::other(err)))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|addr| addr.port())
            .ok_or_else(|| {
                ConfigCenterError::Io(io::Error::other("server bound to a non-IP address"))
            })?;

        let bearer_token = random_token();
        let session_token = random_token();
        let csrf_token = random_token();

        Ok(Self {
            server: Arc::new(server),
            port,
            bearer_token,
            session_token,
            csrf_token,
            repository: ProviderProfileRepository::new(code_home),
            secret_store: ProviderSecretStore::new(code_home),
            last_activity: Arc::new(std::sync::Mutex::new(Instant::now())),
            idle_timeout,
            request_count: Arc::new(AtomicU64::new(0)),
            shutdown_requested: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }

    /// Returns the port the server listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the current document revision of the profile store.
    ///
    /// A write passes this back to the repository as the expected revision,
    /// so a concurrent writer is detected instead of being overwritten.
    fn current_revision(&self) -> Result<u64, ConfigCenterError> {
        self.repository
            .document_revision()
            .map_err(|err| ConfigCenterError::Io(std::io::Error::other(err)))
    }

    /// Returns the URL of the configuration page.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    /// Returns the bearer token.
    ///
    /// The token is written to the state file so that the CLI that started
    /// the server can talk to it, but it is never rendered into the page.
    pub fn bearer_token(&self) -> &str {
        &self.bearer_token
    }

    /// Returns the CSRF token embedded in the page.
    pub fn csrf_token(&self) -> &str {
        &self.csrf_token
    }

    /// Returns the session token handed to the page as a cookie.
    pub fn session_token(&self) -> &str {
        &self.session_token
    }

    /// Returns the number of requests served.
    pub fn request_count(&self) -> u64 {
        self.request_count.load(Ordering::Relaxed)
    }

    /// Returns whether a shutdown has been requested.
    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::Relaxed)
    }

    /// Serves requests until the idle timeout elapses.
    ///
    /// Returns when the server stops.
    pub fn serve_until_idle(&self) -> Result<(), ConfigCenterError> {
        loop {
            if self.shutdown_requested() {
                return Ok(());
            }
            let server = Arc::clone(&self.server);
            match server.recv_timeout(Duration::from_millis(250)) {
                Ok(Some(request)) => {
                    self.handle_request(request);
                }
                Ok(None) => {
                    let idle = {
                        let last = *self
                            .last_activity
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        last.elapsed()
                    };
                    if idle >= self.idle_timeout {
                        return Ok(());
                    }
                }
                Err(err) => {
                    let io_err = io::Error::other(err);
                    if io_err.kind() == io::ErrorKind::WouldBlock
                        || io_err.kind() == io::ErrorKind::TimedOut
                    {
                        continue;
                    }
                    return Err(ConfigCenterError::Io(io_err));
                }
            }
        }
    }

    /// Handles a single request.
    fn handle_request(&self, request: Request) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        // Any request counts as activity, so an in-use page does not hit the
        // idle timeout.
        {
            let mut last = self
                .last_activity
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *last = Instant::now();
        }
        let method = request.method().clone();
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or("/").to_string();

        let mut request = request;
        let mut response = match (method.clone(), path.as_str()) {
            (Method::Get, "/") => self.handle_page(&request),
            (Method::Get, "/api/state") => self.handle_state(&request),
            (Method::Post, "/api/shutdown") => self.handle_shutdown(&request),
            (Method::Post, "/api/profiles") => self.handle_create(&mut request),
            (Method::Post, _) if path.ends_with("/activate") => {
                self.handle_activate(&request, &path)
            }
            (Method::Post, _) if path.ends_with("/delete") => {
                self.handle_delete(&request, &path)
            }
            (Method::Post, _) if path.starts_with("/api/profiles/") => {
                self.handle_update(&mut request, &path)
            }
            _ => error_response(StatusCode(404), "not found"),
        };

        add_common_headers(&mut response);
        let _ = request.respond(response);
    }

    /// Serves the configuration page.
    fn handle_page(&self, request: &Request) -> HttpResponse {
        if !self.has_valid_origin(request) {
            return error_response(StatusCode(403), "origin not allowed");
        }
        let body = render_page(&self.csrf_token);
        let mut response = Response::from_data(body.into_bytes());
        if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]) {
            response.add_header(header);
        }
        if let Ok(header) = Header::from_bytes(
            &b"Set-Cookie"[..],
            format!(
                "{SESSION_COOKIE_NAME}={}; HttpOnly; SameSite=Strict; Path=/",
                self.session_token
            )
            .as_bytes(),
        ) {
            response.add_header(header);
        }
        response
    }

    /// Serves the redacted store state.
    fn handle_state(&self, request: &Request) -> HttpResponse {
        if !self.has_valid_origin(request) {
            return error_response(StatusCode(403), "origin not allowed");
        }
        let profiles = match self.repository.list() {
            Ok(profiles) => profiles,
            Err(err) => return error_response(StatusCode(500), &err.to_string()),
        };
        let active_id = match self.repository.active_profile_id() {
            Ok(active_id) => active_id,
            Err(err) => return error_response(StatusCode(500), &err.to_string()),
        };
        let state = match super::ProviderStoreState::new(&profiles, active_id.as_deref(), &self.secret_store)
        {
            Ok(state) => state,
            Err(err) => return error_response(StatusCode(500), &err.to_string()),
        };
        json_response(StatusCode(200), &state)
    }

    /// Handles a shutdown request.
    ///
    /// The request must carry the bearer token, which the CLI owns and never
    /// hands to the page, so a stray local page cannot stop the server.
    fn handle_shutdown(&self, request: &Request) -> HttpResponse {
        if !self.has_valid_bearer_token(request) {
            return error_response(StatusCode(401), "missing or invalid bearer token");
        }
        self.shutdown_requested
            .store(true, Ordering::Relaxed);
        json_response(StatusCode(200), &serde_json::json!({ "stopping": true }))
    }

    /// Creates a profile.
    fn handle_create(&self, request: &mut Request) -> HttpResponse {
        if let Some(response) = self.reject_unauthorized(request) {
            return response;
        }
        let body = match read_body(request) {
            Ok(body) => body,
            Err(response) => return response,
        };
        let payload: CreateProfileRequest = match serde_json::from_slice(&body) {
            Ok(payload) => payload,
            Err(err) => return error_response(StatusCode(400), &format!("invalid JSON: {err}")),
        };
        let wire_api = match super::parse_wire_api(&payload.wire_api) {
            Ok(wire_api) => wire_api,
            Err(err) => return error_response(StatusCode(400), &err),
        };

        let existing = match self.repository.list() {
            Ok(profiles) => profiles,
            Err(err) => return error_response(StatusCode(500), &err.to_string()),
        };
        let id = payload.id.clone().unwrap_or_else(|| {
            super::unique_profile_id(
                &existing
                    .iter()
                    .map(|profile| (profile.id.clone(), profile.clone()))
                    .collect(),
            )
        });
        let display_name = payload
            .display_name
            .clone()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| id.clone());

        let mut profile = ProviderProfile::new(id.clone(), display_name, payload.base_url, wire_api, payload.model);
        profile.provider_id = payload.provider_id.clone();

        if let Some(api_key) = payload.api_key.as_ref() {
            if !api_key.trim().is_empty() {
                // Remove the previous secret first, so a replaced key does
                // not linger in the credential store.
                if let Some(previous) = profile.credential.as_ref() {
                    let _ = self.secret_store.delete_secret(previous);
                }
                match self.secret_store.store_secret(&id, api_key.trim()) {
                    Ok(secret_ref) => profile.credential = Some(secret_ref),
                    Err(err) => return error_response(StatusCode(500), &err.to_string()),
                }
            }
        }

        let stored_secret = profile.credential.clone();
        let revision = match self.current_revision() {
            Ok(revision) => revision,
            Err(err) => return error_response(StatusCode(500), &err.to_string()),
        };
        match self.repository.add(profile, Some(revision)) {
            Ok(stored) => json_response(StatusCode(200), &stored),
            Err(err) => {
                // The profile was not created, so the secret that was just
                // stored would be an orphan. Remove it.
                if let Some(secret_ref) = stored_secret.as_ref() {
                    let _ = self.secret_store.delete_secret(secret_ref);
                }
                let status = if is_conflict(&err) {
                    StatusCode(409)
                } else {
                    StatusCode(400)
                };
                error_response(status, &err.to_string())
            }
        }
    }

    /// Updates a profile.
    fn handle_update(&self, request: &mut Request, path: &str) -> HttpResponse {
        if let Some(response) = self.reject_unauthorized(request) {
            return response;
        }
        let Some(id) = path_id(path, "/api/profiles/") else {
            return error_response(StatusCode(400), "missing profile id");
        };
        let body = match read_body(request) {
            Ok(body) => body,
            Err(response) => return response,
        };
        let payload: UpdateProfileRequest = match serde_json::from_slice(&body) {
            Ok(payload) => payload,
            Err(err) => return error_response(StatusCode(400), &format!("invalid JSON: {err}")),
        };

        let mut profile = match self.repository.get(&id) {
            Ok(Some(profile)) => profile,
            Ok(None) => return error_response(StatusCode(404), "profile does not exist"),
            Err(err) => return error_response(StatusCode(500), &err.to_string()),
        };

        if let Some(display_name) = payload.display_name.as_ref() {
            profile.display_name = display_name.clone();
        }
        if let Some(base_url) = payload.base_url.as_ref() {
            profile.base_url = base_url.clone();
        }
        if let Some(wire_api) = payload.wire_api.as_ref() {
            match super::parse_wire_api(wire_api) {
                Ok(wire_api) => profile.wire_api = wire_api,
                Err(err) => return error_response(StatusCode(400), &err),
            }
        }
        if let Some(model) = payload.model.as_ref() {
            profile.model = model.clone();
        }
        if let Some(provider_id) = payload.provider_id.as_ref() {
            profile.provider_id = Some(provider_id.clone());
        }
        if payload.clear_api_key == Some(true) {
            if let Some(secret_ref) = profile.credential.take() {
                let _ = self.secret_store.delete_secret(&secret_ref);
            }
        }
        if let Some(api_key) = payload.api_key.as_ref() {
            if !api_key.trim().is_empty() {
                match self.secret_store.store_secret(&id, api_key.trim()) {
                    Ok(secret_ref) => profile.credential = Some(secret_ref),
                    Err(err) => return error_response(StatusCode(500), &err.to_string()),
                }
            }
        }

        profile.revision += 1;
        profile.updated_at = super::now_secs();

        let revision = match self.current_revision() {
            Ok(revision) => revision,
            Err(err) => return error_response(StatusCode(500), &err.to_string()),
        };
        match self.repository.update(profile, Some(revision)) {
            Ok(stored) => json_response(StatusCode(200), &stored),
            Err(err) => {
                let status = if is_conflict(&err) {
                    StatusCode(409)
                } else {
                    StatusCode(400)
                };
                error_response(status, &err.to_string())
            }
        }
    }

    /// Activates a profile.
    fn handle_activate(&self, request: &Request, path: &str) -> HttpResponse {
        if let Some(response) = self.reject_unauthorized(request) {
            return response;
        }
        let Some(id) = path_id(path, "/api/profiles/") else {
            return error_response(StatusCode(400), "missing profile id");
        };
        let revision = match self.current_revision() {
            Ok(revision) => revision,
            Err(err) => return error_response(StatusCode(500), &err.to_string()),
        };
        match self.repository.activate(&id, Some(revision)) {
            Ok(stored) => json_response(StatusCode(200), &stored),
            Err(err) => {
                let status = if is_conflict(&err) {
                    StatusCode(409)
                } else {
                    StatusCode(400)
                };
                error_response(status, &err.to_string())
            }
        }
    }

    /// Deletes a profile.
    fn handle_delete(&self, request: &Request, path: &str) -> HttpResponse {
        if let Some(response) = self.reject_unauthorized(request) {
            return response;
        }
        let Some(id) = path_id(path, "/api/profiles/") else {
            return error_response(StatusCode(400), "missing profile id");
        };
        let profile = match self.repository.get(&id) {
            Ok(Some(profile)) => profile,
            Ok(None) => return error_response(StatusCode(404), "profile does not exist"),
            Err(err) => return error_response(StatusCode(500), &err.to_string()),
        };
        if let Some(secret_ref) = profile.credential.as_ref() {
            let _ = self.secret_store.delete_secret(secret_ref);
        }
        let revision = match self.current_revision() {
            Ok(revision) => revision,
            Err(err) => return error_response(StatusCode(500), &err.to_string()),
        };
        match self.repository.remove(&id, None, Some(revision)) {
            Ok(removed) => json_response(
                StatusCode(200),
                &serde_json::json!({ "removed": removed }),
            ),
            Err(err) => {
                let status = if is_conflict(&err) {
                    StatusCode(409)
                } else {
                    StatusCode(400)
                };
                error_response(status, &err.to_string())
            }
        }
    }

    /// Rejects a request that fails the authorization checks.
    fn reject_unauthorized(&self, request: &Request) -> Option<HttpResponse> {
        if !self.has_valid_origin(request) {
            return Some(error_response(StatusCode(403), "origin not allowed"));
        }
        if !self.has_valid_credential(request) {
            return Some(error_response(StatusCode(401), "missing or invalid credentials"));
        }
        let csrf = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("X-CSRF-Token"))
            .map(|header| header.value.as_str().to_string());
        match csrf {
            Some(csrf) if constant_time_eq(csrf.as_bytes(), self.csrf_token.as_bytes()) => None,
            Some(_) => Some(error_response(StatusCode(403), "invalid csrf token")),
            None => Some(error_response(StatusCode(403), "missing csrf token")),
        }
    }

    /// Checks that the request carries a valid bearer token or session cookie.
    fn has_valid_credential(&self, request: &Request) -> bool {
        if let Some(auth) = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Authorization"))
            .map(|header| header.value.as_str().to_string())
        {
            if let Some(token) = auth.strip_prefix("Bearer ")
                && constant_time_eq(token.as_bytes(), self.bearer_token.as_bytes())
            {
                return true;
            }
        }
        if let Some(cookie) = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Cookie"))
            .map(|header| header.value.as_str().to_string())
            && let Some(session) = parse_cookie(&cookie, SESSION_COOKIE_NAME)
            && constant_time_eq(session.as_bytes(), self.session_token.as_bytes())
        {
            return true;
        }
        false
    }

    /// Checks that the request carries the bearer token.
    fn has_valid_bearer_token(&self, request: &Request) -> bool {
        request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Authorization"))
            .and_then(|header| header.value.as_str().strip_prefix("Bearer ").map(str::to_string))
            .is_some_and(|token| constant_time_eq(token.as_bytes(), self.bearer_token.as_bytes()))
    }

    /// Checks the `Origin` header against the server's own origin.
    fn has_valid_origin(&self, request: &Request) -> bool {
        let origin = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Origin"))
            .map(|header| header.value.as_str().to_string());
        let Some(origin) = origin else {
            // Same-origin navigations from the page do not always send
            // `Origin`. Require a matching `Host` instead.
            return self.has_valid_host(request);
        };
        origin == format!("http://127.0.0.1:{}", self.port)
            || origin == format!("http://localhost:{}", self.port)
    }

    /// Checks the `Host` header against the server's own address.
    fn has_valid_host(&self, request: &Request) -> bool {
        let host = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Host"))
            .map(|header| header.value.as_str().to_string());
        match host {
            Some(host) => {
                host == format!("127.0.0.1:{}", self.port)
                    || host == format!("localhost:{}", self.port)
            }
            None => false,
        }
    }
}

/// Request body for creating a profile.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
struct CreateProfileRequest {
    id: Option<String>,
    display_name: Option<String>,
    base_url: String,
    wire_api: String,
    model: String,
    provider_id: Option<String>,
    api_key: Option<String>,
}

/// Request body for updating a profile.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
struct UpdateProfileRequest {
    display_name: Option<String>,
    base_url: Option<String>,
    wire_api: Option<String>,
    model: Option<String>,
    provider_id: Option<String>,
    api_key: Option<String>,
    clear_api_key: Option<bool>,
}

/// Reads a request body, rejecting anything oversized.
fn read_body(request: &mut Request) -> Result<Vec<u8>, HttpResponse> {
    let mut body = Vec::new();
    let reader = request.as_reader();
    let mut buffer = [0u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if body.len() + count > MAX_BODY_BYTES {
                    return Err(error_response(
                        StatusCode(413),
                        "request body is too large",
                    ));
                }
                body.extend_from_slice(&buffer[..count]);
            }
            Err(err) => {
                return Err(error_response(
                    StatusCode(400),
                    &format!("could not read request body: {err}"),
                ));
            }
        }
    }
    Ok(body)
}

/// Extracts the profile id from a request path.
fn path_id(path: &str, prefix: &str) -> Option<String> {
    let rest = path.strip_prefix(prefix)?;
    let id = rest.split('/').next()?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

/// Builds a JSON response.
fn json_response<T: serde::Serialize>(status: StatusCode, value: &T) -> HttpResponse {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    let mut response = Response::from_string(body).with_status_code(status);
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]) {
        response.add_header(header);
    }
    response
}

/// Reports whether a store error is a revision conflict.
fn is_conflict(error: &ProviderProfileStoreError) -> bool {
    matches!(error, ProviderProfileStoreError::Conflict { .. })
}

/// Builds an error response.
fn error_response(status: StatusCode, message: &str) -> HttpResponse {
    let mut response = Response::from_string(message.to_string()).with_status_code(status);
    if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], &b"text/plain; charset=utf-8"[..]) {
        response.add_header(header);
    }
    response
}

/// Adds the security headers every response carries.
fn add_common_headers(response: &mut HttpResponse) {
    let headers: [(&[u8], &[u8]); 5] = [
        (b"Cache-Control", b"no-store"),
        (b"X-Content-Type-Options", b"nosniff"),
        (b"X-Frame-Options", b"DENY"),
        (
            b"Content-Security-Policy",
            b"default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; form-action 'none'; base-uri 'none'; frame-ancestors 'none'",
        ),
        (b"Referrer-Policy", b"no-referrer"),
    ];
    for (name, value) in headers {
        if let Ok(header) = Header::from_bytes(name, value) {
            response.add_header(header);
        }
    }
}

/// Compares two byte slices without an early exit on the first mismatch.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

/// Extracts one cookie value from a `Cookie` header.
fn parse_cookie(cookie: &str, name: &str) -> Option<String> {
    for part in cookie.split(';') {
        let part = part.trim();
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        if key.trim() == name {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// Generates a random token.
fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Returns the code home used by the configuration center.
pub fn config_center_code_home(code_home: &std::path::Path) -> PathBuf {
    code_home.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_profiles::ProviderProfile;
    use crate::provider_profiles::ProviderProfileRepository;
    use crate::provider_profiles::ProviderSecretStore;
    use std::path::PathBuf;

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "qcodex-config-center-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn profile(id: &str) -> ProviderProfile {
        ProviderProfile::new(
            id,
            "Example",
            "https://example.com/v1",
            crate::model_provider_info::WireApi::Chat,
            "m",
        )
    }

    #[test]
    fn the_server_binds_to_loopback_only() {
        let home = TempHome::new("loopback");
        let center = ConfigCenter::start(&home.0, Duration::from_secs(60)).expect("start");
        assert!(center.port() > 0);
        assert!(center.url().starts_with("http://127.0.0.1:"));
    }

    #[test]
    fn the_bearer_token_is_not_the_session_token() {
        let home = TempHome::new("tokens");
        let center = ConfigCenter::start(&home.0, Duration::from_secs(60)).expect("start");
        assert_ne!(center.bearer_token(), center.session_token());
        assert!(!center.session_token().is_empty());
        assert!(!center.bearer_token().is_empty());
    }

    #[test]
    fn create_request_accepts_the_configuration_page_payload() {
        let payload = serde_json::json!({
            "display_name": "MiniMax",
            "base_url": "https://api.minimax.cn/v1",
            "wire_api": "chat",
            "model": "MiniMax-M3",
            "api_key": "test-key",
        });

        let request: CreateProfileRequest =
            serde_json::from_value(payload).expect("configuration page payload parses");

        assert_eq!(request.display_name.as_deref(), Some("MiniMax"));
        assert_eq!(request.base_url, "https://api.minimax.cn/v1");
        assert_eq!(request.wire_api, "chat");
        assert_eq!(request.model, "MiniMax-M3");
        assert_eq!(request.api_key.as_deref(), Some("test-key"));
    }

    #[test]
    fn update_request_accepts_the_configuration_page_payload() {
        let payload = serde_json::json!({
            "display_name": "MiniMax updated",
            "base_url": "https://api.minimax.cn/v1",
            "wire_api": "responses",
            "model": "MiniMax-M3",
            "clear_api_key": true,
        });

        let request: UpdateProfileRequest =
            serde_json::from_value(payload).expect("configuration page payload parses");

        assert_eq!(request.display_name.as_deref(), Some("MiniMax updated"));
        assert_eq!(request.base_url.as_deref(), Some("https://api.minimax.cn/v1"));
        assert_eq!(request.wire_api.as_deref(), Some("responses"));
        assert_eq!(request.model.as_deref(), Some("MiniMax-M3"));
        assert_eq!(request.clear_api_key, Some(true));
    }

    #[test]
    fn state_response_uses_the_configuration_page_field_names() {
        let home = TempHome::new("state-json");
        let secret_store = ProviderSecretStore::with_keyring(
            &home.0,
            crate::provider_profiles::KeyringStore::in_memory(),
        );
        let state = crate::provider_profiles::ProviderStoreState::new(
            &[profile("p1")],
            Some("p1"),
            &secret_store,
        )
        .expect("state");

        let value = serde_json::to_value(state).expect("serialize state");
        let profile = &value["profiles"][0];
        assert_eq!(value["active_profile_id"], "p1");
        assert_eq!(profile["display_name"], "Example");
        assert_eq!(profile["base_url"], "https://example.com/v1");
        assert_eq!(profile["wire_api"], "chat");
        assert_eq!(profile["has_api_key"], false);
    }

    #[test]
    fn a_request_refreshes_the_idle_timer() {
        let home = TempHome::new("idle");
        let center = ConfigCenter::start(&home.0, Duration::from_secs(60)).expect("start");
        let before = *center
            .last_activity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // Serving any request must move the timer forward.
        let server = Arc::clone(&center.server);
        std::thread::spawn(move || {
            let _ = server.recv_timeout(Duration::from_millis(200));
        });

        let after = *center
            .last_activity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(after >= before);
    }

    #[test]
    fn a_conflicting_write_is_rejected() {
        let home = TempHome::new("conflict");
        let center = ConfigCenter::start(&home.0, Duration::from_secs(60)).expect("start");

        // Take the current revision, then change the store behind the
        // server's back, then try to write with the stale revision.
        let revision = center.current_revision().expect("revision");
        let repository = ProviderProfileRepository::new(&home.0);
        repository.add(profile("p1"), None).expect("add");

        let error = repository
            .activate("p1", Some(revision))
            .expect_err("conflict expected");
        assert!(matches!(
            error,
            crate::provider_profiles::ProviderProfileStoreError::Conflict { .. }
        ));
    }

    #[test]
    fn an_added_profile_never_stores_the_key_in_the_profile_document() {
        let home = TempHome::new("no-key");
        let secret_store = ProviderSecretStore::with_keyring(
            &home.0,
            crate::provider_profiles::KeyringStore::in_memory(),
        );
        let secret_ref = secret_store.store_secret("p1", "sk-secret").expect("store");

        let repository = ProviderProfileRepository::new(&home.0);
        let mut with_secret = profile("p1");
        with_secret.credential = Some(secret_ref);
        repository.add(with_secret, None).expect("add");

        let raw = std::fs::read(repository.path()).expect("read");
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains("sk-secret"));
    }
}
