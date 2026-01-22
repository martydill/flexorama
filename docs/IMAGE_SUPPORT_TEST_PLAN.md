# Image Support Test Plan

## Overview
This document outlines the test coverage for the image paste support feature in the Web UI.

## Test Coverage

### Backend Tests (src/web.rs)

#### 1. ContentBlockDto Serialization with Images
**Test:** `test_block_to_dto_with_image_source`
- Verify that `block_to_dto()` correctly serializes image blocks
- Verify that the `source` field contains media_type and data
- Verify that image blocks without source are handled correctly

#### 2. block_text_summary for Images
**Test:** `test_block_text_summary_image_type`
- Verify that image blocks return "[Image]" as summary text
- Verify that text blocks still return their text content
- Verify that tool blocks return their tool summary

#### 3. MessageRequest Deserialization with Images
**Test:** `test_message_request_deserialize_with_images`
- Verify that MessageRequest correctly deserializes with images array
- Verify that MessageRequest works without images (None)
- Verify that empty images array is handled correctly

### Backend Tests (src/agent.rs)

#### 4. Agent::add_image Method
**Test:** `test_add_image_adds_to_conversation`
- Verify that add_image() adds a message to the conversation
- Verify that the message contains an image ContentBlock
- Verify that the image has correct media_type and base64 data
- Verify that optional description is added when provided

**Test:** `test_add_image_without_description`
- Verify that images can be added without description
- Verify that only image ContentBlock is added (no text block)

**Test:** `test_add_image_with_description`
- Verify that description is added as a text ContentBlock
- Verify that both image and text blocks are present

### Integration Tests

#### 5. Message Handler Processing
**Test:** `test_send_message_with_images` (would require mocking the LLM)
- Verify that send_message_to_conversation processes images
- Verify that images are added to conversation before message processing
- (Note: This may be complex due to LLM dependencies)

### Frontend Tests (Manual)
Since the frontend is JavaScript and this project doesn't have a JS test framework:

1. **Image Paste Test**
   - Paste image into textarea
   - Verify thumbnail appears
   - Verify remove button works

2. **Image Send Test**
   - Paste image and send message
   - Verify image is sent to backend
   - Verify image appears in message history

3. **Multiple Images Test**
   - Paste multiple images
   - Verify all thumbnails appear
   - Verify all images are sent

4. **Image Display Test**
   - Verify images display in conversation
   - Verify click to expand works
   - Verify max dimensions are respected

## Test Results

### Unit Tests
- All unit tests should pass with `cargo test`
- Tests should run in isolation without external dependencies

### Integration Tests
- Manual testing required for Web UI
- Test with different image formats (PNG, JPEG, GIF, WebP)
- Test with different image sizes

## Coverage Gaps

The following areas need testing but may be difficult to automate:

1. **Base64 Encoding** - Verify images are correctly encoded
2. **Image Size Limits** - Verify large images are handled (though this is done in image.rs)
3. **Image Format Support** - Verify all supported formats work
4. **Error Handling** - Verify graceful degradation if image fails

## Recommendations

1. Add unit tests for all new backend functions
2. Consider adding integration tests with a test LLM client
3. Add manual test checklist for Web UI functionality
4. Consider adding E2E tests with a tool like Playwright in the future
