# WorkBuddy

A cross-platform desktop AI assistant that sees your screen. It floats as a
thin bar at the top of your display, captures what you're looking at, answers
questions with your choice of LLM provider, and can physically point at
things on screen.

**Direction:** WorkBuddy is evolving into a private, automatic work journal
for Windows/Linux/Mac — periodic screen capture → LLM analysis → a timeline
of your day (in the spirit of [Dayflow](https://github.com/JerryZLiu/Dayflow)
for the Mac). Phase 0 (this state) is the stripped, rebranded, relicensed
foundation; the recorder and timeline come next. See `docs/DECISIONS.md`
ADR-041.

## Features

- **Multi-LLM streaming** — Anthropic Claude, OpenAI GPT, Google Gemini, Groq, Ollama (local), OpenRouter
- **Screen capture** — JPEG screenshots with every question (multi-monitor support, select monitor in Settings)
- **Browser extension** — Chrome/Edge extension reads the DOM directly for instant (<10ms) pixel-precise element detection on web pages
- **Accessibility-powered detection** — Native OS accessibility API (Windows UIA / macOS AX / Linux AT-SPI2) reads element names + bounding rects from the focused window for pixel-precise pointing in IDEs, terminals, and Electron apps
- **Cursor pointing** — the model points at screen elements with a spring-animated cursor + SVG spotlight overlay
- **Push-to-talk** — hold-to-record microphone with three STT providers: OpenAI Whisper, ElevenLabs, or Gemini Flash
- **Two TTS providers** — ElevenLabs or Gemini Flash, streamed sentence-by-sentence
- **RAG document search** — OpenAI embeddings + cosine similarity over a folder you choose
- **Persistent history** — conversations saved to SQLite, survive app restarts
- **Global shortcuts** — Ctrl+Shift+S (toggle), Ctrl+Shift+X (screenshot), Ctrl+Shift+F (focus), Ctrl+Space (push-to-talk)

## Quick Start

```bash
# Prerequisites: Rust, Node.js 18+, platform libs (see below)

git clone https://github.com/Frostbite1536/WorkBuddy.git
cd WorkBuddy
npm install

# Run in dev mode
npx tauri dev

# Build release
npx tauri build
```

### Linux Dependencies

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
  libpipewire-0.3-dev libxdo-dev libasound2-dev
```

### API Keys

You need an API key from at least one LLM provider (or a local Ollama
instance, which needs none). Optional: ElevenLabs (voice), OpenAI (Whisper
STT / RAG embeddings), Google AI (Gemini LLM + TTS + STT with one key).

## Running Tests

```bash
cd src-tauri && cargo test   # Rust unit tests
npx tsc --noEmit             # TypeScript type check
npm test                     # Vitest unit tests
npx vite build               # Frontend build
```

## License

Proprietary — all rights reserved. See [LICENSE](LICENSE) for the full
notice, including provenance credits to
[pluely](https://github.com/iamsrikanthnani/pluely) and
[Clicky](https://github.com/farzaa/clicky), which inspired the original
architecture (no code copied — audit recorded in `docs/DECISIONS.md`
ADR-041).
