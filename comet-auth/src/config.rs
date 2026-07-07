use crate::{DEFAULT_SESSION_COOKIE, DEFAULT_SESSION_TTL_SECONDS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieSameSite {
    Strict,
    Lax,
    None,
}

impl From<CookieSameSite> for rocket::http::SameSite {
    fn from(value: CookieSameSite) -> Self {
        match value {
            CookieSameSite::Strict => rocket::http::SameSite::Strict,
            CookieSameSite::Lax => rocket::http::SameSite::Lax,
            CookieSameSite::None => rocket::http::SameSite::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub base_url: Option<String>,
    pub session_cookie: String,
    pub session_ttl_seconds: u64,
    pub authorization_claims_cache_ttl_seconds: u64,
    pub same_site: CookieSameSite,
    pub secure_cookies: bool,
    pub token_pepper_env: Option<String>,
    pub providers: Vec<ProviderConfig>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            session_cookie: DEFAULT_SESSION_COOKIE.to_owned(),
            session_ttl_seconds: DEFAULT_SESSION_TTL_SECONDS,
            authorization_claims_cache_ttl_seconds: 60,
            same_site: CookieSameSite::Lax,
            secure_cookies: true,
            token_pepper_env: Some("COMET_AUTH_TOKEN_PEPPER".to_owned()),
            providers: Vec::new(),
        }
    }
}

impl AuthConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_env() -> Self {
        Self::default()
    }

    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn session_cookie(mut self, name: impl Into<String>) -> Self {
        self.session_cookie = name.into();
        self
    }

    pub fn session_ttl_seconds(mut self, ttl: u64) -> Self {
        self.session_ttl_seconds = ttl;
        self
    }

    pub fn authorization_claims_cache_ttl_seconds(mut self, ttl: u64) -> Self {
        self.authorization_claims_cache_ttl_seconds = ttl;
        self
    }

    pub fn same_site(mut self, same_site: CookieSameSite) -> Self {
        self.same_site = same_site;
        self
    }

    pub fn secure_cookies(mut self, secure: bool) -> Self {
        self.secure_cookies = secure;
        self
    }

    pub fn token_pepper_env(mut self, name: impl Into<String>) -> Self {
        self.token_pepper_env = Some(name.into());
        self
    }

    pub fn provider(mut self, provider: impl Into<ProviderConfig>) -> Self {
        self.providers.push(provider.into());
        self
    }

    pub fn provider_config(&self, provider: &str) -> Option<&ProviderConfig> {
        self.providers
            .iter()
            .find(|config| config.provider_id() == Some(provider))
    }
}

#[derive(Debug, Clone)]
pub enum ProviderConfig {
    Google(GoogleProviderConfig),
    Apple(AppleProviderConfig),
    GitHub(GitHubProviderConfig),
}

impl ProviderConfig {
    pub fn provider_id(&self) -> Option<&'static str> {
        match self {
            Self::Google(_) => Some("google"),
            Self::Apple(_) => Some("apple"),
            Self::GitHub(_) => Some("github"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GoogleProviderConfig {
    pub web_client_id_env: Option<String>,
    pub web_client_secret_env: Option<String>,
    pub native_client_id_envs: Vec<String>,
}

impl GoogleProviderConfig {
    pub fn from_env() -> Self {
        Self::default()
    }

    pub fn web_client_id_env(mut self, name: impl Into<String>) -> Self {
        self.web_client_id_env = Some(name.into());
        self
    }

    pub fn web_client_secret_env(mut self, name: impl Into<String>) -> Self {
        self.web_client_secret_env = Some(name.into());
        self
    }

    pub fn native_client_id_env(mut self, name: impl Into<String>) -> Self {
        self.native_client_id_envs.push(name.into());
        self
    }
}

impl From<GoogleProviderConfig> for ProviderConfig {
    fn from(value: GoogleProviderConfig) -> Self {
        Self::Google(value)
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppleProviderConfig {
    pub service_id_env: Option<String>,
    pub team_id_env: Option<String>,
    pub key_id_env: Option<String>,
    pub private_key_pkcs8_pem_env: Option<String>,
    pub client_secret_env: Option<String>,
    pub native_audience_envs: Vec<String>,
}

impl AppleProviderConfig {
    pub fn from_env() -> Self {
        Self::default()
    }

    pub fn service_id_env(mut self, name: impl Into<String>) -> Self {
        self.service_id_env = Some(name.into());
        self
    }

    pub fn team_id_env(mut self, name: impl Into<String>) -> Self {
        self.team_id_env = Some(name.into());
        self
    }

    pub fn key_id_env(mut self, name: impl Into<String>) -> Self {
        self.key_id_env = Some(name.into());
        self
    }

    pub fn private_key_pkcs8_pem_env(mut self, name: impl Into<String>) -> Self {
        self.private_key_pkcs8_pem_env = Some(name.into());
        self
    }

    pub fn client_secret_env(mut self, name: impl Into<String>) -> Self {
        self.client_secret_env = Some(name.into());
        self
    }

    pub fn native_audience_env(mut self, name: impl Into<String>) -> Self {
        self.native_audience_envs.push(name.into());
        self
    }
}

impl From<AppleProviderConfig> for ProviderConfig {
    fn from(value: AppleProviderConfig) -> Self {
        Self::Apple(value)
    }
}

#[derive(Debug, Clone, Default)]
pub struct GitHubProviderConfig {
    pub client_id_env: Option<String>,
    pub client_secret_env: Option<String>,
}

impl GitHubProviderConfig {
    pub fn from_env() -> Self {
        Self::default()
    }

    pub fn client_id_env(mut self, name: impl Into<String>) -> Self {
        self.client_id_env = Some(name.into());
        self
    }

    pub fn client_secret_env(mut self, name: impl Into<String>) -> Self {
        self.client_secret_env = Some(name.into());
        self
    }
}

impl From<GitHubProviderConfig> for ProviderConfig {
    fn from(value: GitHubProviderConfig) -> Self {
        Self::GitHub(value)
    }
}

pub mod providers {
    pub type Google = super::GoogleProviderConfig;
    pub type Apple = super::AppleProviderConfig;
    pub type GitHub = super::GitHubProviderConfig;
}
