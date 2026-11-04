//! Integration tests for provider profile routing.
//!
//! These tests stand up two mock OpenAI-compatible servers and prove that
//! each profile reaches its own endpoint with its own credential and model,
//! and that a switch never sends a request to the previous provider.

use super::ProviderProfile;
use super::ProviderProfileRepository;
use super::ProviderSecretStore;
use super::ProviderSecretRef;
use crate::model_provider_info::WireApi;
use std::path::PathBuf;
use std::sync::Arc;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

struct TempHome(PathBuf);

impl TempHome {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "qcodex-profile-integration-{tag}-{}",
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

/// Builds a profile pointing at `base_url`.
fn profile(id: &str, base_url: &str, model: &str) -> ProviderProfile {
    ProviderProfile::new(
        id,
        format!("Provider {id}"),
        base_url,
        WireApi::Chat,
        model,
    )
}

/// Returns the `Authorization` header of the first recorded request.
fn first_authorization(requests: &[wiremock::Request]) -> Option<String> {
    requests
        .first()
        .and_then(|request| {
            request
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
        })
        .map(str::to_string)
}

/// Returns the body of the first recorded request.
fn first_body(requests: &[wiremock::Request]) -> Option<String> {
    requests
        .first()
        .map(|request| String::from_utf8_lossy(&request.body).to_string())
}

#[tokio::test]
#[serial_test::serial]
async fn each_profile_reaches_its_own_endpoint_with_its_own_credential() {
    let home = TempHome::new("routing");
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server_a)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server_b)
        .await;

    // Use an in-memory credential store so the test never touches the
    // machine's real keychain, which would block on an authorization prompt.
    let secret_store = ProviderSecretStore::with_keyring(
        &home.0,
        super::KeyringStore::in_memory(),
    );
    super::register_secret_store(&home.0, Arc::new(secret_store.clone()));

    let repository = ProviderProfileRepository::new(&home.0);
    let mut profile_a = profile("a", &format!("{}/v1", server_a.uri()), "model-a");
    profile_a.credential = Some(secret_store.store_secret("a", "key-a").expect("store a"));
    repository.add(profile_a, None).expect("add a");

    let mut profile_b = profile("b", &format!("{}/v1", server_b.uri()), "model-b");
    profile_b.credential = Some(secret_store.store_secret("b", "key-b").expect("store b"));
    repository.add(profile_b, None).expect("add b");

    // Profile A is active.
    repository.activate("a", None).expect("activate a");
    let resolved = super::ProviderProfileResolver::with_repository(repository.clone())
        .resolve(&super::ProviderResolutionInputs::default())
        .expect("resolve")
        .expect("resolved");

    let client = reqwest::Client::new();
    let request = resolved
        .provider
        .create_request_builder(&client, &None)
        .await
        .expect("request builder");
    request
        .json(&serde_json::json!({ "model": "model-a" }))
        .send()
        .await
        .expect("send request to profile A");

    let requests_a = server_a.received_requests().await.expect("requests a");
    assert_eq!(requests_a.len(), 1, "profile A must reach server A");
    assert_eq!(first_authorization(&requests_a).as_deref(), Some("Bearer key-a"));
    let body = first_body(&requests_a).expect("body");
    assert!(
        body.contains("model-a"),
        "profile A must request model-a, body was: {body}"
    );

    // Switch to profile B.
    repository.activate("b", None).expect("activate b");
    let resolved = super::ProviderProfileResolver::with_repository(repository.clone())
        .resolve(&super::ProviderResolutionInputs::default())
        .expect("resolve")
        .expect("resolved");

    let request = resolved
        .provider
        .create_request_builder(&client, &None)
        .await
        .expect("request builder");
    request
        .json(&serde_json::json!({ "model": "model-b" }))
        .send()
        .await
        .expect("send request to profile B");

    let requests_b = server_b.received_requests().await.expect("requests b");
    assert_eq!(requests_b.len(), 1, "profile B must reach server B");
    assert_eq!(first_authorization(&requests_b).as_deref(), Some("Bearer key-b"));
    let body = first_body(&requests_b).expect("body");
    assert!(
        body.contains("model-b"),
        "profile B must request model-b, body was: {body}"
    );

    // The switch must not send anything to the previous provider.
    let requests_a_after = server_a.received_requests().await.expect("requests a again");
    assert_eq!(
        requests_a_after.len(),
        1,
        "server A must not receive requests after the switch"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn a_profile_without_a_credential_fails_closed() {
    let home = TempHome::new("no-credential");
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    // Register an empty store so the lookup finds nothing.
    super::register_secret_store(
        &home.0,
        Arc::new(ProviderSecretStore::with_keyring(
            &home.0,
            super::KeyringStore::in_memory(),
        )),
    );

    let repository = ProviderProfileRepository::new(&home.0);
    let mut orphan = profile("orphan", &format!("{}/v1", server.uri()), "model-x");
    orphan.credential = Some(ProviderSecretRef::new("missing"));
    repository.add(orphan, None).expect("add");
    repository.activate("orphan", None).expect("activate");

    let resolved = super::ProviderProfileResolver::with_repository(repository)
        .resolve(&super::ProviderResolutionInputs::default())
        .expect("resolve")
        .expect("resolved");

    let client = reqwest::Client::new();
    let result = resolved
        .provider
        .create_request_builder(&client, &None)
        .await;

    assert!(
        result.is_err(),
        "a profile whose credential is missing must not build a request"
    );

    let requests = server.received_requests().await.expect("requests");
    assert!(
        requests.is_empty(),
        "no request may reach the provider when the credential is missing"
    );
}

#[tokio::test]
async fn the_stored_profile_document_never_contains_a_secret() {
    let home = TempHome::new("no-secret-on-disk");
    let server = MockServer::start().await;
    let secret_store = ProviderSecretStore::with_keyring(
        &home.0,
        super::KeyringStore::in_memory(),
    );
    let secret_ref = secret_store.store_secret("a", "sk-super-secret").expect("store");

    let repository = ProviderProfileRepository::new(&home.0);
    let mut with_secret = profile("a", &format!("{}/v1", server.uri()), "model-a");
    with_secret.credential = Some(secret_ref);
    repository.add(with_secret, None).expect("add");

    let raw = std::fs::read(repository.path()).expect("read profile document");
    let text = String::from_utf8_lossy(&raw);
    assert!(
        !text.contains("sk-super-secret"),
        "the profile document must not contain the API key"
    );
}
