use serde::{Deserialize, Serialize};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use tracing::warn;
use crate::config::Config;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OpenListAPIResponse<T> {
    pub code: i32,
    pub message: String,
    pub data: Option<T>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FileItem {
    pub name: String,
    pub size: i64,
    pub is_dir: bool,
    pub modified: Option<String>,
    #[serde(rename = "type")]
    pub item_type: i32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FileInfo {
    pub name: String,
    pub size: i64,
    pub is_dir: bool,
    pub modified: Option<String>,
    pub sign: Option<String>,
    pub thumb: Option<String>,
    #[serde(rename = "type")]
    pub item_type: i32,
    pub raw_url: Option<String>,
    pub readme: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StorageInfo {
    pub id: i64,
    pub mount_path: Option<String>,
    pub remark: Option<String>,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TaskInfo {
    pub id: String,
    pub name: String,
    pub status: Option<String>,
    #[serde(default)]
    pub progress: f64,
    #[serde(default)]
    pub total_bytes: i64,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct OpenListClient {
    client: reqwest::Client,
    host: String,
    token: String,
}

impl OpenListClient {
    pub fn new(config: &Config, client: reqwest::Client) -> Self {
        Self {
            client,
            host: config.openlist.openlist_host.clone(),
            token: config.openlist.openlist_token.clone(),
        }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        match HeaderValue::from_str(&self.token) {
            Ok(val) => { headers.insert(AUTHORIZATION, val); }
            Err(_) => warn!("OpenList token contains invalid characters; Authorization header left empty"),
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("User-Agent", HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36"));
        headers
    }

    pub async fn login(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Just verify connection by hitting /api/public/offline_download_tools
        let url = format!("{}/api/public/offline_download_tools", self.host.trim_end_matches('/'));
        let res = self.client.get(&url)
            .headers(self.headers())
            .send()
            .await?;
        if !res.status().is_success() {
            return Err(format!("Login check failed with status: {}", res.status()).into());
        }
        Ok(())
    }

    /// List a directory without forcing OpenList to re-index the backend.
    /// Use this for interactive browsing so每次点目录不会把 115/阿里云等云盘
    /// 全量刷新一遍（慢且易触发风控）。
    pub async fn fs_list(&self, path: &str) -> Result<Vec<FileItem>, Box<dyn std::error::Error + Send + Sync>> {
        self.fs_list_inner(path, false).await
    }

    /// List a directory and force OpenList to refresh its cache for that path.
    /// Only the explicit `/refresh` command should need this.
    pub async fn fs_list_refresh(&self, path: &str) -> Result<Vec<FileItem>, Box<dyn std::error::Error + Send + Sync>> {
        self.fs_list_inner(path, true).await
    }

    async fn fs_list_inner(&self, path: &str, refresh: bool) -> Result<Vec<FileItem>, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/fs/list", self.host.trim_end_matches('/'));
        let body = serde_json::json!({
            "path": path,
            "page": 1,
            "per_page": 0,
            "refresh": refresh
        });
        
        let res = self.client.post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await?;
            
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("fs_list failed: {}, body: {}", status, text).into());
        }

        // The response might contain {"data": {"content": [...]}} or {"data": [...]}
        // We parse as serde_json::Value first to handle both gracefully
        let val: OpenListAPIResponse<serde_json::Value> = serde_json::from_str(&text)?;
        if val.code != 200 {
            return Err(format!("fs_list API error: {}", val.message).into());
        }

        if let Some(data) = val.data {
            if data.is_array() {
                let items: Vec<FileItem> = serde_json::from_value(data)?;
                return Ok(items);
            } else if let Some(content) = data.get("content") {
                if content.is_array() {
                    let items: Vec<FileItem> = serde_json::from_value(content.clone())?;
                    return Ok(items);
                }
            }
        }

        Ok(vec![])
    }

    pub async fn fs_get(&self, path: &str) -> Result<FileInfo, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/fs/get", self.host.trim_end_matches('/'));
        let body = serde_json::json!({ "path": path });
        
        let res = self.client.post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await?;
            
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("fs_get failed: {}, body: {}", status, text).into());
        }

        let resp: OpenListAPIResponse<FileInfo> = serde_json::from_str(&text)?;
        if resp.code != 200 {
            return Err(format!("fs_get API error: {}", resp.message).into());
        }
        
        resp.data.ok_or_else(|| "No file info in response".into())
    }

    pub async fn storage_list(&self) -> Result<Vec<StorageInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/admin/storage/list", self.host.trim_end_matches('/'));
        
        let res = self.client.get(&url)
            .headers(self.headers())
            .send()
            .await?;
            
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("storage_list failed: {}, body: {}", status, text).into());
        }

        let val: OpenListAPIResponse<serde_json::Value> = serde_json::from_str(&text)?;
        if val.code != 200 {
            return Err(format!("storage_list API error: {}", val.message).into());
        }

        if let Some(data) = val.data {
            if data.is_array() {
                let items: Vec<StorageInfo> = serde_json::from_value(data)?;
                return Ok(items);
            } else if let Some(content) = data.get("content") {
                if content.is_array() {
                    let items: Vec<StorageInfo> = serde_json::from_value(content.clone())?;
                    return Ok(items);
                }
            }
        }

        Ok(vec![])
    }

    pub async fn get_offline_download_tools(&self) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/public/offline_download_tools", self.host.trim_end_matches('/'));
        
        let res = self.client.get(&url)
            .headers(self.headers())
            .send()
            .await?;
            
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("get_offline_download_tools failed: {}, body: {}", status, text).into());
        }

        let resp: OpenListAPIResponse<Vec<String>> = serde_json::from_str(&text)?;
        if resp.code != 200 {
            return Err(format!("get_offline_download_tools API error: {}", resp.message).into());
        }
        
        Ok(resp.data.unwrap_or_default())
    }

    pub async fn add_offline_download(&self, urls: Vec<String>, tool: &str, path: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/fs/add_offline_download", self.host.trim_end_matches('/'));
        let body = serde_json::json!({
            "urls": urls,
            "tool": tool,
            "path": path,
            "delete_policy": "0"
        });
        
        let res = self.client.post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await?;
            
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("add_offline_download failed: {}, body: {}", status, text).into());
        }

        let resp: OpenListAPIResponse<serde_json::Value> = serde_json::from_str(&text)?;
        if resp.code != 200 {
            return Err(format!("add_offline_download API error: {}", resp.message).into());
        }
        
        Ok(())
    }

    pub async fn get_offline_download_undone_task(&self) -> Result<Vec<TaskInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/admin/task/offline_download/undone", self.host.trim_end_matches('/'));
        
        let res = self.client.get(&url)
            .headers(self.headers())
            .send()
            .await?;
            
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("get_offline_download_undone_task failed: {}, body: {}", status, text).into());
        }

        let resp: OpenListAPIResponse<Vec<TaskInfo>> = serde_json::from_str(&text)?;
        if resp.code != 200 {
            return Err(format!("get_offline_download_undone_task API error: {}", resp.message).into());
        }
        
        Ok(resp.data.unwrap_or_default())
    }

    pub async fn get_offline_download_done_task(&self) -> Result<Vec<TaskInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/admin/task/offline_download/done", self.host.trim_end_matches('/'));
        
        let res = self.client.get(&url)
            .headers(self.headers())
            .send()
            .await?;
            
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("get_offline_download_done_task failed: {}, body: {}", status, text).into());
        }

        let resp: OpenListAPIResponse<Vec<TaskInfo>> = serde_json::from_str(&text)?;
        if resp.code != 200 {
            return Err(format!("get_offline_download_done_task API error: {}", resp.message).into());
        }
        
        Ok(resp.data.unwrap_or_default())
    }

    pub async fn fs_remove(&self, dir_path: &str, names: Vec<String>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/fs/remove", self.host.trim_end_matches('/'));
        let body = serde_json::json!({
            "dir": dir_path,
            "names": names
        });
        
        let res = self.client.post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await?;
            
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("fs_remove failed: {}, body: {}", status, text).into());
        }

        let resp: OpenListAPIResponse<serde_json::Value> = serde_json::from_str(&text)?;
        if resp.code != 200 {
            return Err(format!("fs_remove API error: {}", resp.message).into());
        }
        
        Ok(())
    }

    pub async fn fs_mkdir(&self, path: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/fs/mkdir", self.host.trim_end_matches('/'));
        let body = serde_json::json!({ "path": path });
        
        let res = self.client.post(&url)
            .headers(self.headers())
            .json(&body)
            .send()
            .await?;
            
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("fs_mkdir failed: {}, body: {}", status, text).into());
        }

        let resp: OpenListAPIResponse<serde_json::Value> = serde_json::from_str(&text)?;
        if resp.code != 200 {
            return Err(format!("fs_mkdir API error: {}", resp.message).into());
        }
        
        Ok(())
    }

    pub async fn fs_put_bytes(&self, bytes: Vec<u8>, remote_path: &str, file_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/api/fs/put", self.host.trim_end_matches('/'));
        
        let remote_full_path = format!("{}/{}", remote_path.trim_end_matches('/'), file_name.trim_start_matches('/'));
        let encoded_path = percent_encoding::utf8_percent_encode(&remote_full_path, percent_encoding::NON_ALPHANUMERIC).to_string();
        
        let mut headers = self.headers();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
        headers.insert("As-Task", HeaderValue::from_static("false"));
        headers.insert("File-Path", HeaderValue::from_str(&encoded_path)?);
        // Content-Length is set automatically by reqwest from the body.

        let res = self.client.put(&url)
            .headers(headers)
            .body(bytes)
            .send()
            .await?;
            
        let status = res.status();
        let text = res.text().await?;
        if !status.is_success() {
            return Err(format!("fs_put_bytes failed: {}, body: {}", status, text).into());
        }

        let resp: OpenListAPIResponse<serde_json::Value> = serde_json::from_str(&text)?;
        if resp.code != 200 {
            return Err(format!("fs_put_bytes API error: {}", resp.message).into());
        }
        
        Ok(())
    }
}
