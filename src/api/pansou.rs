use serde::{Deserialize, Serialize};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use tracing::info;
use crate::config::Config;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PanSouResult {
    pub name: String,
    pub url: String,
    pub size: String,
    pub source: String,
    pub pan_type: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Clone)]
pub struct PanSouClient {
    client: reqwest::Client,
    host: Option<String>,
    token: Option<String>,
}

impl PanSouClient {
    pub fn new(config: &Config, client: reqwest::Client) -> Self {
        if let Some(cfg) = &config.pansou {
            Self {
                client,
                host: Some(cfg.pansou_host.clone()),
                token: cfg.pansou_token.clone(),
            }
        } else {
            Self {
                client,
                host: None,
                token: None,
            }
        }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("User-Agent", HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36"));
        if let Some(token) = &self.token {
            let auth = format!("Bearer {}", token);
            if let Ok(val) = HeaderValue::from_str(&auth) {
                headers.insert(AUTHORIZATION, val);
            }
        }
        headers
    }

    pub async fn search(&self, keyword: &str) -> Result<Vec<PanSouResult>, Box<dyn std::error::Error + Send + Sync>> {
        let host = match &self.host {
            Some(h) => h,
            None => return Err("PanSou API not configured".into()),
        };

        let url = format!("{}/api/search", host.trim_end_matches('/'));
        let body = serde_json::json!({
            "kw": keyword,
            "res": "merge",
            "src": "all"
        });

        info!("PanSou search for keyword: {}", keyword);
        let res = self.client.post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await?;

        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("PanSou search failed: {}, body: {}", status, text).into());
        }

        let resp_json: serde_json::Value = serde_json::from_str(&text)?;
        let code = resp_json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 && code != 200 {
            let msg = resp_json.get("message").and_then(|m| m.as_str()).unwrap_or("Search failed");
            return Err(msg.into());
        }

        let mut results = Vec::new();
        if let Some(data) = resp_json.get("data") {
            if let Some(merged) = data.get("merged_by_type") {
                if let Some(obj) = merged.as_object() {
                    for (pan_type, links) in obj {
                        if let Some(arr) = links.as_array() {
                            for link in arr {
                                let note = link.get("note").and_then(|n| n.as_str()).unwrap_or("");
                                let clean_note = note.replace("\u{a0}", " ").trim().to_string();
                                results.push(PanSouResult {
                                    name: clean_note,
                                    url: link.get("url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
                                    size: link.get("size").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                                    source: link.get("source").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                                    pan_type: pan_type.clone(),
                                    password: link.get("password").and_then(|p| p.as_str()).unwrap_or("").to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    pub async fn search_sukebei(&self, keyword: &str) -> Result<Vec<PanSouResult>, Box<dyn std::error::Error + Send + Sync>> {
        let encoded_keyword = percent_encoding::utf8_percent_encode(keyword, percent_encoding::NON_ALPHANUMERIC).to_string();
        let url = format!("https://sukebei.nyaa.si/?page=rss&q={}", encoded_keyword);

        info!("Sukebei search for keyword: {}", keyword);
        let res = self.client.get(&url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36")
            .send()
            .await?;

        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("Sukebei search failed: {}", status).into());
        }

        let mut results = Vec::new();
        let parts: Vec<&str> = text.split("<item>").collect();
        if parts.len() > 1 {
            for part in &parts[1..] {
                if let Some(end_idx) = part.find("</item>") {
                    let item_str = &part[..end_idx];
                    
                    let title = extract_tag_content(item_str, "title").unwrap_or_default();
                    let info_hash = extract_tag_content(item_str, "nyaa:infoHash").unwrap_or_default();
                    let size = extract_tag_content(item_str, "nyaa:size").unwrap_or_default();
                    
                    if !title.is_empty() && !info_hash.is_empty() {
                        let decoded_title = decode_xml_entities(&title);
                        let encoded_title = percent_encoding::utf8_percent_encode(&decoded_title, percent_encoding::NON_ALPHANUMERIC).to_string();
                        let magnet_url = format!("magnet:?xt=urn:btih:{}&dn={}", info_hash.trim(), encoded_title);
                        
                        results.push(PanSouResult {
                            name: decoded_title,
                            url: magnet_url,
                            size: decode_xml_entities(&size),
                            source: "Sukebei".to_string(),
                            pan_type: "magnet".to_string(),
                            password: "".to_string(),
                        });
                    }
                }
            }
        }

        Ok(results)
    }
}

fn extract_tag_content(item_str: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{}", tag);
    let end_tag = format!("</{}>", tag);
    
    let start_idx = item_str.find(&start_tag)?;
    let content_start = item_str[start_idx..].find('>')? + start_idx + 1;
    let end_idx = item_str[content_start..].find(&end_tag)? + content_start;
    
    Some(item_str[content_start..end_idx].to_string())
}

fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
     .replace("&lt;", "<")
     .replace("&gt;", ">")
     .replace("&quot;", "\"")
     .replace("&apos;", "'")
}
