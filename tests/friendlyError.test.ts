// Coverage for the LLM-error → user-readable mapper. Each branch
// is hit so a future contributor adding a category sees the
// matching test slot. Falls back to a "Details:" line for the
// uncategorized case.

import { describe, it, expect } from "vitest";
import { friendlyStreamError } from "../src/lib/friendlyError";

describe("friendlyStreamError", () => {
  it("maps 401 / unauthorized / invalid_api_key to API key guidance", () => {
    const cases = [
      "401 Unauthorized",
      "unauthorized",
      "Invalid API key",
      "invalid_api_key",
      "missing x-api-key",
    ];
    for (const c of cases) {
      expect(friendlyStreamError(c)).toMatch(/API key was rejected/i);
    }
  });

  it("maps 429 / rate.limit to rate-limit guidance", () => {
    expect(friendlyStreamError("429 Too Many Requests")).toMatch(/rate-limited/i);
    expect(friendlyStreamError("rate limit exceeded")).toMatch(/rate-limited/i);
  });

  it("maps quota / billing / out of credit to billing guidance", () => {
    expect(friendlyStreamError("insufficient_credit_balance")).toMatch(/out of credits/i);
    expect(friendlyStreamError("billing issue")).toMatch(/out of credits/i);
    expect(friendlyStreamError("quota exceeded")).toMatch(/out of credits/i);
  });

  it("maps 5xx / internal server error to transient-error guidance", () => {
    expect(friendlyStreamError("502 Bad Gateway")).toMatch(/server error/i);
    expect(friendlyStreamError("503 Service Unavailable")).toMatch(/server error/i);
    expect(friendlyStreamError("Internal Server Error")).toMatch(/server error/i);
  });

  it("maps DNS / network / TLS / timeout errors to connectivity guidance", () => {
    const cases = [
      "dns error: failed to lookup",
      "Connection refused",
      "request timed out",
      "network unreachable",
      "tls handshake failure",
    ];
    for (const c of cases) {
      expect(friendlyStreamError(c)).toMatch(/network|connection/i);
    }
  });

  it("maps Tauri/IPC errors to backend-rejection guidance", () => {
    expect(friendlyStreamError("tauri command rejected")).toMatch(/backend rejected/i);
    expect(friendlyStreamError("invoke failed")).toMatch(/backend rejected/i);
  });

  it("falls through to Details: for unrecognized errors with the raw text", () => {
    const out = friendlyStreamError("flux capacitor jammed");
    expect(out).toMatch(/Something went wrong/);
    expect(out).toContain("flux capacitor jammed");
  });

  it("accepts Error instances and unwraps message", () => {
    const e = new Error("401 Unauthorized");
    expect(friendlyStreamError(e)).toMatch(/API key was rejected/i);
  });

  it("truncates very long fallback details to 300 chars", () => {
    const long = "x".repeat(500);
    const out = friendlyStreamError(long);
    // Full message should NOT be present verbatim — it gets sliced.
    expect(out.length).toBeLessThan(500);
  });
});
