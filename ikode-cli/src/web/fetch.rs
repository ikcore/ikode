//! URL → readable text pipeline for `web_fetch`: guarded HTTPS fetch (size cap,
//! bounded redirects, SSRF host checks on every hop), pure extraction, then the
//! JavaScript fallbacks — hydration-payload harvesting first, a headless render
//! via the user's own Chrome (when discovered at startup) second.

use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};

use super::chrome;
use super::extract::{self, truncate_chars};
use super::guard;
use super::FetchedPage;

/// Hard cap on downloaded bytes: a huge page must not blow the heap.
pub const MAX_FETCH_BYTES: usize = 2 * 1024 * 1024;
/// Cap on the readable text handed back to the model.
pub const MAX_PAGE_CHARS: usize = 8_000;
/// Wall-clock allowance for a headless Chrome render.
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(30);

/// Fetch `url` and reduce it to readable text. `browser` enables the headless
/// JavaScript fallback for detected SPA shells.
pub async fn fetch_page(
    client: &reqwest::Client,
    url: &str,
    browser: Option<&Path>,
) -> Result<FetchedPage> {
    let parsed = reqwest::Url::parse(url).with_context(|| format!("invalid URL '{url}'"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        bail!("only http(s) URLs can be fetched");
    }
    let host = parsed.host_str().ok_or_else(|| anyhow!("URL has no host"))?;
    if let Some(reason) = guard::blocked_host_reason(host) {
        bail!("refusing to fetch '{host}': {reason}");
    }

    let response = client
        .get(parsed)
        .send()
        .await
        .with_context(|| format!("fetch {url}"))?;
    let status = response.status();
    if !status.is_success() {
        bail!("fetch failed with HTTP {status}");
    }
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("text/html")
        .to_ascii_lowercase();
    if !is_readable_content_type(&content_type) {
        bail!("unsupported content type '{content_type}' — only HTML/text/JSON pages are readable");
    }

    let (body, byte_capped) = read_capped(response, MAX_FETCH_BYTES).await?;
    let body = String::from_utf8_lossy(&body).into_owned();

    // Plain text / JSON: no HTML pass needed.
    if !content_type.contains("html") {
        let (text, cut) = truncate_chars(body.trim(), MAX_PAGE_CHARS);
        return Ok(FetchedPage {
            url: final_url,
            title: None,
            text,
            truncated: cut || byte_capped,
            via: "static",
        });
    }

    let extracted = extract::extract(&body);
    if !extracted.js_shell {
        let (text, cut) = truncate_chars(&extracted.text, MAX_PAGE_CHARS);
        return Ok(FetchedPage {
            url: final_url,
            title: extracted.title,
            text,
            truncated: cut || byte_capped,
            via: "static",
        });
    }

    // SPA shell. Cheapest first: content embedded as SSR/hydration JSON.
    if let Some(harvested) = extract::harvest_hydration_json(&body, 40, MAX_PAGE_CHARS) {
        return Ok(FetchedPage {
            url: final_url,
            title: extracted.title,
            text: harvested,
            truncated: byte_capped,
            via: "hydration-json",
        });
    }

    // Then the real thing: render in the user's own browser, headlessly.
    if let Some(browser) = browser {
        let dom = chrome::render_dom(browser, &final_url, RENDER_TIMEOUT).await?;
        let rendered = extract::extract(&dom);
        if !rendered.text.trim().is_empty() {
            let (text, cut) = truncate_chars(&rendered.text, MAX_PAGE_CHARS);
            return Ok(FetchedPage {
                url: final_url,
                title: rendered.title.or(extracted.title),
                text,
                truncated: cut,
                via: "chrome",
            });
        }
    }

    Ok(FetchedPage {
        url: final_url,
        title: extracted.title,
        text: extracted.text,
        truncated: byte_capped,
        via: "js-shell",
    })
}

/// Accept only content types we can turn into readable text.
pub fn is_readable_content_type(content_type: &str) -> bool {
    let essence = content_type.split(';').next().unwrap_or("").trim();
    essence.starts_with("text/")
        || matches!(
            essence,
            "application/json"
                | "application/xhtml+xml"
                | "application/xml"
                | "application/rss+xml"
                | "application/atom+xml"
                | ""
        )
}

/// Drain the body up to `cap` bytes, then stop reading (flagging the cut) so a
/// multi-gigabyte response costs at most `cap` memory and bandwidth.
async fn read_capped(mut response: reqwest::Response, cap: usize) -> Result<(Vec<u8>, bool)> {
    let mut body: Vec<u8> = Vec::with_capacity(64 * 1024);
    while let Some(chunk) = response.chunk().await.context("read response body")? {
        if body.len() + chunk.len() > cap {
            body.extend_from_slice(&chunk[..cap - body.len()]);
            return Ok((body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_content_types_accept_text_and_json_but_not_binaries() {
        assert!(is_readable_content_type("text/html; charset=utf-8"));
        assert!(is_readable_content_type("text/plain"));
        assert!(is_readable_content_type("application/json"));
        assert!(is_readable_content_type("application/xhtml+xml"));
        assert!(is_readable_content_type(""));
        assert!(!is_readable_content_type("application/pdf"));
        assert!(!is_readable_content_type("image/png"));
        assert!(!is_readable_content_type("application/octet-stream"));
    }
}
