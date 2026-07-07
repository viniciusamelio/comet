#![cfg_attr(not(feature = "cloudflare"), allow(dead_code))]

use base64ct::Encoding;
use p256::ecdsa::SigningKey;
use p256::ecdsa::signature::Signer;
use p256::pkcs8::DecodePrivateKey;
use serde::Deserialize;
use url::form_urlencoded;

use crate::config::{
    AppleProviderConfig, GitHubProviderConfig, GoogleProviderConfig, ProviderConfig,
};
#[cfg(feature = "cloudflare")]
use crate::oidc::{self, OidcValidation};
use crate::session;
#[cfg(feature = "cloudflare")]
use crate::session::ProviderIdentity;
use crate::{AuthConfig, AuthError};

pub const OAUTH_STATE_TTL_SECONDS: u64 = 10 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthProviderId {
    Google,
    Apple,
    GitHub,
}

impl OAuthProviderId {
    pub fn parse(value: &str) -> Result<Self, AuthError> {
        match value {
            "google" => Ok(Self::Google),
            "apple" => Ok(Self::Apple),
            "github" => Ok(Self::GitHub),
            other => Err(AuthError::UnsupportedProvider(other.to_owned())),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Apple => "apple",
            Self::GitHub => "github",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthStart {
    pub provider: OAuthProviderId,
    pub state: String,
    pub state_hash: String,
    pub code_verifier: String,
    pub nonce: String,
    pub authorize_url: String,
    pub redirect_after: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderTokens {
    pub access_token: Option<String>,
    pub id_token: Option<String>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    pub expires_in: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ProviderSecrets {
    pub client_id: String,
    pub client_secret: Option<String>,
}

pub trait EnvReader {
    fn get_env(&self, name: &str) -> Result<Option<String>, AuthError>;
}

#[cfg(feature = "cloudflare")]
impl EnvReader for worker::Env {
    fn get_env(&self, name: &str) -> Result<Option<String>, AuthError> {
        match self.secret(name) {
            Ok(secret) => return Ok(Some(secret.to_string())),
            Err(_) => {}
        }

        match self.var(name) {
            Ok(var) => Ok(Some(var.to_string())),
            Err(_) => Ok(None),
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StaticEnv<'a>(pub &'a [(&'a str, &'a str)]);

impl EnvReader for StaticEnv<'_> {
    fn get_env(&self, name: &str) -> Result<Option<String>, AuthError> {
        Ok(self
            .0
            .iter()
            .find_map(|(key, value)| (*key == name).then(|| (*value).to_owned())))
    }
}

pub fn start_oauth(
    config: &AuthConfig,
    env: &impl EnvReader,
    provider: OAuthProviderId,
    redirect_after: Option<String>,
) -> Result<OAuthStart, AuthError> {
    let state = session::generate_token()?;
    let state_hash = session::hash_token(&state, None);
    let code_verifier = session::generate_token()?;
    let nonce = session::generate_token()?;
    let code_challenge = session::hash_token(&code_verifier, None);
    let redirect_uri = redirect_uri(config, provider)?;
    let secrets = provider_secrets(config, env, provider)?;
    let authorize_url = authorize_url(
        provider,
        &secrets.client_id,
        &redirect_uri,
        &state,
        &nonce,
        &code_challenge,
    );

    Ok(OAuthStart {
        provider,
        state,
        state_hash,
        code_verifier,
        nonce,
        authorize_url,
        redirect_after,
    })
}

pub fn redirect_uri(config: &AuthConfig, provider: OAuthProviderId) -> Result<String, AuthError> {
    let base_url = config.base_url.as_deref().ok_or(AuthError::MissingConfig)?;
    Ok(format!(
        "{}/auth/{}/callback",
        base_url.trim_end_matches('/'),
        provider.as_str()
    ))
}

pub fn provider_secrets(
    config: &AuthConfig,
    env: &impl EnvReader,
    provider: OAuthProviderId,
) -> Result<ProviderSecrets, AuthError> {
    let provider_config = config
        .provider_config(provider.as_str())
        .ok_or_else(|| AuthError::ProviderNotConfigured(provider.as_str().to_owned()))?;

    match (provider, provider_config) {
        (OAuthProviderId::Google, ProviderConfig::Google(config)) => google_secrets(env, config),
        (OAuthProviderId::Apple, ProviderConfig::Apple(config)) => apple_secrets(env, config),
        (OAuthProviderId::GitHub, ProviderConfig::GitHub(config)) => github_secrets(env, config),
        _ => Err(AuthError::ProviderNotConfigured(
            provider.as_str().to_owned(),
        )),
    }
}

fn google_secrets(
    env: &impl EnvReader,
    config: &GoogleProviderConfig,
) -> Result<ProviderSecrets, AuthError> {
    Ok(ProviderSecrets {
        client_id: required_env(
            env,
            "google",
            "web_client_id_env",
            config.web_client_id_env.as_deref(),
        )?,
        client_secret: Some(required_env(
            env,
            "google",
            "web_client_secret_env",
            config.web_client_secret_env.as_deref(),
        )?),
    })
}

fn apple_secrets(
    env: &impl EnvReader,
    config: &AppleProviderConfig,
) -> Result<ProviderSecrets, AuthError> {
    let client_id = required_env(
        env,
        "apple",
        "service_id_env",
        config.service_id_env.as_deref(),
    )?;
    let client_secret = match config.client_secret_env.as_deref() {
        Some(name) => Some(required_env(env, "apple", "client_secret_env", Some(name))?),
        None => Some(generate_apple_client_secret(env, config, &client_id)?),
    };

    Ok(ProviderSecrets {
        client_id,
        client_secret,
    })
}

fn generate_apple_client_secret(
    env: &impl EnvReader,
    config: &AppleProviderConfig,
    service_id: &str,
) -> Result<String, AuthError> {
    let team_id = required_env(env, "apple", "team_id_env", config.team_id_env.as_deref())?;
    let key_id = required_env(env, "apple", "key_id_env", config.key_id_env.as_deref())?;
    let private_key = required_env(
        env,
        "apple",
        "private_key_pkcs8_pem_env",
        config.private_key_pkcs8_pem_env.as_deref(),
    )?
    .replace("\\n", "\n");
    let now = session::now_unix();
    let header = serde_json::json!({
        "alg": "ES256",
        "kid": key_id,
        "typ": "JWT",
    });
    let claims = serde_json::json!({
        "iss": team_id,
        "iat": now,
        "exp": now + 60 * 60 * 24 * 180,
        "aud": "https://appleid.apple.com",
        "sub": service_id,
    });
    let signing_input = format!("{}.{}", base64_json(&header)?, base64_json(&claims)?,);
    let signing_key = SigningKey::from_pkcs8_pem(&private_key).map_err(|error| {
        AuthError::ProviderRequest(format!("invalid apple private key: {error}"))
    })?;
    let signature: p256::ecdsa::Signature = signing_key.sign(signing_input.as_bytes());
    let signature = base64ct::Base64UrlUnpadded::encode_string(&signature.to_bytes());

    Ok(format!("{signing_input}.{signature}"))
}

fn base64_json(value: &serde_json::Value) -> Result<String, AuthError> {
    Ok(base64ct::Base64UrlUnpadded::encode_string(
        serde_json::to_string(value)?.as_bytes(),
    ))
}

fn github_secrets(
    env: &impl EnvReader,
    config: &GitHubProviderConfig,
) -> Result<ProviderSecrets, AuthError> {
    Ok(ProviderSecrets {
        client_id: required_env(
            env,
            "github",
            "client_id_env",
            config.client_id_env.as_deref(),
        )?,
        client_secret: Some(required_env(
            env,
            "github",
            "client_secret_env",
            config.client_secret_env.as_deref(),
        )?),
    })
}

fn required_env(
    env: &impl EnvReader,
    provider: &'static str,
    setting: &'static str,
    env_name: Option<&str>,
) -> Result<String, AuthError> {
    let env_name = env_name.ok_or(AuthError::MissingProviderSetting { provider, setting })?;
    env.get_env(env_name)?
        .ok_or(AuthError::MissingProviderSetting { provider, setting })
}

fn authorize_url(
    provider: OAuthProviderId,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    nonce: &str,
    code_challenge: &str,
) -> String {
    let (base, scope) = match provider {
        OAuthProviderId::Google => (
            "https://accounts.google.com/o/oauth2/v2/auth",
            "openid email profile",
        ),
        OAuthProviderId::Apple => (
            "https://appleid.apple.com/auth/authorize",
            "openid email name",
        ),
        OAuthProviderId::GitHub => (
            "https://github.com/login/oauth/authorize",
            "read:user user:email",
        ),
    };

    let mut query = form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", scope)
        .append_pair("state", state);

    if !matches!(provider, OAuthProviderId::GitHub) {
        query
            .append_pair("nonce", nonce)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256");
    }

    format!("{base}?{}", query.finish())
}

pub fn form_body(pairs: &[(&str, &str)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

#[cfg(feature = "cloudflare")]
pub async fn exchange_code(
    config: &AuthConfig,
    env: &worker::Env,
    provider: OAuthProviderId,
    code: &str,
    code_verifier: &str,
) -> Result<ProviderTokens, AuthError> {
    let secrets = provider_secrets(config, env, provider)?;
    let redirect_uri = redirect_uri(config, provider)?;
    let token_url = match provider {
        OAuthProviderId::Google => "https://oauth2.googleapis.com/token",
        OAuthProviderId::Apple => "https://appleid.apple.com/auth/token",
        OAuthProviderId::GitHub => "https://github.com/login/oauth/access_token",
    };

    let mut pairs = vec![
        ("client_id", secrets.client_id.as_str()),
        ("code", code),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri.as_str()),
    ];
    if !matches!(provider, OAuthProviderId::GitHub) {
        pairs.push(("code_verifier", code_verifier));
    }
    if let Some(secret) = secrets.client_secret.as_deref() {
        pairs.push(("client_secret", secret));
    }

    let body = form_body(&pairs);
    let mut response = post_form(token_url, &body, &[("accept", "application/json")]).await?;
    if response.status_code() >= 400 {
        return Err(AuthError::ProviderRequest(response.text().await?));
    }
    response
        .json::<ProviderTokens>()
        .await
        .map_err(AuthError::from)
}

#[cfg(feature = "cloudflare")]
pub async fn fetch_identity(
    config: &AuthConfig,
    env: &worker::Env,
    provider: OAuthProviderId,
    tokens: &ProviderTokens,
    expected_nonce: Option<&str>,
) -> Result<ProviderIdentity, AuthError> {
    match provider {
        OAuthProviderId::Google => {
            validate_google_identity(config, env, tokens, expected_nonce).await
        }
        OAuthProviderId::Apple => {
            validate_apple_identity(config, env, tokens, expected_nonce).await
        }
        OAuthProviderId::GitHub => fetch_github_identity(tokens).await,
    }
}

#[cfg(feature = "cloudflare")]
pub async fn validate_native_identity(
    config: &AuthConfig,
    env: &worker::Env,
    provider: OAuthProviderId,
    id_token: &str,
    expected_nonce: Option<&str>,
) -> Result<ProviderIdentity, AuthError> {
    let tokens = ProviderTokens {
        access_token: None,
        id_token: Some(id_token.to_owned()),
        token_type: None,
        scope: None,
        expires_in: None,
    };

    match provider {
        OAuthProviderId::Google => {
            validate_google_identity(config, env, &tokens, expected_nonce).await
        }
        OAuthProviderId::Apple => {
            validate_apple_identity(config, env, &tokens, expected_nonce).await
        }
        OAuthProviderId::GitHub => Err(AuthError::UnsupportedProvider("github_native".into())),
    }
}

#[cfg(feature = "cloudflare")]
async fn validate_google_identity(
    config: &AuthConfig,
    env: &worker::Env,
    tokens: &ProviderTokens,
    expected_nonce: Option<&str>,
) -> Result<ProviderIdentity, AuthError> {
    let id_token = tokens
        .id_token
        .as_deref()
        .ok_or_else(|| AuthError::ProviderRequest("google response missing id_token".into()))?;
    let audiences = google_audiences(config, env)?;
    let claims = oidc::validate_rs256_id_token(
        "https://www.googleapis.com/oauth2/v3/certs",
        id_token,
        OidcValidation {
            issuer: "https://accounts.google.com",
            audiences: &audiences,
            expected_nonce,
        },
    )
    .await?;
    Ok(ProviderIdentity {
        provider: "google".to_owned(),
        provider_account_id: claims.sub,
        email: claims.email,
        email_verified: claims
            .email_verified
            .map(|value| value.as_bool())
            .unwrap_or(false),
        name: claims.name,
        avatar_url: claims.picture,
        raw_profile_json: None,
    })
}

#[cfg(feature = "cloudflare")]
async fn validate_apple_identity(
    config: &AuthConfig,
    env: &worker::Env,
    tokens: &ProviderTokens,
    expected_nonce: Option<&str>,
) -> Result<ProviderIdentity, AuthError> {
    let id_token = tokens
        .id_token
        .as_deref()
        .ok_or_else(|| AuthError::ProviderRequest("apple response missing id_token".into()))?;
    let audiences = apple_audiences(config, env)?;
    let claims = oidc::validate_rs256_id_token(
        "https://appleid.apple.com/auth/keys",
        id_token,
        OidcValidation {
            issuer: "https://appleid.apple.com",
            audiences: &audiences,
            expected_nonce,
        },
    )
    .await?;
    Ok(ProviderIdentity {
        provider: "apple".to_owned(),
        provider_account_id: claims.sub,
        email: claims.email,
        email_verified: claims
            .email_verified
            .map(|value| value.as_bool())
            .unwrap_or(false),
        name: claims.name,
        avatar_url: None,
        raw_profile_json: None,
    })
}

#[cfg(feature = "cloudflare")]
fn google_audiences(config: &AuthConfig, env: &worker::Env) -> Result<Vec<String>, AuthError> {
    let ProviderConfig::Google(google) = config
        .provider_config("google")
        .ok_or_else(|| AuthError::ProviderNotConfigured("google".into()))?
    else {
        return Err(AuthError::ProviderNotConfigured("google".into()));
    };

    let mut audiences = Vec::new();
    if let Some(name) = google.web_client_id_env.as_deref() {
        if let Some(value) = env.get_env(name)? {
            audiences.push(value);
        }
    }
    for name in &google.native_client_id_envs {
        if let Some(value) = env.get_env(name)? {
            audiences.push(value);
        }
    }
    if audiences.is_empty() {
        return Err(AuthError::MissingProviderSetting {
            provider: "google",
            setting: "web_client_id_env/native_client_id_env",
        });
    }
    Ok(audiences)
}

#[cfg(feature = "cloudflare")]
fn apple_audiences(config: &AuthConfig, env: &worker::Env) -> Result<Vec<String>, AuthError> {
    let ProviderConfig::Apple(apple) = config
        .provider_config("apple")
        .ok_or_else(|| AuthError::ProviderNotConfigured("apple".into()))?
    else {
        return Err(AuthError::ProviderNotConfigured("apple".into()));
    };

    let mut audiences = Vec::new();
    if let Some(name) = apple.service_id_env.as_deref() {
        if let Some(value) = env.get_env(name)? {
            audiences.push(value);
        }
    }
    for name in &apple.native_audience_envs {
        if let Some(value) = env.get_env(name)? {
            audiences.push(value);
        }
    }
    if audiences.is_empty() {
        return Err(AuthError::MissingProviderSetting {
            provider: "apple",
            setting: "service_id_env/native_audience_env",
        });
    }
    Ok(audiences)
}

#[cfg(feature = "cloudflare")]
async fn fetch_github_identity(tokens: &ProviderTokens) -> Result<ProviderIdentity, AuthError> {
    let access_token = tokens
        .access_token
        .as_deref()
        .ok_or_else(|| AuthError::ProviderRequest("github response missing access_token".into()))?;
    let auth = format!("Bearer {access_token}");
    let headers = [
        ("authorization", auth.as_str()),
        ("accept", "application/vnd.github+json"),
        ("user-agent", "comet-auth"),
    ];
    let mut user_response = get_json("https://api.github.com/user", &headers).await?;
    let user = user_response.json::<GitHubUser>().await?;
    let mut emails_response = get_json("https://api.github.com/user/emails", &headers).await?;
    let emails = emails_response
        .json::<Vec<GitHubEmail>>()
        .await
        .unwrap_or_default();
    let primary_email = emails
        .iter()
        .find(|email| email.primary && email.verified)
        .or_else(|| emails.iter().find(|email| email.verified));

    Ok(ProviderIdentity {
        provider: "github".to_owned(),
        provider_account_id: user.id.to_string(),
        email: primary_email
            .map(|email| email.email.clone())
            .or(user.email),
        email_verified: primary_email.is_some(),
        name: user.name.or(user.login),
        avatar_url: user.avatar_url,
        raw_profile_json: None,
    })
}

#[cfg(feature = "cloudflare")]
async fn post_form(
    url: &str,
    body: &str,
    headers: &[(&str, &str)],
) -> Result<worker::Response, AuthError> {
    let mut init = worker::RequestInit::new();
    init.with_method(worker::Method::Post);
    init.with_body(Some(worker::wasm_bindgen::JsValue::from_str(body)));
    init.headers
        .set("content-type", "application/x-www-form-urlencoded")?;
    for (name, value) in headers {
        init.headers.set(name, value)?;
    }
    let request = worker::Request::new_with_init(url, &init)?;
    worker::Fetch::Request(request)
        .send()
        .await
        .map_err(AuthError::from)
}

#[cfg(feature = "cloudflare")]
async fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<worker::Response, AuthError> {
    let init = worker::RequestInit::new();
    for (name, value) in headers {
        init.headers.set(name, value)?;
    }
    let request = worker::Request::new_with_init(url, &init)?;
    let mut response = worker::Fetch::Request(request).send().await?;
    if response.status_code() >= 400 {
        return Err(AuthError::ProviderRequest(response.text().await?));
    }
    Ok(response)
}

#[derive(Debug, Deserialize)]
struct GitHubUser {
    id: u64,
    login: Option<String>,
    name: Option<String>,
    email: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubEmail {
    email: String,
    primary: bool,
    verified: bool,
}

#[cfg(test)]
mod tests {
    use super::{OAuthProviderId, StaticEnv, form_body, start_oauth};
    use crate::AuthConfig;
    use crate::providers;

    #[test]
    fn start_google_oauth_builds_authorize_url() {
        let config = AuthConfig::default()
            .base_url("https://api.example.com")
            .provider(
                providers::Google::from_env()
                    .web_client_id_env("GOOGLE_ID")
                    .web_client_secret_env("GOOGLE_SECRET"),
            );
        let env = StaticEnv(&[("GOOGLE_ID", "gid"), ("GOOGLE_SECRET", "secret")]);

        let start = start_oauth(&config, &env, OAuthProviderId::Google, None).unwrap();

        assert!(
            start
                .authorize_url
                .starts_with("https://accounts.google.com/o/oauth2/v2/auth?")
        );
        assert!(start.authorize_url.contains("client_id=gid"));
        assert!(start.authorize_url.contains("code_challenge_method=S256"));
        assert!(
            start
                .authorize_url
                .contains("redirect_uri=https%3A%2F%2Fapi.example.com%2Fauth%2Fgoogle%2Fcallback")
        );
    }

    #[test]
    fn form_body_url_encodes_values() {
        assert_eq!(
            form_body(&[("scope", "openid email")]),
            "scope=openid+email"
        );
    }

    #[test]
    fn apple_requires_client_secret_or_signing_settings() {
        let config = AuthConfig::default()
            .base_url("https://api.example.com")
            .provider(providers::Apple::from_env().service_id_env("APPLE_SERVICE_ID"));
        let env = StaticEnv(&[("APPLE_SERVICE_ID", "app.example.web")]);

        let error = start_oauth(&config, &env, OAuthProviderId::Apple, None).unwrap_err();

        assert!(error.to_string().contains("team_id_env"));
    }
}
