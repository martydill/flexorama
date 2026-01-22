# Image Support Implementation Plan

> **Status: ✅ IMPLEMENTED**

## Overview

Add the ability to include images as context when chatting with LLMs, allowing users to ask questions about images. This follows the existing `@file` syntax pattern for seamless integration.

## Usage Examples

```bash
# Single image with question
flexorama "What's in this image? @screenshot.png"

# Multiple images
flexorama "Compare these two diagrams @diagram1.png @diagram2.jpg"

# Mixed files and images
flexorama "Review this code @main.rs and explain this architecture diagram @arch.png"

# CLI flag (existing pattern)
flexorama -f screenshot.png "Describe this image"
```

---

## Phase 1: Core Data Structures

### 1.1 Extend `ContentBlock` in `anthropic.rs`

Add image support to the `ContentBlock` struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    // ... existing fields ...
    
    // NEW: Image support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<ImageSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,  // "base64"
    pub media_type: String,   // "image/png", "image/jpeg", etc.
    pub data: String,         // base64-encoded image data
}

impl ContentBlock {
    pub fn image(media_type: String, base64_data: String) -> Self {
        Self {
            block_type: "image".to_string(),
            source: Some(ImageSource {
                source_type: "base64".to_string(),
                media_type,
                data: base64_data,
            }),
            // ... other fields as None ...
        }
    }
}
```

### 1.2 Supported Image Formats

| Extension | Media Type |
|-----------|------------|
| `.png` | `image/png` |
| `.jpg`, `.jpeg` | `image/jpeg` |
| `.gif` | `image/gif` |
| `.webp` | `image/webp` |

---

## Phase 2: Image Utilities Module

### 2.1 Create `src/image.rs`

```rust
use anyhow::{bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::path::Path;

const SUPPORTED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const MAX_IMAGE_SIZE_MB: u64 = 20; // Anthropic's limit

pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SUPPORTED_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn get_media_type(path: &Path) -> Result<String> {
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .ok_or_else(|| anyhow::anyhow!("No file extension"))?;
    
    match ext.as_str() {
        "png" => Ok("image/png".to_string()),
        "jpg" | "jpeg" => Ok("image/jpeg".to_string()),
        "gif" => Ok("image/gif".to_string()),
        "webp" => Ok("image/webp".to_string()),
        _ => bail!("Unsupported image format: {}", ext),
    }
}

pub fn load_image_as_base64(path: &Path) -> Result<(String, String)> {
    // Check file size
    let metadata = std::fs::metadata(path)?;
    let size_mb = metadata.len() / (1024 * 1024);
    if size_mb > MAX_IMAGE_SIZE_MB {
        bail!("Image too large: {}MB (max {}MB)", size_mb, MAX_IMAGE_SIZE_MB);
    }
    
    let media_type = get_media_type(path)?;
    let data = std::fs::read(path)?;
    let base64_data = STANDARD.encode(&data);
    
    Ok((media_type, base64_data))
}
```

---

## Phase 3: Conversation Manager Updates

### 3.1 Update `conversation.rs`

Modify `add_context_file` to handle images differently:

```rust
pub async fn add_context_file(&mut self, file_path: &str) -> Result<()> {
    let absolute_path = self.resolve_path(file_path)?;
    
    if crate::image::is_image_file(&absolute_path) {
        self.add_image_context(&absolute_path).await
    } else {
        self.add_text_context(&absolute_path).await
    }
}

async fn add_image_context(&mut self, path: &Path) -> Result<()> {
    let (media_type, base64_data) = crate::image::load_image_as_base64(path)?;
    
    // Create message with both image and description
    let content = vec![
        ContentBlock::image(media_type, base64_data),
        ContentBlock::text(format!("Image file: {}", path.display())),
    ];
    
    self.conversation.push(Message {
        role: "user".to_string(),
        content,
    });
    
    debug!("Added image context: {}", path.display());
    Ok(())
}
```

---

## Phase 4: Provider Adapters

Each LLM provider has different image API formats. Update adapters accordingly:

### 4.1 Anthropic (`anthropic.rs`) - Native Support
Already uses the `source` format, minimal changes needed.

### 4.2 OpenAI (`openai.rs`)
Convert to OpenAI's format:
```json
{
  "type": "image_url",
  "image_url": {
    "url": "data:image/png;base64,{data}"
  }
}
```

### 4.3 Gemini (`gemini.rs`)
Convert to Gemini's format:
```json
{
  "inlineData": {
    "mimeType": "image/png",
    "data": "{base64_data}"
  }
}
```

### 4.4 Ollama (`ollama.rs`)
Images passed via the `images` array field (base64 strings).

### 4.5 Mistral (`mistral.rs`)
Similar to OpenAI format (data URLs).

---

## Phase 5: CLI & Interactive Mode

### 5.1 Update `@file` Detection in `conversation.rs`

The existing regex `@([^\s@]+)` already captures image files. No changes needed for detection.

### 5.2 Update User Feedback

Modify output messages in `interactive.rs`:

```rust
if crate::image::is_image_file(Path::new(file_path)) {
    app_println!("{} Added image: {}", "🖼️".green(), file_path);
} else {
    app_println!("{} Added context file: {}", "✓".green(), file_path);
}
```

---

## Phase 6: Web UI Support

### 6.1 Update `web.rs`

- Add `ContentBlockDto` variant for images
- Update `block_to_dto` to handle image blocks
- Frontend: Display image thumbnails in context panel

---

## Phase 7: ACP Mode

### 7.1 Update `acp/handler.rs`

Add image support to the `context/addFile` method:
- Detect image files and encode as base64
- Return appropriate content block type

---

## Implementation Order

| Priority | Task | Status |
|----------|------|--------|
| 1 | Create `src/image.rs` utilities | ✅ Done |
| 2 | Extend `ContentBlock` struct | ✅ Done |
| 3 | Update `conversation.rs` | ✅ Done |
| 4 | Update Anthropic adapter | ✅ Done |
| 5 | Update OpenAI adapter | ✅ Done |
| 6 | Update Gemini adapter | ✅ Done |
| 7 | Update Ollama adapter | ✅ Done |
| 8 | Update Mistral adapter | ✅ Done |
| 9 | Update CLI feedback messages | ✅ Done |
| 10 | Update web UI | ⏳ Future |
| 11 | Update ACP handler | ⏳ Future |
| 12 | Add tests | ✅ Done |
| 13 | Update documentation | ✅ Done |

---

## Testing Strategy

1. **Unit tests** for `image.rs` utilities
2. **Integration tests** for each provider adapter
3. **Manual testing** with various image formats and sizes
4. **Edge cases**: corrupt images, unsupported formats, oversized files

---

## Future Enhancements

- URL-based image support (`@https://example.com/image.png`)
- Image resizing to reduce token usage
- Clipboard paste support in interactive mode
- Image preview in terminal (iTerm2, Kitty protocols)
