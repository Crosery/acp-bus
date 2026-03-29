use super::clipboard::PendingImage;
use crate::i18n;

/// Save a PendingImage to a temp file in cwd. Returns (file_path, prompt_text).
pub(crate) async fn save_paste_image(
    img: &PendingImage,
    cwd: &str,
    text_payload: &str,
) -> (std::path::PathBuf, String) {
    let ext = if img.media_type.contains("png") {
        "png"
    } else {
        "jpg"
    };
    let ts = chrono::Utc::now().timestamp_millis();
    let filename = format!(".acp-paste-{ts}.{ext}");
    let filepath = std::path::Path::new(cwd).join(&filename);
    if let Ok(raw) = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &img.base64,
    ) {
        let _ = tokio::fs::write(&filepath, &raw).await;
    }
    let abs_path = filepath.to_string_lossy();
    let prompt = if text_payload.is_empty() {
        i18n::sys_image_pasted(&abs_path)
    } else {
        format!(
            "{}\n{text_payload}",
            i18n::sys_image_pasted_with_text(&abs_path)
        )
    };
    (filepath, prompt)
}

/// Remove a temp paste file.
pub(crate) async fn cleanup_paste_file(path: &std::path::PathBuf) {
    let _ = tokio::fs::remove_file(path).await;
}

/// Remove all .acp-paste-* files in a directory.
pub(crate) async fn cleanup_all_paste_files(dir: &str) {
    if let Ok(mut entries) = tokio::fs::read_dir(dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(".acp-paste-") {
                    let _ = tokio::fs::remove_file(entry.path()).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn save_and_cleanup_round_trip() {
        let img = PendingImage {
            base64: base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                b"fake png",
            ),
            media_type: "image/png".into(),
        };
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        let (path, prompt) = save_paste_image(&img, &dir, "hello").await;
        assert!(path.exists());
        assert!(prompt.contains("hello"));
        cleanup_paste_file(&path).await;
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn save_empty_text_gives_image_only_prompt() {
        let img = PendingImage {
            base64: base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                b"fake image data for test",
            ),
            media_type: "image/png".into(),
        };
        let dir = std::env::temp_dir()
            .join("acp-test-empty-text")
            .to_string_lossy()
            .to_string();
        let _ = tokio::fs::create_dir_all(&dir).await;
        let (path, prompt) = save_paste_image(&img, &dir, "").await;
        assert!(path.exists());
        // Should not contain a newline before content since text is empty
        assert!(!prompt.contains('\n'));
        cleanup_paste_file(&path).await;
        let _ = tokio::fs::remove_dir(&dir).await;
    }

    #[tokio::test]
    async fn cleanup_all_removes_paste_files() {
        let dir = std::env::temp_dir().join("acp-test-cleanup");
        let _ = tokio::fs::create_dir_all(&dir).await;
        let paste = dir.join(".acp-paste-12345.png");
        let normal = dir.join("keep-me.txt");
        let _ = tokio::fs::write(&paste, b"img").await;
        let _ = tokio::fs::write(&normal, b"text").await;

        cleanup_all_paste_files(&dir.to_string_lossy()).await;

        assert!(!paste.exists());
        assert!(normal.exists());

        // Cleanup test dir
        let _ = tokio::fs::remove_file(&normal).await;
        let _ = tokio::fs::remove_dir(&dir).await;
    }
}
