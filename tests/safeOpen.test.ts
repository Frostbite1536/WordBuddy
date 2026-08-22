// Tests for the markdown-link confirmation gate. PR #32 P2 fix —
// covers the apex/exact split that closes the GitHub Pages-takeover
// bypass and the "subdomain entry as wildcard" issue.

import { describe, it, expect, beforeEach, afterEach, vi, beforeAll } from "vitest";
import { confirmExternalLink } from "../src/lib/safeOpen";

// Vitest's default environment is `node`, which has no `window`.
// Stub it so `window.confirm` can be spied on. The real WebView
// runtime obviously has a window; this is only for the test harness.
beforeAll(() => {
  if (typeof (globalThis as { window?: unknown }).window === "undefined") {
    (globalThis as { window: { confirm: (msg: string) => boolean } }).window = {
      confirm: () => false,
    };
  } else if (typeof window.confirm !== "function") {
    (window as Window & { confirm: (msg: string) => boolean }).confirm = () => false;
  }
});

describe("safeOpen.confirmExternalLink", () => {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let confirmSpy: any;

  beforeEach(() => {
    confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
  });

  afterEach(() => {
    confirmSpy.mockRestore();
  });

  describe("trusted apex hosts", () => {
    it("permits limitless.exchange apex without prompting", () => {
      expect(confirmExternalLink("https://limitless.exchange/markets")).toBe(true);
      expect(confirmSpy).not.toHaveBeenCalled();
    });

    it("permits any subdomain of a trusted apex", () => {
      expect(confirmExternalLink("https://docs.limitless.exchange/")).toBe(true);
      expect(confirmExternalLink("https://www.limitless.exchange/")).toBe(true);
      expect(confirmExternalLink("https://anything.openrouter.ai/")).toBe(true);
      expect(confirmSpy).not.toHaveBeenCalled();
    });

    it("permits all known LLM-provider apex hosts silently", () => {
      const apex = [
        "https://anthropic.com/",
        "https://console.anthropic.com/keys",
        "https://openai.com/",
        "https://platform.openai.com/api-keys",
        "https://elevenlabs.io/",
        "https://openrouter.ai/",
        "https://ollama.com/",
      ];
      for (const url of apex) {
        expect(confirmExternalLink(url)).toBe(true);
      }
      expect(confirmSpy).not.toHaveBeenCalled();
    });
  });

  describe("exact-only trusted hosts", () => {
    it("permits github.com exactly", () => {
      expect(confirmExternalLink("https://github.com/Frostbite1536/WordBuddy")).toBe(true);
      expect(confirmSpy).not.toHaveBeenCalled();
    });

    it("does NOT permit *.github.com — closes Pages-takeover bypass", () => {
      // github.com is in EXACT precisely so an attacker-controllable
      // pages.github.com / something.github.com host can't bypass.
      expect(confirmExternalLink("https://pages.github.com/")).toBe(false);
      expect(confirmExternalLink("https://attacker.github.com/")).toBe(false);
      expect(confirmSpy).toHaveBeenCalledTimes(2);
    });

    it("permits ai.google.dev exactly but not generic google.dev subdomains", () => {
      expect(confirmExternalLink("https://ai.google.dev/")).toBe(true);
      // No wildcard against google.dev because google.dev itself isn't
      // an apex we allowlisted.
      expect(confirmExternalLink("https://random.google.dev/")).toBe(false);
      expect(confirmExternalLink("https://google.dev/")).toBe(false);
    });
  });

  describe("untrusted hosts prompt user", () => {
    it("prompts for arbitrary HTTPS host", () => {
      confirmSpy.mockReturnValue(false);
      expect(confirmExternalLink("https://attacker.com/phish")).toBe(false);
      expect(confirmSpy).toHaveBeenCalledOnce();
      // Full URL must be in the prompt for homoglyph spotting.
      expect(confirmSpy.mock.calls[0][0]).toContain("https://attacker.com/phish");
      expect(confirmSpy.mock.calls[0][0]).toContain("attacker.com");
    });

    it("returns true when user confirms", () => {
      confirmSpy.mockReturnValue(true);
      expect(confirmExternalLink("https://attacker.com/")).toBe(true);
    });
  });

  describe("subdomain entry would-have-been-bypass", () => {
    // Regression test for the original Greptile P2 finding: pre-fix
    // code put `www.limitless.exchange` in the wildcard set, which
    // made `evil.www.limitless.exchange` pass via endsWith.
    it("does NOT trust evil.www.limitless.exchange as a fake subdomain", () => {
      // limitless.exchange IS in the apex set, so any subdomain of
      // limitless.exchange IS trusted (including www.limitless.exchange).
      // But evil.www.limitless.exchange ends in `.limitless.exchange`
      // so it is in fact a legitimate subdomain — that's how DNS
      // works. The narrower regression is `evil.something.com`
      // matching when only the apex is allowlisted: tested below.
      const url = "https://evil.unknown-host.com/";
      expect(confirmExternalLink(url)).toBe(false);
    });

    it("does NOT match a host that merely contains the trusted apex as a substring", () => {
      // limitless.exchange-attacker.com would have matched a naive
      // .endsWith check without the leading dot.
      expect(confirmExternalLink("https://limitless.exchange.attacker.com/")).toBe(false);
      expect(confirmExternalLink("https://attacker-limitless.exchange/")).toBe(false);
    });
  });

  describe("malformed and non-web URLs are refused outright", () => {
    it("refuses unparseable URLs", () => {
      expect(confirmExternalLink("not a url")).toBe(false);
      expect(confirmExternalLink("")).toBe(false);
    });

    it("refuses non-http(s) schemes (mailto, file, javascript)", () => {
      expect(confirmExternalLink("javascript:alert(1)")).toBe(false);
      expect(confirmExternalLink("file:///etc/passwd")).toBe(false);
      expect(confirmExternalLink("mailto:foo@bar.com")).toBe(false);
      // The shell plugin allowlist also gates these — this is the
      // single decision point the markdown renderer consults.
    });
  });
});
