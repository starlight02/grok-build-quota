//! Shared reqwest client for outbound probes (timeout, UA, proxy).

use std::time::Duration;

static CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(build_client);

pub fn shared_client() -> &'static reqwest::Client {
    &CLIENT
}

fn build_client() -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .user_agent("grok-cli/0.2.93")
        .redirect(reqwest::redirect::Policy::limited(5));

    if let Ok(proxy) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("HTTP_PROXY"))
        && let Ok(p) = reqwest::Proxy::all(proxy)
    {
        builder = builder.proxy(p);
    }

    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}
