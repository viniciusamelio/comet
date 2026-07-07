use rocket::http::{Cookie, SameSite};
use rocket::time::Duration;

use crate::AuthConfig;

pub fn build_session_cookie(config: &AuthConfig, token: impl Into<String>) -> Cookie<'static> {
    Cookie::build((config.session_cookie.clone(), token.into()))
        .http_only(true)
        .secure(config.secure_cookies)
        .same_site(SameSite::from(config.same_site))
        .path("/")
        .max_age(Duration::seconds(config.session_ttl_seconds as i64))
        .build()
}

pub fn remove_session_cookie(config: &AuthConfig) -> Cookie<'static> {
    Cookie::build(config.session_cookie.clone())
        .path("/")
        .secure(config.secure_cookies)
        .same_site(SameSite::from(config.same_site))
        .build()
}

#[cfg(test)]
mod tests {
    use rocket::http::SameSite;

    use crate::{AuthConfig, build_session_cookie};

    #[test]
    fn session_cookie_is_http_only_secure_and_lax_by_default() {
        let cookie = build_session_cookie(&AuthConfig::default(), "token");

        assert_eq!(cookie.name(), "__Host-comet_session");
        assert_eq!(cookie.value(), "token");
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    }
}
