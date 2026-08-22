# WordBuddy

A privacy-first, system-wide writing assistant (Tauri 2 desktop app +
Chrome/Edge extension): real-time checking in any text field, color-coded
inline suggestions in the browser, a floating suggestion card near the caret
in native apps, one-click corrections, AI rewrites, weekly writing analytics,
and personalization.

Correctness checking runs locally via harper-core; clarity/engagement/delivery
passes use your own LLM provider key and never see ambient keystrokes.

- Authoritative specs: [docs/plans/PLAN-INDEX.md](docs/plans/PLAN-INDEX.md)
- Shared contracts + invariants: [docs/plans/CONTRACTS.md](docs/plans/CONTRACTS.md)

## Build

```bash
npm install && npx vite build
cd src-tauri && cargo test && cargo check
npx tauri dev
```

Proprietary — all rights reserved.
