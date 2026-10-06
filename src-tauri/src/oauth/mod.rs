use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const DEFAULT_AUTH_BASE: &str = "https://auth.atlassian.com";
pub const DEFAULT_API_BASE: &str = "https://api.atlassian.com";
/// Atlassian requires the callback URL to be registered exactly, so the app
/// listens on a fixed loopback port.
pub const DEFAULT_CALLBACK_PORT: u16 = 8765;
pub const DEFAULT_SCOPES: &[&str] = &[
    "read:jira-work",
    "write:jira-work",
    "read:jira-user",
    "manage:jira-webhooks",
    "offline_access",
];

#[derive(Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub auth_base: String,
    pub api_base: String,
    pub scopes: Vec<String>,
}

impl OAuthConfig {
    pub fn new(client_id: String, client_secret: Option<String>) -> Self {
        Self {
            client_id,
            client_secret,
            auth_base: DEFAULT_AUTH_BASE.to_string(),
            api_base: DEFAULT_API_BASE.to_string(),
            scopes: DEFAULT_SCOPES.iter().map(|scope| scope.to_string()).collect(),
        }
    }

    #[allow(dead_code)] // used by tests to point the client at a mock server
    pub fn with_bases(mut self, auth_base: &str, api_base: &str) -> Self {
        self.auth_base = auth_base.to_string();
        self.api_base = api_base.to_string();
        self
    }

    fn http(&self) -> Result<reqwest::blocking::Client, String> {
        reqwest::blocking::Client::builder()
            .user_agent("jira-agent-desktop")
            .build()
            .map_err(|error| error.to_string())
    }

    fn oauth_error(value: &Value, status: reqwest::StatusCode) -> String {
        let detail = value
            .get("error_description")
            .or_else(|| value.get("error"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| status.to_string());
        format!("JIRA OAuth request failed: {detail}")
    }

    fn token_request(&self, form: &[(&str, &str)]) -> Result<TokenResponse, String> {
        let response = self
            .http()?
            .post(format!("{}/oauth/token", self.auth_base.trim_end_matches('/')))
            .header("Accept", "application/json")
            .form(form)
            .send()
            .map_err(|error| format!("JIRA OAuth request failed: {error}"))?;
        let status = response.status();
        let value: Value = response.json().unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(Self::oauth_error(&value, status));
        }
        serde_json::from_value(value).map_err(|error| format!("invalid token response: {error}"))
    }

    pub fn exchange_code(
        &self,
        redirect_uri: &str,
        code: &str,
        verifier: &str,
    ) -> Result<TokenResponse, String> {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("client_id", self.client_id.as_str()),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ];
        if let Some(secret) = &self.client_secret {
            form.push(("client_secret", secret.as_str()));
        }
        self.token_request(&form)
    }

    pub fn refresh(&self, refresh_token: &str) -> Result<TokenResponse, String> {
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("client_id", self.client_id.as_str()),
            ("refresh_token", refresh_token),
        ];
        if let Some(secret) = &self.client_secret {
            form.push(("client_secret", secret.as_str()));
        }
        self.token_request(&form)
    }

    pub fn accessible_resources(
        &self,
        access_token: &str,
    ) -> Result<Vec<AccessibleResource>, String> {
        let response = self
            .http()?
            .get(format!(
                "{}/oauth/token/accessible-resources",
                self.api_base.trim_end_matches('/')
            ))
            .bearer_auth(access_token)
            .header("Accept", "application/json")
            .send()
            .map_err(|error| format!("JIRA OAuth request failed: {error}"))?;
        let status = response.status();
        let value: Value = response.json().unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(Self::oauth_error(&value, status));
        }
        serde_json::from_value(value)
            .map_err(|error| format!("invalid accessible resources response: {error}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibleResource {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default, rename = "avatarUrl")]
    pub avatar_url: Option<String>,
}

pub fn generate_verifier() -> String {
    format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

pub fn challenge_for(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

pub fn generate_state() -> String {
    Uuid::new_v4().simple().to_string()
}

pub fn authorize_url(
    config: &OAuthConfig,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> String {
    let mut url = reqwest::Url::parse(&format!(
        "{}/authorize",
        config.auth_base.trim_end_matches('/')
    ))
    .expect("auth base must be a valid URL");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("audience", "api.atlassian.com");
        query.append_pair("client_id", &config.client_id);
        query.append_pair("scope", &config.scopes.join(" "));
        query.append_pair("redirect_uri", redirect_uri);
        query.append_pair("state", state);
        query.append_pair("response_type", "code");
        query.append_pair("prompt", "consent");
        query.append_pair("code_challenge", challenge);
        query.append_pair("code_challenge_method", "S256");
    }
    url.to_string()
}

/// A one-shot loopback listener that captures the OAuth redirect.
pub struct CallbackListener {
    listener: TcpListener,
    pub port: u16,
}

impl CallbackListener {
    pub fn bind() -> Result<Self, String> {
        Self::bind_on(DEFAULT_CALLBACK_PORT).map_err(|error| {
            format!(
                "could not listen on the JIRA callback port {DEFAULT_CALLBACK_PORT}: {error}. \
                 Close whatever is using it and try again."
            )
        })
    }

    pub fn bind_on(port: u16) -> Result<Self, String> {
        let listener =
            TcpListener::bind(("127.0.0.1", port)).map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        Ok(Self { listener, port })
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://localhost:{}/callback", self.port)
    }

    /// Blocks until the browser hits `/callback`, returning the authorization
    /// code. Non-callback requests are answered and ignored.
    pub fn wait(self, expected_state: &str, timeout: Duration) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() >= deadline {
                return Err("timed out waiting for JIRA authorization".to_string());
            }
            let (mut stream, _) = match self.listener.accept() {
                Ok(value) => value,
                Err(error) => return Err(error.to_string()),
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buffer = [0u8; 8192];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]);
            let request_line = request.lines().next().unwrap_or_default();
            let (path, params) = parse_request_line(request_line);

            if path != "/callback" {
                let _ = respond(&mut stream, 404, "<h1>Not found</h1>");
                continue;
            }

            let result = validate_callback(&params, expected_state);
            match &result {
                Ok(_) => {
                    let _ = respond(
                        &mut stream,
                        200,
                        "<h1>JIRA connected</h1><p>You can close this tab and return to the app.</p>",
                    );
                }
                Err(error) => {
                    let _ = respond(&mut stream, 400, &format!("<h1>Authorization failed</h1><p>{error}</p>"));
                }
            }
            return result;
        }
    }
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = if status < 400 { "OK" } else { "Bad Request" };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

pub fn parse_request_line(request_line: &str) -> (String, HashMap<String, String>) {
    let mut parts = request_line.split_whitespace();
    let _method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), query),
        None => (target.to_string(), ""),
    };
    let mut params = HashMap::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };
        params.insert(
            percent_decode(key),
            percent_decode(value),
        );
    }
    (path, params)
}

fn validate_callback(
    params: &HashMap<String, String>,
    expected_state: &str,
) -> Result<String, String> {
    if let Some(error) = params.get("error") {
        let description = params
            .get("error_description")
            .map(String::as_str)
            .unwrap_or_default();
        return Err(format!("JIRA authorization failed: {error} {description}").trim().to_string());
    }
    let state = params
        .get("state")
        .ok_or_else(|| "authorization callback is missing state".to_string())?;
    if state != expected_state {
        return Err("authorization state did not match".to_string());
    }
    params
        .get("code")
        .cloned()
        .ok_or_else(|| "authorization callback is missing code".to_string())
}

pub fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        output.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        output.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockServer;
    use std::thread;

    fn test_config(base: &str) -> OAuthConfig {
        OAuthConfig::new("client-123".to_string(), Some("secret-123".to_string()))
            .with_bases(base, base)
    }

    #[test]
    fn builds_authorize_url_with_pkce() {
        let config = test_config(DEFAULT_AUTH_BASE);
        let url = authorize_url(
            &config,
            "http://127.0.0.1:1234/callback",
            "state-abc",
            "challenge-xyz",
        );
        assert!(url.starts_with("https://auth.atlassian.com/authorize?"));
        assert!(url.contains("client_id=client-123"));
        assert!(url.contains("state=state-abc"));
        assert!(url.contains("code_challenge=challenge-xyz"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("audience=api.atlassian.com"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A1234%2Fcallback"));
    }

    #[test]
    fn challenge_matches_rfc7636_example() {
        // RFC 7636 Appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge_for(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifier_length_is_valid() {
        let verifier = generate_verifier();
        assert!(verifier.len() >= 43 && verifier.len() <= 128);
    }

    #[test]
    fn parses_and_validates_callbacks() {
        let (path, params) = parse_request_line("GET /callback?code=abc&state=s1 HTTP/1.1");
        assert_eq!(path, "/callback");
        assert_eq!(validate_callback(&params, "s1").unwrap(), "abc");
        assert!(validate_callback(&params, "other").is_err());
        assert!(validate_callback(&HashMap::new(), "s1").is_err());
    }

    #[test]
    fn listener_captures_code_and_rejects_bad_state() {
        let listener = CallbackListener::bind_on(0).unwrap();
        let redirect = listener.redirect_uri();
        let handle = thread::spawn(move || listener.wait("expected", Duration::from_secs(10)));

        let response = reqwest::blocking::get(format!("{redirect}?code=the-code&state=expected"))
            .expect("callback request");
        assert!(response.status().is_success());
        assert_eq!(handle.join().unwrap().unwrap(), "the-code");

        let listener = CallbackListener::bind_on(0).unwrap();
        let redirect = listener.redirect_uri();
        let handle = thread::spawn(move || listener.wait("expected", Duration::from_secs(10)));
        let _ = reqwest::blocking::get(format!("{redirect}?code=the-code&state=wrong"));
        assert!(handle.join().unwrap().is_err());
    }

    #[test]
    fn exchanges_code_and_lists_resources_against_mock() {
        let server = MockServer::start(|request, body| match request {
            "POST /oauth/token" => {
                assert!(body.contains("grant_type=authorization_code"));
                assert!(body.contains("code=the-code"));
                assert!(body.contains("code_verifier="));
                (
                    200,
                    r#"{"access_token":"access-1","refresh_token":"refresh-1","expires_in":3600,"scope":"read:jira-work"}"#.to_string(),
                )
            }
            "GET /oauth/token/accessible-resources" => (
                200,
                r#"[{"id":"cloud-1","url":"https://acme.atlassian.net","name":"Acme","scopes":["read:jira-work"]}]"#.to_string(),
            ),
            _ => (404, r#"{"error":"not found"}"#.to_string()),
        });
        let config = test_config(&server.base_url());

        let token = config
            .exchange_code("http://127.0.0.1:1/callback", "the-code", "verifier")
            .unwrap();
        assert_eq!(token.access_token, "access-1");
        assert_eq!(token.refresh_token.as_deref(), Some("refresh-1"));

        let resources = config.accessible_resources(&token.access_token).unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].id, "cloud-1");
        assert_eq!(resources[0].url, "https://acme.atlassian.net");
    }

    #[test]
    fn refreshes_tokens_against_mock() {
        let server = MockServer::start(|request, body| {
            if request == "POST /oauth/token" {
                assert!(body.contains("grant_type=refresh_token"));
                assert!(body.contains("refresh_token=refresh-1"));
                (
                    200,
                    r#"{"access_token":"access-2","refresh_token":"refresh-2","expires_in":3600}"#
                        .to_string(),
                )
            } else {
                (404, "{}".to_string())
            }
        });
        let config = test_config(&server.base_url());
        let token = config.refresh("refresh-1").unwrap();
        assert_eq!(token.access_token, "access-2");
    }

    #[test]
    fn surfaces_oauth_errors() {
        let server = MockServer::start(|_request, _body| {
            (400, r#"{"error":"invalid_grant","error_description":"code expired"}"#.to_string())
        });
        let config = test_config(&server.base_url());
        let error = config
            .exchange_code("http://127.0.0.1:1/callback", "x", "y")
            .unwrap_err();
        assert!(error.contains("code expired"));
    }
}
