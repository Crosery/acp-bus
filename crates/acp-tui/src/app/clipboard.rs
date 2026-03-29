/// Image data from clipboard, ready to send with next prompt.
#[derive(Debug, Clone)]
pub(crate) struct PendingImage {
    pub base64: String,
    pub media_type: String,
}

/// Collection of pending images attached to the next message.
#[derive(Debug, Clone, Default)]
pub(crate) struct PendingImages {
    pub images: Vec<PendingImage>,
}

/// Read image from clipboard using arboard, falling back to wl-paste/xclip.
pub(crate) async fn read_clipboard_image() -> Option<PendingImage> {
    // Strategy 1: arboard
    let result = tokio::task::spawn_blocking(|| -> Result<PendingImage, String> {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| format!("clipboard init: {e}"))?;
        let img = clipboard
            .get_image()
            .map_err(|e| format!("get_image: {e}"))?;
        let mut png_buf = std::io::Cursor::new(Vec::new());
        let encoder = image::codecs::png::PngEncoder::new(&mut png_buf);
        image::ImageEncoder::write_image(
            encoder,
            &img.bytes,
            img.width as u32,
            img.height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("png encode: {e}"))?;
        let b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            png_buf.into_inner(),
        );
        Ok(PendingImage {
            base64: b64,
            media_type: "image/png".to_string(),
        })
    })
    .await;

    if let Ok(Ok(img)) = result {
        return Some(img);
    }

    // Strategy 2: shell fallback
    let output = tokio::process::Command::new("sh")
        .args([
            "-c",
            "xclip -selection clipboard -t image/png -o 2>/dev/null || wl-paste --type image/png 2>/dev/null",
        ])
        .output()
        .await
        .ok()?;

    if output.status.success() && !output.stdout.is_empty() {
        let b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &output.stdout,
        );
        return Some(PendingImage {
            base64: b64,
            media_type: "image/png".to_string(),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_images_default_empty() {
        let pi = PendingImages::default();
        assert!(pi.images.is_empty());
    }

    #[test]
    fn pending_image_clone() {
        let img = PendingImage {
            base64: "abc".into(),
            media_type: "image/png".into(),
        };
        let cloned = img.clone();
        assert_eq!(cloned.base64, "abc");
        assert_eq!(cloned.media_type, "image/png");
    }

    #[test]
    fn pending_images_push_and_count() {
        let mut pi = PendingImages::default();
        pi.images.push(PendingImage {
            base64: "a".into(),
            media_type: "image/png".into(),
        });
        pi.images.push(PendingImage {
            base64: "b".into(),
            media_type: "image/jpeg".into(),
        });
        assert_eq!(pi.images.len(), 2);
    }
}
