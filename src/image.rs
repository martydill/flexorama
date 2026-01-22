use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use log::debug;
use std::path::Path;

const SUPPORTED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const MAX_IMAGE_SIZE_MB: u64 = 20;

/// Check if a file path points to a supported image format
pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Check if a file path string points to a supported image format
pub fn is_image_path(path_str: &str) -> bool {
    is_image_file(Path::new(path_str))
}

/// Get the MIME type for an image file based on its extension
pub fn get_media_type(path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .ok_or_else(|| anyhow::anyhow!("No file extension found"))?;

    match ext.as_str() {
        "png" => Ok("image/png".to_string()),
        "jpg" | "jpeg" => Ok("image/jpeg".to_string()),
        "gif" => Ok("image/gif".to_string()),
        "webp" => Ok("image/webp".to_string()),
        _ => bail!("Unsupported image format: {}", ext),
    }
}

/// Load an image file and encode it as base64
/// Returns (media_type, base64_data)
pub fn load_image_as_base64(path: &Path) -> Result<(String, String)> {
    let metadata = std::fs::metadata(path)?;
    let size_mb = metadata.len() / (1024 * 1024);

    if size_mb > MAX_IMAGE_SIZE_MB {
        bail!(
            "Image file too large: {}MB (maximum is {}MB)",
            size_mb,
            MAX_IMAGE_SIZE_MB
        );
    }

    let media_type = get_media_type(path)?;
    let data = std::fs::read(path)?;
    let base64_data = STANDARD.encode(&data);

    debug!(
        "Loaded image: {} ({}, {} bytes)",
        path.display(),
        media_type,
        data.len()
    );

    Ok((media_type, base64_data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_is_image_file_supported_formats() {
        assert!(is_image_file(Path::new("test.png")));
        assert!(is_image_file(Path::new("test.PNG")));
        assert!(is_image_file(Path::new("test.jpg")));
        assert!(is_image_file(Path::new("test.JPG")));
        assert!(is_image_file(Path::new("test.jpeg")));
        assert!(is_image_file(Path::new("test.JPEG")));
        assert!(is_image_file(Path::new("test.gif")));
        assert!(is_image_file(Path::new("test.GIF")));
        assert!(is_image_file(Path::new("test.webp")));
        assert!(is_image_file(Path::new("test.WEBP")));
    }

    #[test]
    fn test_is_image_file_unsupported_formats() {
        assert!(!is_image_file(Path::new("test.txt")));
        assert!(!is_image_file(Path::new("test.rs")));
        assert!(!is_image_file(Path::new("test.pdf")));
        assert!(!is_image_file(Path::new("test.svg")));
        assert!(!is_image_file(Path::new("test.bmp")));
        assert!(!is_image_file(Path::new("test")));
    }

    #[test]
    fn test_is_image_path() {
        assert!(is_image_path("path/to/image.png"));
        assert!(is_image_path("./screenshot.jpg"));
        assert!(!is_image_path("document.pdf"));
    }

    #[test]
    fn test_get_media_type() {
        assert_eq!(get_media_type(Path::new("test.png")).unwrap(), "image/png");
        assert_eq!(get_media_type(Path::new("test.jpg")).unwrap(), "image/jpeg");
        assert_eq!(
            get_media_type(Path::new("test.jpeg")).unwrap(),
            "image/jpeg"
        );
        assert_eq!(get_media_type(Path::new("test.gif")).unwrap(), "image/gif");
        assert_eq!(
            get_media_type(Path::new("test.webp")).unwrap(),
            "image/webp"
        );
    }

    #[test]
    fn test_get_media_type_unsupported() {
        assert!(get_media_type(Path::new("test.txt")).is_err());
        assert!(get_media_type(Path::new("test")).is_err());
    }

    #[test]
    fn test_load_image_as_base64() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("test.png");

        // Create a minimal valid PNG file (1x1 transparent pixel)
        let png_data: [u8; 63] = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, // RGBA, etc
            0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, // IDAT chunk
            0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, // compressed data
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, // IEND chunk
            0xAE, 0x42, 0x60, 0x82,
        ];

        let mut file = std::fs::File::create(&image_path).unwrap();
        file.write_all(&png_data).unwrap();

        let (media_type, base64_data) = load_image_as_base64(&image_path).unwrap();

        assert_eq!(media_type, "image/png");
        assert!(!base64_data.is_empty());

        // Verify we can decode it back
        let decoded = STANDARD.decode(&base64_data).unwrap();
        assert_eq!(decoded, png_data);
    }

    #[test]
    fn test_load_image_nonexistent_file() {
        let result = load_image_as_base64(Path::new("nonexistent.png"));
        assert!(result.is_err());
    }
}
