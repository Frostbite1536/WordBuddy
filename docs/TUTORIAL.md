# WorkBuddy Setup Tutorial

A step-by-step guide to getting WorkBuddy running on your computer. No prior
programming experience required.

---

## What is WorkBuddy?

WorkBuddy is a desktop app that floats at the top of your screen like a thin
bar. You type a question (or hold a button to speak), and it captures your
screen, sends it to an AI, and gives you a contextual answer. It's designed
for students learning prediction market trading through Limitless Exchange's
education programs.

---

## Step 1: Get an AI API Key

WorkBuddy needs an API key to talk to an AI. You have several options:

### Option A: Anthropic Claude (Recommended)

1. Go to [console.anthropic.com](https://console.anthropic.com/)
2. Create a free account (you'll need a phone number)
3. Add a payment method (Claude charges per use — a typical study session
   costs a few cents)
4. Go to **Settings > API Keys** and click **Create Key**
5. Copy the key — it starts with `sk-ant-...`
6. Save it somewhere safe. You'll paste it into WorkBuddy later.

### Option B: Groq (Free Tier)

1. Go to [console.groq.com](https://console.groq.com/)
2. Create a free account
3. Go to **API Keys** and create one
4. Copy the key — it starts with `gsk_...`
5. Groq offers a generous free tier with fast models like Llama 3.3

### Option C: Ollama (Free, Runs Locally)

1. Download [Ollama](https://ollama.com/download) for your OS
2. Install and run it
3. Open a terminal and run: `ollama pull llama3.2-vision`
4. No API key needed — the AI runs on your own machine
5. Requires a decent GPU (8GB+ VRAM) for good performance

### Option D: Google Gemini (Recommended for Full-Stack Value)

1. Go to [aistudio.google.com/apikey](https://aistudio.google.com/apikey)
2. Sign in with a Google account
3. Click **Create API key**
4. Copy the key — it starts with `AIza...`

Why Gemini is a particularly good choice: **one key powers three services**.
The same Google API key works for the LLM (Gemini 2.5/3.x Flash/Pro), the
TTS voice responses (30 voices via Gemini 3.1 Flash TTS), *and* push-to-talk
speech-to-text (Gemini Flash audio understanding). You don't need separate
OpenAI or ElevenLabs accounts.

### Option E: Other Providers

WorkBuddy also supports OpenAI (GPT) and OpenRouter. Get a key from any
of these services if you already have one.

---

## Step 2: Install Prerequisites

WorkBuddy is built with Tauri, which needs Rust and Node.js. Follow the
instructions for your operating system.

### Windows

1. **Install Rust**
   - Go to [rustup.rs](https://rustup.rs/)
   - Download and run `rustup-init.exe`
   - Accept the defaults (press Enter when prompted)
   - When it finishes, close and reopen any terminal windows

2. **Install Node.js**
   - Go to [nodejs.org](https://nodejs.org/)
   - Download the **LTS** version (the big green button)
   - Run the installer, accepting all defaults

3. **Install Visual Studio Build Tools**
   - Rust on Windows needs C++ build tools
   - Go to [visualstudio.microsoft.com/visual-cpp-build-tools/](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
   - Download and run the installer
   - Select **"Desktop development with C++"** and install

4. **Verify everything works** — open a new terminal (Command Prompt or
   PowerShell) and run:
   ```
   rustc --version
   node --version
   npm --version
   ```
   Each should print a version number. If any says "not recognized", restart
   your computer and try again.

### macOS

1. **Install Xcode Command Line Tools**
   - Open Terminal (press Cmd+Space, type "Terminal", press Enter)
   - Run: `xcode-select --install`
   - Click "Install" in the popup

2. **Install Rust**
   - In Terminal, run:
     ```
     curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
     ```
   - Press Enter to accept defaults
   - Run: `source $HOME/.cargo/env`

3. **Install Node.js**
   - Go to [nodejs.org](https://nodejs.org/) and download the **LTS** version
   - Run the `.pkg` installer

4. **Verify** in Terminal:
   ```
   rustc --version
   node --version
   ```

### Linux (Ubuntu / Debian)

1. **Install system dependencies**
   - Open a terminal and run:
     ```bash
     sudo apt update
     sudo apt install -y \
       build-essential \
       curl \
       wget \
       file \
       libwebkit2gtk-4.1-dev \
       libgtk-3-dev \
       libayatana-appindicator3-dev \
       librsvg2-dev \
       libsoup-3.0-dev \
       libjavascriptcoregtk-4.1-dev \
       libxdo-dev \
       libasound2-dev \
       libpipewire-0.3-dev
     ```

2. **Install Rust**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source $HOME/.cargo/env
   ```

3. **Install Node.js**
   ```bash
   curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
   sudo apt install -y nodejs
   ```

4. **Verify**:
   ```bash
   rustc --version
   node --version
   ```

---

## Step 3: Download WorkBuddy

### Option A: Download with Git (Recommended)

If you have Git installed:
```bash
git clone https://github.com/Frostbite1536/WorkBuddy.git
cd WorkBuddy
```

If you don't have Git:
- **Windows:** Download [Git for Windows](https://git-scm.com/download/win)
  and install it, then run the commands above in Git Bash
- **macOS:** Git was installed with Xcode tools in Step 2
- **Linux:** Run `sudo apt install git`, then the commands above

### Option B: Download as ZIP

1. Go to [github.com/Frostbite1536/WorkBuddy](https://github.com/Frostbite1536/WorkBuddy)
2. Click the green **"Code"** button
3. Click **"Download ZIP"**
4. Extract the ZIP file to a folder you can find easily (e.g., your Desktop
   or Documents)
5. Open a terminal and navigate to that folder:
   ```bash
   cd ~/Desktop/WorkBuddy    # adjust the path to where you extracted it
   ```

---

## Step 4: Install Dependencies

In your terminal, make sure you're inside the WorkBuddy folder, then run:

```bash
npm install
```

This downloads all the JavaScript libraries WorkBuddy needs. It may take a
minute or two. You'll see a progress bar and then a summary.

If you see **"found 0 vulnerabilities"** at the end, you're good.

---

## Step 5: Run WorkBuddy

```bash
npx tauri dev
```

The first time you run this, it will:
1. Download and compile all the Rust dependencies (~2-5 minutes)
2. Start the Vite development server
3. Open the WorkBuddy window

You'll see a lot of text scrolling in the terminal — that's normal. Wait
until you see the WorkBuddy bar appear at the top of your screen.

**If you get errors**, see the [Troubleshooting](#troubleshooting) section below.

---

## Step 6: First Launch Setup

When WorkBuddy opens for the first time, you'll see the **onboarding wizard**.

### 6a. Welcome Screen

Click **"Get Started"**.

### 6b. API Key

1. Paste the API key you got in Step 1
2. Click **"Validate Key"**
3. Wait for the green checkmark
4. Click **"Continue"**

If you chose Ollama (no key needed), you can change the provider later in
Settings — for now, enter any key or skip to set up Ollama after onboarding.

### 6c. Choose Your Program

Select which program you're enrolled in:
- **PM Academy** — Learning prediction market trading fundamentals
- **API Academy** — Building trading bots with the SDK
- **Agents Academy** — Building AI-powered trading agents
- **Limitless Trader Lab** — The 4-week cohort program

Don't worry — you can change this anytime in Settings.

### 6d. Keyboard Shortcuts

Review the shortcuts:

| Shortcut | Action |
|----------|--------|
| **Ctrl + Shift + S** | Show/hide WorkBuddy |
| **Ctrl + Shift + X** | Take a screenshot |
| **Ctrl + Shift + F** | Focus the text input |
| **Ctrl + Space** | Toggle push-to-talk microphone |

These work from **any application** — you don't need WorkBuddy focused.

### 6e. Ready!

Click **"Start Learning"**. WorkBuddy collapses to a thin bar at the top
of your screen.

---

## Step 7: Using WorkBuddy

### Ask a Question

1. Click the WorkBuddy bar or press **Ctrl+Shift+S** to show it
2. Type your question in the text field
3. Press **Enter** or click the Send button
4. WorkBuddy automatically captures your screen for context
5. The response streams in below your question

### Voice Input (Push-to-Talk)

1. Hold the **microphone button** in the bar (or press **Ctrl+Space** to toggle)
2. Speak your question
3. Release the button — your speech is transcribed and auto-submitted

**STT providers (pick one in Settings > Speech-to-Text):**
- **OpenAI Whisper** — industry-standard, 99+ languages. Requires an OpenAI key.
- **ElevenLabs Scribe** — reuses your ElevenLabs TTS key. Requires the
  "Speech to Text" permission on the key.
- **Gemini Flash** — reuses your Google API key (the same one used for the
  Gemini LLM and Gemini TTS). Typically the cheapest option for short
  utterances (~3x cheaper than Whisper).

### Streaming Voice Responses

Voice responses stream **sentence-by-sentence as they arrive** — you don't
need to wait for the full response. Each sentence is synthesized and played
automatically.

**TTS providers (pick one in Settings > Voice Responses):**
- **ElevenLabs** — premium quality, 10 curated voices. Requires a separate
  [ElevenLabs](https://elevenlabs.io/app/developers/api-keys) key.
- **Gemini Flash** — 30 voices (Sulafat/Achird/Sadaltager work great for a
  tutor tone). Reuses your Google API key. Lower cost than ElevenLabs.

**ElevenLabs API key setup:** When creating a key, enable these permissions:
- **Text to Speech → Access** (required for voice responses)
- **Speech to Text → Access** (optional — enables ElevenLabs for push-to-talk)

Everything else can stay at "No Access" for maximum security. You can also
set a monthly credit limit to control costs.

**Gemini TTS/STT setup:** No extra config beyond having a Google API key in
Settings > AI Provider. Select "Gemini Flash" in the TTS and/or STT provider
sections and pick a voice from the dropdown (for TTS).

### Cursor Pointing

WorkBuddy can point at elements on your screen with an animated cursor and
spotlight effect. The screen dims and a bright spotlight highlights the
element the AI is referring to.

WorkBuddy uses a layered **detection stack** for precise pointing (you don't
need to configure this — it all happens automatically):

1. **Browser extension** (if installed) — reads the DOM directly, under 10ms,
   perfect accuracy on web pages. See the workbuddy-extension folder.
2. **Accessibility detection** (enabled by default) — reads element names
   and bounding rectangles from your OS accessibility tree. Works in IDEs,
   terminals, Claude Desktop, and other apps. No download required. Toggle
   in **Settings > Accessibility Detection** if you want to disable it.
   On macOS, grant Accessibility permission in System Settings > Privacy &
   Security when prompted.
3. **Local UI Detection** (opt-in, fallback) — an AI model that detects
   buttons/icons from the screenshot. Go to **Settings > Local UI Detection**
   and click **"Download model (~40-50 MB)"** to enable. Only needed for apps
   with stubbed-out accessibility trees (rare — mostly games and legacy apps).
4. **LLM estimation** — the final fallback when nothing above produces data.

**Auto-screenshot** is also enabled by default — WorkBuddy automatically
captures your screen with every question for visual context. You can toggle
this off in **Settings > Auto-screenshot** if you prefer text-only mode.

### Multi-Monitor Setup

If you have multiple monitors, go to **Settings > Capture Monitor** to select
which monitor WorkBuddy screenshots. By default it captures the primary
monitor. Select your second monitor to have WorkBuddy see what's on that
screen instead.

### Tutor Mode (Socratic Learning)

WorkBuddy has a **tutor mode** that changes how the AI interacts with you.
Instead of just answering your questions, it becomes a Socratic tutor:

- **Asks questions** to test your understanding before giving answers
- **Guides you through interactive elements** ("Try setting the slider to
  70% and tell me what the payout shows")
- **Points at things** you should interact with on screen
- **Builds on your answers** to progressively deepen your understanding

To enable tutor mode:
1. Click the **book icon** in the WorkBuddy bar (it turns amber when active)
2. Or go to **Settings > Tutor Mode** and toggle it on

Tutor mode works great when you're studying a module and want to test yourself
rather than just reading. Try asking "quiz me on this section" or
"check my understanding of order types."

### View History

Click the **clock icon** or go to **Settings > History** to see past
conversations. These are saved to a local database and survive app restarts.

### Change Provider or Model

1. Click the **gear icon** to open Settings
2. Under **"AI Provider"**, select your preferred provider
3. Enter the API key for that provider
4. Choose a model from the dropdown

### Document Knowledge Base (RAG)

WorkBuddy can index Limitless documentation so it retrieves the most
relevant docs for each question you ask — not just the static snippets
for your current module.

1. Go to **Settings** and scroll to **"Document Knowledge Base"**
2. Click **"Index Documents"** to index the Limitless source docs
3. Once indexed, every question you ask will automatically search for
   relevant documentation and include it in the AI's context

**Note:** Document indexing requires an **OpenAI API key** (for generating
embeddings via text-embedding-3-small). This is the same key used for
Whisper speech-to-text. The one-time indexing cost is ~$0.003 — essentially
free. If no OpenAI key is configured, WorkBuddy still works using the
built-in static topic snippets for each module.

---

## Step 8: Build a Release Binary (Optional)

If you want to create an installer you can share or use without the terminal:

```bash
npx tauri build
```

This creates:
- **Windows:** `.msi` installer in `src-tauri/target/release/bundle/msi/`
- **macOS:** `.dmg` file in `src-tauri/target/release/bundle/dmg/`
- **Linux:** `.AppImage` and `.deb` in `src-tauri/target/release/bundle/`

Double-click the installer to install WorkBuddy like any normal app.

---

## Troubleshooting

### "cargo: command not found"

Rust isn't in your PATH. Try:
- **Windows:** Close and reopen your terminal, or restart your computer
- **macOS/Linux:** Run `source $HOME/.cargo/env`

### "npm: command not found"

Node.js isn't installed or isn't in your PATH. Re-download from
[nodejs.org](https://nodejs.org/) and reinstall.

### Build fails with "linker not found" (Windows)

You need the Visual Studio Build Tools. Go back to Step 2 and install the
**"Desktop development with C++"** workload.

### Build fails with "webkit2gtk not found" (Linux)

Install the missing system libraries:
```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev
```

### "Failed to list monitors" or "No monitors found"

- **Linux (Wayland):** WorkBuddy's screen capture works best under X11.
  Try running with `GDK_BACKEND=x11 npx tauri dev`
- **macOS:** Grant screen recording permission in System Settings >
  Privacy & Security > Screen Recording

### "No input device available" (Microphone)

- Check that your microphone is connected and enabled in your OS settings
- **macOS:** Grant microphone permission in System Settings >
  Privacy & Security > Microphone
- **Linux:** Make sure PulseAudio or PipeWire is running

### "API key invalid"

- Make sure you copied the full key including the prefix (`sk-ant-...` for
  Anthropic, `sk-...` for OpenAI, etc.)
- Check that you have billing enabled on your API provider's dashboard
- Some providers require email verification before keys work

### The app window disappeared

Press **Ctrl+Shift+S** to toggle visibility. WorkBuddy hides to the
taskbar tray when minimized.

### Everything is really slow the first time

The first `npx tauri dev` compiles all Rust dependencies from source. This
is a one-time cost — subsequent launches are much faster (a few seconds).

---

## Updating WorkBuddy

If you installed from Git:
```bash
cd WorkBuddy
git pull
npm install
npx tauri dev
```

If you installed a release binary, WorkBuddy will check for updates
automatically and notify you when a new version is available.

---

## Uninstalling

### If you used `npx tauri dev` (development mode)
Just delete the WorkBuddy folder. No files are installed system-wide except
the config and data files at:
- **Windows:** `%APPDATA%\workbuddy\` (config.json + rag_vectors.db)
- **macOS:** `~/Library/Application Support/workbuddy/`
- **Linux:** `~/.config/workbuddy/`

### If you installed a release binary
- **Windows:** Uninstall via Settings > Apps
- **macOS:** Drag WorkBuddy from Applications to Trash
- **Linux:** Delete the `.AppImage`, or `sudo dpkg -r workbuddy` if you
  installed the `.deb`

---

## Getting Help

- **GitHub Issues:** [github.com/Frostbite1536/WorkBuddy/issues](https://github.com/Frostbite1536/WorkBuddy/issues)
- **Source Code:** [github.com/Frostbite1536/WorkBuddy](https://github.com/Frostbite1536/WorkBuddy)
- **Architecture Docs:** See [docs/ARCHITECTURE.md](ARCHITECTURE.md) for
  how the app is built

---

## Quick Reference Card

| Action | How |
|--------|-----|
| Show/hide WorkBuddy | **Ctrl + Shift + S** |
| Ask a question | Type in the bar and press **Enter** |
| Take a screenshot | **Ctrl + Shift + X** or click the camera icon |
| Focus the input field | **Ctrl + Shift + F** |
| Push-to-talk | Hold the mic button or **Ctrl + Space** |
| Listen to a response | Click **"Listen"** below an assistant message |
| View history | Click the clock icon in the bar |
| Toggle tutor mode | Click the book icon in the bar |
| Open settings | Click the gear icon in the bar |
| Change AI provider | Settings > AI Provider |
| Change program | Settings > Active Program |
| Index docs for RAG | Settings > Document Knowledge Base > Index |
| Select capture monitor | Settings > Capture Monitor |
| Download UI detection model | Settings > Local UI Detection > Download |
| Toggle accessibility detection | Settings > Accessibility Detection |
| Change STT provider | Settings > Speech-to-Text (Whisper / ElevenLabs / Gemini Flash) |
| Change TTS provider | Settings > Voice Responses (ElevenLabs / Gemini Flash) |
| Change TTS voice | Settings > Voice Responses > Voice dropdown |
| Clear messages | Click the trash icon in the bar |
| Close WorkBuddy | Click the **X** button in the bar |
