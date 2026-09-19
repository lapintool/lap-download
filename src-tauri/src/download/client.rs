use crate::settings::Settings;
use reqwest::Client;
use std::time::Duration;

pub fn build_client(settings: &Settings) -> Result<Client, String> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(0))
        .connect_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(settings.threads.max(1) as usize)
        .user_agent("lapdw/0.1")
        .redirect(reqwest::redirect::Policy::limited(10));

    if settings.use_proxy {
        if settings.proxy.is_empty() {
            return Err("proxy enabled but proxy address is empty".into());
        }
        let proxy = reqwest::Proxy::all(&settings.proxy)
            .map_err(|e| format!("invalid proxy {}: {e}", settings.proxy))?;
        builder = builder.proxy(proxy);
    }

    builder.build().map_err(|e| format!("http client: {e}"))
}

pub fn apply_auth(req: reqwest::RequestBuilder, settings: &Settings) -> reqwest::RequestBuilder {
    let mut req = req;
    if let Some(token) = settings.effective_token() {
        req = req.header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {token}"),
        );
    }
    if !settings.cookie.is_empty() {
        req = req.header(reqwest::header::COOKIE, &settings.cookie);
    }
    req
}
