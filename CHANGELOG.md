# Changelog

All notable changes to WorkBuddy will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-08

Initial public release: a cross-platform desktop AI teaching assistant
for prediction-market education, built on Tauri 2 (Rust + React/TS).

### Added
- **Multi-LLM core.** Anthropic, OpenAI, Google, Groq, Ollama, OpenRouter
  with provider-specific SSE parsing, vision support, and stream
  cancellation via generation counter.
- **Screen capture + UI detection.** `xcap` JPEG capture across
  monitors, with a 3-tier detection stack: browser extension (DOM,
  <10ms) → OS accessibility tree (Windows UIA / macOS AX / Linux
  AT-SPI) → OmniParser V2 YOLOv8s ONNX inference.
- **Speech.** ElevenLabs and Gemini Flash TTS (with per-provider WAV
  vs MP3 handling); OpenAI Whisper / ElevenLabs / Gemini Flash STT.
  Push-to-talk microphone with tuned VAD thresholds.
- **Curriculum context.** RAG over Limitless Academy lesson plans
  (OpenAI text-embedding-3-small, ~300 chunks, Rust cosine search) +
  module-specific UI element descriptions for 58 modules.
- **Cohort telemetry.** Two-tier collection (Tier 1 metadata, Tier 2
  redacted fragments) with consent gating, 7-stage redaction
  pipeline (INV-TEL-010), and policy-version re-consent flow.
- **Brand alignment.** "WorkBuddy for Limitless Academy" identity
  across the shell — self-hosted MD Nichrome typography, dynamic
  per-academy accent skin (oxblood / navy / aubergine / espresso),
  Limitless-aligned mark.
- **Wotch integration.** One-click "Open in Wotch" handoff for code
  questions; optional stdio MCP server (`workbuddy-mcp`) exposing
  curriculum context to Claude Code.

### Known issues
- Auto-updater is intentionally disabled pending signing-key
  configuration. Updates require manual download until
  `TAURI_SIGNING_PRIVATE_KEY` is wired and the plugin is re-enabled.
- macOS and Windows binaries are unsigned in this release. Users
  will see "unidentified developer" / SmartScreen warnings on first
  launch.

[Unreleased]: https://github.com/Frostbite1536/WorkBuddy/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Frostbite1536/WorkBuddy/releases/tag/v0.1.0
