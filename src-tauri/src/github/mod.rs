use serde_json::{json, Value};

pub fn parse_owner_repo(url: &str) -> Option<(String, String)> {
    let url = url.trim().trim_end_matches(".git");
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        return split_owner_repo(rest);
    }
    for prefix in [
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
    ] {
        if let Some(rest) = url.strip_prefix(prefix) {
            return split_owner_repo(rest);
        }
    }
    None
}

fn split_owner_repo(value: &str) -> Option<(String, String)> {
    let mut parts = value.trim_matches('/').splitn(2, '/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

pub struct GithubClient {
    token: String,
    http: reqwest::blocking::Client,
}

impl GithubClient {
    pub fn new(token: String) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .user_agent("jira-agent-desktop")
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { token, http })
    }

    pub fn account(&self) -> Result<String, String> {
        let response = self
            .http
            .get("https://api.github.com/user")
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|error| format!("github request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("github auth failed: {}", response.status()));
        }
        let value: Value = response
            .json()
            .map_err(|error| error.to_string())?;
        value
            .get("login")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "github user response missing login".to_string())
    }

    pub fn create_pull_request(
        &self,
        owner: &str,
        repo: &str,
        title: &str,
        head: &str,
        base: &str,
        body: &str,
    ) -> Result<Value, String> {
        let response = self
            .http
            .post(format!(
                "https://api.github.com/repos/{owner}/{repo}/pulls"
            ))
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .json(&json!({ "title": title, "head": head, "base": base, "body": body }))
            .send()
            .map_err(|error| format!("github request failed: {error}"))?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().unwrap_or_default();
            return Err(format!("github returned {status}: {text}"));
        }
        response.json().map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_urls() {
        assert_eq!(
            parse_owner_repo("https://github.com/octocat/Hello-World.git"),
            Some(("octocat".to_string(), "Hello-World".to_string()))
        );
        assert_eq!(
            parse_owner_repo("git@github.com:octocat/Hello-World.git"),
            Some(("octocat".to_string(), "Hello-World".to_string()))
        );
        assert_eq!(
            parse_owner_repo("https://github.com/octocat/Hello-World"),
            Some(("octocat".to_string(), "Hello-World".to_string()))
        );
        assert_eq!(parse_owner_repo("https://gitlab.com/foo/bar.git"), None);
    }
}
