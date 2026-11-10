# ADR 0051 — Images ride with the message that produced them

## Status

Accepted.

## Context

A host needs the model to see an image, starting with a screenshot of a
window. The kernel's `Message.content` is a `String`, and a tool result is a
string, so an image has nowhere to live. Both providers the kernel speaks
accept image content: Anthropic as `image` blocks, OpenAI-compatible
endpoints as `image_url` parts with a `data:` URL. OpenAI tool messages carry
text only; Anthropic `tool_result` blocks can hold images directly.

The question is where an image lives and how long it survives. A screenshot is
evidence for the work it belongs to; storing it in the session would grow the
document that ADR-level work just shrank, and a summarizer cannot fold an
image into prose without losing it anyway.

## Decision

1. `ImagePart { media_type, data }` is a value the kernel can put on a
   `Message` and on a `ToolResult`. `Message.images` and `ToolResult.images`
   default to empty; existing constructors are unchanged.
2. Images are in-memory only. The session snapshot stores no image bytes, so
   an image is visible for the turn that produced it. A later turn that needs
   to look again calls the tool again.
3. The driver attaches a result's images to its tool message. The text
   projection of the result is unchanged.
4. Adapters encode the images:
   - OpenAI-compatible: a user or assistant message with images becomes a
     content array (`text` plus `image_url`). A tool message keeps its string
     content and is followed by a user message holding the images, because
     that wire format has no image content on a `tool` role.
   - Anthropic: a `tool_result` whose content is an array of blocks, and user
     content blocks for a user message.
5. The context estimate charges one fixed amount per image
   (`ESTIMATED_IMAGE_TOKENS`, 1024). Providers resize an image to their own
   budget, so bytes are not a token count.

## Consequences

- A host tool can return a screenshot without the kernel linking a platform
  capture API; the kernel stays platform-free.
- Images cost budget but not disk. A conversation cannot grow because of
  screenshots, and a restored session has the text and the path, not the
  pixels.
- A follow-up question about an old screenshot cannot be answered from
  history; the tool must capture again. This is deliberate: the screen has
  probably changed.
- Both adapters must keep the image shape in their wire tests; a provider that
  rejects images fails at the endpoint, not silently.

## Alternatives rejected

- Base64 inside the text content: the model does not decode images from text;
  it only spends tokens.
- Storing images in the session snapshot: unbounded document growth, and a
  restored image is stale evidence.
- A kernel image store with paths: the kernel would own file IO for something
  a host tool already writes. The host keeps the file; the model gets bytes.
- A separate image message type: the message already knows its role and call
  id, which is what the adapters need to place the image.
