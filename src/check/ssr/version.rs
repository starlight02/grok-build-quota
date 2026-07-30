//! Resolve the current Grok CLI stable version from xAI's installer channel pointer.

use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use futures::lock::Mutex;

const PRIMARY_STABLE_URL: &str = "https://x.ai/cli/stable";
const FALLBACK_STABLE_URL: &str =
    "https://storage.googleapis.com/grok-build-public-artifacts/cli/stable";
const FALLBACK_VERSION: &str = "0.2.93";
const CACHE_TTL: Duration = Duration::from_secs(60 * 60);

#[derive(Clone)]
struct CachedVersion {
    value: String,
    fetched_at: Instant,
}

static VERSION: LazyLock<Mutex<Option<CachedVersion>>> = LazyLock::new(|| Mutex::new(None));

fn parse_version(body: &str) -> Option<String> {
    let version = body.lines().next()?.trim();
    let mut parts = version.splitn(2, '-');
    let core = parts.next()?;
    let suffix = parts.next();
    let nums = core.split('.').collect::<Vec<_>>();
    if nums.len() != 3
        || nums
            .iter()
            .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    if suffix.is_some_and(|value| {
        value.is_empty()
            || !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_'))
    }) {
        return None;
    }
    Some(version.to_string())
}

async fn fetch_pointer(client: &reqwest::Client, url: &str) -> Option<String> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    parse_version(&response.text().await.ok()?)
}

pub async fn current_cli_version(client: &reqwest::Client) -> String {
    // 持锁跨越单次网络请求，确保批量检测首轮只查询一次稳定版指针。
    let mut cached = VERSION.lock().await;
    if let Some(value) = cached
        .as_ref()
        .filter(|value| value.fetched_at.elapsed() < CACHE_TTL)
    {
        return value.value.clone();
    }

    let stale = cached.as_ref().map(|value| value.value.clone());
    let resolved = resolve_cli_version(client)
        .await
        .or(stale)
        .unwrap_or_else(|| FALLBACK_VERSION.to_string());
    *cached = Some(CachedVersion {
        value: resolved.clone(),
        fetched_at: Instant::now(),
    });
    resolved
}

async fn resolve_cli_version(client: &reqwest::Client) -> Option<String> {
    if let Some(version) = fetch_pointer(client, PRIMARY_STABLE_URL).await {
        return Some(version);
    }
    fetch_pointer(client, FALLBACK_STABLE_URL).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_channel_pointer_version() {
        assert_eq!(parse_version("0.2.104\n"), Some("0.2.104".into()));
        assert_eq!(
            parse_version("0.3.0-alpha.2\r\n"),
            Some("0.3.0-alpha.2".into())
        );
        assert!(parse_version("latest").is_none());
        assert!(parse_version("0.2").is_none());
        assert!(parse_version("0.2.3 bad").is_none());
    }
}
