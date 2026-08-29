# Privacy Policy — WordBuddy — Page Connector

**Last updated:** August 28, 2026

## What This Extension Does

WordBuddy — Page Connector is a browser extension that reads the structure of
web pages you visit (buttons, links, headings, form fields) and sends that
information to the WordBuddy desktop application running on your computer.
This helps WordBuddy provide contextual guidance while you learn.

## What Data Is Collected

The extension collects the following from pages you visit on matched domains:

- **Element metadata:** Tag names (e.g. "button", "link"), visible text
  labels (truncated to 80 characters), and viewport positions of interactive
  elements (buttons, links, headings, inputs, form fields). Scans are
  capped at 400 elements per page to bound payload size.
- **Form-field values:** The periodic page-structure scan never includes
  typed form values. If inline checking is enabled, WordBuddy reads only
  the focused eligible text field to perform a check. **You can disable
  this** with the **"Don't read fields while I type"** toggle; it stops
  inline checking entirely. Inline checking is always disabled on
  `github.com` and its subdomains.
- **Curriculum metadata:** Page-authored `<meta name="wordbuddy-*">`
  tags, when present, are read and forwarded so the desktop app can
  identify the learning context more reliably than from the window title.
- **Page URL and title:** The origin and path of the current page, plus
  its title. **Query strings and URL fragments are stripped** before the
  URL leaves the page, so session ids, OAuth tokens, and tracking
  parameters are not transmitted. Link `href` attributes on the page
  are treated the same way.
- **Sensitive fields:** Password, credential, payment, token, and one-time
  code fields are excluded from inline checking. Hidden input values are
  never scanned.
- **No cookies or browser-history database access.** The extension does not
  access cookies or Chrome history. Visible page labels, titles, and link
  text can still contain personal information supplied by a website, so only
  enable the extension on sites you trust.

## Pausing and Stopping Data Collection

The extension popup includes two privacy toggles that take effect
immediately in all open tabs (no reload required):

- **Pause scanning on this browser** — halts all DOM scanning and
  highlight polling until re-enabled. The connection indicator in the
  popup will read "Paused — scanning disabled."
- **Don't read fields while I type** — keeps the page scan metadata-only and
  disables inline checking, so typed text never leaves the page through the
  extension. This protection is **always on for `github.com`** regardless of
  the toggle, because GitHub pages commonly contain sensitive content such
  as private repository names, unsent PR comments, and review drafts; the
  inline checker does not run there at all.

The connector also runs only in top-level, non-incognito pages. It does not
scan embedded frames, including `about:blank` frames inherited from another
page, and remains disabled in private browsing even if the browser permits
extensions there.

## Where Data Is Sent

All collected data goes **first to the WordBuddy desktop application**
running on your own computer at `http://127.0.0.1` (localhost), over an
authenticated local connection. What happens next depends on features:

- **Element metadata** stays on your machine. It is used for tutoring
  context and highlighting and is never forwarded anywhere.
- **Correctness checking** (spelling/grammar underlines) runs locally in
  the desktop app and never contacts any external service.
- **Browser style checking** is opt-in for each site. If you enable style
  suggestions on a site, focused-field text may be sent to the LLM provider
  configured in WordBuddy (for example Anthropic, OpenAI, Google, Groq, or
  OpenRouter) when the desktop app produces those suggestions. That provider
  is a third party governed by its own privacy policy. Setting the
  `WB_DISABLE_LLM` environment variable removes this path entirely, and the
  **"Don't read fields while I type"** toggle disables it per page as
  described above.
- No analytics, telemetry, or tracking of any kind is included.

## How Data Is Used

The WordBuddy desktop application uses the element data to:

1. Provide the AI tutor with accurate information about what's on your screen.
2. Highlight specific elements in the browser when the tutor points at them.

Element data is held in memory only for the duration of the tutoring session.
It is not written to disk, not stored in any database, and is discarded when
new data arrives or the application closes.

## Authentication

Communication between the extension and the desktop application is
authenticated using a locally-generated token. This token is stored on your
computer in the operating system's credential vault and is never transmitted
over the network. A legacy plaintext token file, if present, is migrated into
the vault and removed.

## Permissions Explained

| Permission | Why It's Needed |
|------------|----------------|
| `storage` | Stores the authentication token and port number locally in the browser so you don't have to re-enter them. |
| `activeTab` | Lets the popup identify the site you explicitly opened it on, so you can choose whether to enable WordBuddy there. This access is temporary and applies only to that active tab. |
| `scripting` | Injects the extension's page connector after you grant a site-specific optional host permission. |
| Optional host permission | Lets you enable WordBuddy for a specific HTTP(S) site host from the popup. It does not grant access to every site by default. |
| `host_permissions` (127.0.0.1) | Allows the extension to communicate with the WordBuddy desktop app running on your computer. |
| Content script (matched domains) | Reads the top-level page structure on the built-in sites to provide contextual tutoring. |

## Data Retention

- **In-browser:** The authentication token, port number, and your local
  extension preferences are stored in `chrome.storage.local` until you
  remove the extension or clear its data. Raw field text is not stored.
- **In the desktop app:** Element data is held in memory only. It is
  overwritten every 3 seconds with fresh data and fully discarded when the
  app closes. No element data is ever written to disk.

## Children's Privacy

This extension does not knowingly collect any personal information from
children under 13. The extension collects only page structure metadata
(element types and positions), not personal content.

## Your Rights

- **Uninstall** the extension at any time to stop all data collection.
- **Clear data** via chrome://extensions → WordBuddy — Page Connector → Details
  → Clear data to remove the stored token and port.
- **Inspect traffic** — all communication goes to `127.0.0.1` (your own
  machine). You can verify this with browser developer tools (Network tab).

## License

WordBuddy is proprietary software, all rights reserved. See the `LICENSE`
file in the repository root for terms.

## Changes to This Policy

If this policy is updated, the "Last updated" date at the top will change.
Significant changes will be noted in the extension's changelog.

## Contact

For questions about this privacy policy or the extension's data practices,
open an issue at: https://github.com/Frostbite1536/WordBuddy/issues
