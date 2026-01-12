//! Storage tools for uploading and sharing files via cloud storage.
//!
//! Uses Supabase Storage when SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY are set.
//! Files are uploaded to the appropriate bucket and returned as public URLs with
//! structured metadata for rich rendering in the dashboard.

use std::path::Path;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::Tool;

/// Get Supabase configuration from environment.
fn get_supabase_config() -> Option<(String, String)> {
    let url = std::env::var("SUPABASE_URL").ok()?;
    let key = std::env::var("SUPABASE_SERVICE_ROLE_KEY").ok()?;

    if url.is_empty() || key.is_empty() {
        return None;
    }

    Some((url, key))
}

/// Determine content type from file extension.
fn content_type_from_extension(extension: &str) -> &'static str {
    match extension.to_lowercase().as_str() {
        // Images
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        // Documents
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        // Archives
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" | "gzip" => "application/gzip",
        "rar" => "application/vnd.rar",
        "7z" => "application/x-7z-compressed",
        // Code/text
        "txt" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "ts" => "text/typescript",
        "py" => "text/x-python",
        "rs" => "text/x-rust",
        "go" => "text/x-go",
        "java" => "text/x-java",
        "c" | "h" => "text/x-c",
        "cpp" | "hpp" | "cc" => "text/x-c++",
        "sh" | "bash" => "text/x-shellscript",
        "yaml" | "yml" => "text/yaml",
        "toml" => "text/x-toml",
        "csv" => "text/csv",
        // Audio/video
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        // Default
        _ => "application/octet-stream",
    }
}

/// Determine the file kind from content type (for UI rendering hints).
fn file_kind_from_content_type(content_type: &str) -> &'static str {
    if content_type.starts_with("image/") {
        "image"
    } else if content_type.starts_with("text/")
        || content_type.contains("json")
        || content_type.contains("xml")
    {
        "code"
    } else if content_type.contains("pdf")
        || content_type.contains("document")
        || content_type.contains("word")
        || content_type.contains("sheet")
        || content_type.contains("presentation")
    {
        "document"
    } else if content_type.contains("zip")
        || content_type.contains("tar")
        || content_type.contains("gzip")
        || content_type.contains("compress")
        || content_type.contains("rar")
        || content_type.contains("7z")
    {
        "archive"
    } else {
        "other"
    }
}

/// Share a file by uploading it to cloud storage and returning a public URL.
///
/// This tool uploads any file type to Supabase Storage and returns structured
/// metadata that the dashboard uses for rich rendering:
/// - Images are displayed inline
/// - Documents, archives, and other files show as download cards
pub struct ShareFile;

#[async_trait]
impl Tool for ShareFile {
    fn name(&self) -> &str {
        "share_file"
    }

    fn description(&self) -> &str {
        "Upload a file to cloud storage and get a public URL for sharing.\n\n\
        Supports any file type:\n\
        - Images (PNG, JPEG, GIF, WebP, SVG) - displayed inline in chat\n\
        - Documents (PDF, Word, Excel, etc.) - shown as download card\n\
        - Archives (ZIP, TAR, etc.) - shown as download card\n\
        - Code/text files - shown as download card\n\
        - Any other file type\n\n\
        Returns structured metadata that the dashboard uses to render the file appropriately.\n\n\
        Example:\n\
        1. share_file{path: \"/path/to/screenshot.png\", title: \"Screenshot\"}\n\
        2. The dashboard will automatically display the image inline"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the local file to upload (absolute or relative to working directory)"
                },
                "title": {
                    "type": "string",
                    "description": "Optional display title for the file (defaults to filename)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, args: Value, working_dir: &Path) -> anyhow::Result<String> {
        let (supabase_url, service_role_key) = get_supabase_config()
            .ok_or_else(|| anyhow::anyhow!(
                "Supabase not configured. Set SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY environment variables."
            ))?;

        let path_arg = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

        // Resolve path (relative to working_dir or absolute)
        let file_path = if Path::new(path_arg).is_absolute() {
            std::path::PathBuf::from(path_arg)
        } else {
            working_dir.join(path_arg)
        };

        // Verify file exists
        if !file_path.exists() {
            return Err(anyhow::anyhow!("File not found: {}", file_path.display()));
        }

        // Get file metadata
        let metadata = std::fs::metadata(&file_path)?;
        let size_bytes = metadata.len();

        // Get filename and extension
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let extension = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin")
            .to_lowercase();

        // Use provided title or default to filename
        let title = args["title"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| file_name.to_string());

        // Determine content type
        let content_type = content_type_from_extension(&extension);
        let file_kind = file_kind_from_content_type(content_type);

        // Read file content
        let content = std::fs::read(&file_path)?;

        // Generate a unique path for the uploaded file
        let file_id = uuid::Uuid::new_v4();
        let upload_path = format!("{}.{}", file_id, extension);

        // Determine bucket based on file kind
        let bucket = if file_kind == "image" {
            "images"
        } else {
            "files"
        };

        tracing::info!(
            local_path = %file_path.display(),
            upload_path = %upload_path,
            bucket = %bucket,
            size = size_bytes,
            content_type = %content_type,
            kind = %file_kind,
            "Uploading file to Supabase Storage"
        );

        // Upload to Supabase Storage
        let storage_url = format!(
            "{}/storage/v1/object/{}/{}",
            supabase_url.trim_end_matches('/'),
            bucket,
            upload_path
        );

        let client = reqwest::Client::new();
        let resp = client
            .post(&storage_url)
            .header("apikey", &service_role_key)
            .header("Authorization", format!("Bearer {}", service_role_key))
            .header("Content-Type", content_type)
            .header("x-upsert", "true")
            .body(content)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Failed to upload file: {} - {}",
                status,
                error_text
            ));
        }

        // Construct public URL
        let public_url = format!(
            "{}/storage/v1/object/public/{}/{}",
            supabase_url.trim_end_matches('/'),
            bucket,
            upload_path
        );

        // Build response with structured metadata
        let response = json!({
            "success": true,
            "url": public_url,
            "name": title,
            "content_type": content_type,
            "kind": file_kind,
            "size_bytes": size_bytes,
            "path": upload_path,
        });

        Ok(response.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_type_detection() {
        assert_eq!(content_type_from_extension("png"), "image/png");
        assert_eq!(content_type_from_extension("PDF"), "application/pdf");
        assert_eq!(content_type_from_extension("zip"), "application/zip");
        assert_eq!(content_type_from_extension("rs"), "text/x-rust");
        assert_eq!(
            content_type_from_extension("unknown"),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_file_kind_inference() {
        assert_eq!(file_kind_from_content_type("image/png"), "image");
        assert_eq!(file_kind_from_content_type("application/pdf"), "document");
        assert_eq!(file_kind_from_content_type("application/zip"), "archive");
        assert_eq!(file_kind_from_content_type("text/x-rust"), "code");
        assert_eq!(
            file_kind_from_content_type("application/octet-stream"),
            "other"
        );
    }

    #[test]
    fn test_tool_names() {
        assert_eq!(ShareFile.name(), "share_file");
    }
}
