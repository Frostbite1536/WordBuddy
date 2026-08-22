# Privacy Policy — WorkBuddy Screen Reader Extension

**Last updated:** April 16, 2026

## What This Extension Does

WorkBuddy Screen Reader is a browser extension that reads the structure of
web pages you visit (buttons, links, headings, form fields) and sends that
information to the WorkBuddy desktop application running on your computer.
This helps WorkBuddy provide contextual guidance while you learn.

## What Data Is Collected

The extension collects the following from pages you visit on matched domains:

- **Element metadata:** Tag names (e.g. "button", "link"), visible text
  labels (truncated to 80 characters), and viewport positions of interactive
  elements (buttons, links, headings, inputs, form fields). Scans are
  capped at 400 elements per page to bound payload size.
- **Form-field values:** For non-password, non-hidden `<input>`,
  `<textarea>`, and `<select>` elements, the current typed value is
  included so the tutor can see what you've entered (e.g. a search
  query). **You can disable this** with the **"Don't send form-field
  values"** toggle in the extension popup — when enabled, the label or
  placeholder is sent instead.
- **Page URL and title:** The origin and path of the current page, plus
  its title. **Query strings and URL fragments are stripped** before the
  URL leaves the page, so session ids, OAuth tokens, and tracking
  parameters are not transmitted. Link `href` attributes on the page
  are treated the same way.
- **No passwords:** Password input fields are explicitly excluded.
  Hidden input values are also excluded.
- **No cookies, browsing history, or personal data.**

## Pausing and Stopping Data Collection

The extension popup includes two privacy toggles that take effect
immediately in all open tabs (no reload required):

- **Pause scanning on this browser** — halts all DOM scanning and
  highlight polling until re-enabled. The connection indicator in the
  popup will read "Paused — scanning disabled."
- **Don't send form-field values** — keeps field metadata (a field
  exists, its type, its label) but drops whatever the user has typed.
  This protection is **always on for `github.com`** regardless of the
  toggle, because GitHub pages commonly contain sensitive content such
  as private repository names, unsent PR comments, and review drafts.

## Where Data Is Sent

All collected data is sent **exclusively to the WorkBuddy desktop
application** running on your own computer at `http://127.0.0.1` (localhost).

- Data never leaves your machine.
- Data is never sent to any external server, cloud service, or third party.
- Data is never sold, shared, or monetized.
- No analytics, telemetry, or tracking of any kind is included.

## How Data Is Used

The WorkBuddy desktop application uses the element data to:

1. Provide the AI tutor with accurate information about what's on your screen.
2. Highlight specific elements in the browser when the tutor points at them.

Element data is held in memory only for the duration of the tutoring session.
It is not written to disk, not stored in any database, and is discarded when
new data arrives or the application closes.

## Authentication

Communication between the extension and the desktop application is
authenticated using a locally-generated token. This token is stored on your
computer in your operating system's configuration directory and is never
transmitted over the network.

## Permissions Explained

| Permission | Why It's Needed |
|------------|----------------|
| `storage` | Stores the authentication token and port number locally in the browser so you don't have to re-enter them. |
| `host_permissions` (127.0.0.1) | Allows the extension to communicate with the WorkBuddy desktop app running on your computer. |
| Content script (matched domains) | Reads the page structure on specific educational sites to provide contextual tutoring. |

## Data Retention

- **In-browser:** The authentication token and port number are stored in
  `chrome.storage.local` until you remove the extension or clear its data.
- **In the desktop app:** Element data is held in memory only. It is
  overwritten every 3 seconds with fresh data and fully discarded when the
  app closes. No element data is ever written to disk.

## Children's Privacy

This extension does not knowingly collect any personal information from
children under 13. The extension collects only page structure metadata
(element types and positions), not personal content.

## Your Rights

- **Uninstall** the extension at any time to stop all data collection.
- **Clear data** via chrome://extensions → WorkBuddy Screen Reader → Details
  → Clear data to remove the stored token and port.
- **Inspect traffic** — all communication goes to `127.0.0.1` (your own
  machine). You can verify this with browser developer tools (Network tab).

## Open Source

This extension is open source under the AGPL-3.0 license. You can review
the complete source code at:
https://github.com/Frostbite1536/WorkBuddy/tree/main/workbuddy-extension

## Changes to This Policy

If this policy is updated, the "Last updated" date at the top will change.
Significant changes will be noted in the extension's changelog.

## Contact

For questions about this privacy policy or the extension's data practices,
open an issue at: https://github.com/Frostbite1536/WorkBuddy/issues
