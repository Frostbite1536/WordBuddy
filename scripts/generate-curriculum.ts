/**
 * scripts/generate-curriculum.ts
 *
 * Build-time JSON generator consumed by `src-tauri/workbuddy-mcp`.
 *
 * Reads the TypeScript curriculum sources (the single source of truth for
 * module → snippet mapping, snippet content, per-module UI element blurbs)
 * plus the bundled lesson plans from `../limitless-academy/**` and writes a
 * self-contained JSON document that the Rust MCP binary bundles via
 * `include_str!`.
 *
 * Run with: `npx tsx scripts/generate-curriculum.ts`
 *
 * Output: `src-tauri/workbuddy-mcp/curriculum.json`
 *
 * Keep this script dependency-free beyond `tsx` + Node stdlib.
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// Dynamic imports of the curriculum modules — tsx handles TS on the fly.
import { CONTEXT_REGISTRY, MODULE_CONTEXT_MAP } from "../src/lib/curriculum/context/module_map.ts";
import { UI_ELEMENTS } from "../src/lib/curriculum/context/ui_elements.ts";
import { LIMITLESS_API_REFERENCE } from "../src/lib/curriculum/context/limitless_api_reference.ts";
import { LIMITLESS_PLATFORM } from "../src/lib/curriculum/context/limitless_platform.ts";
import { MARKET_MECHANICS } from "../src/lib/curriculum/context/market_mechanics.ts";
import { PROGRAMMATIC_API } from "../src/lib/curriculum/context/programmatic_api.ts";
import { SDK_TYPESCRIPT } from "../src/lib/curriculum/context/sdk_typescript.ts";
import { SDK_PYTHON } from "../src/lib/curriculum/context/sdk_python.ts";
import { SDK_GO } from "../src/lib/curriculum/context/sdk_go.ts";
import { LIMITLESS_CLI } from "../src/lib/curriculum/context/limitless_cli.ts";
import { LIMITLESS_FEEDS_CLI } from "../src/lib/curriculum/context/limitless_feeds_cli.ts";
import { API_TIER_FOUNDATIONS } from "../src/lib/curriculum/context/api_tier_foundations.ts";
import { API_TIER_REALTIME } from "../src/lib/curriculum/context/api_tier_realtime.ts";
import { API_TIER_DATA } from "../src/lib/curriculum/context/api_tier_data.ts";
import { API_TIER_PRODUCTION } from "../src/lib/curriculum/context/api_tier_production.ts";
import { AGENTS_TIER_FOUNDATIONS } from "../src/lib/curriculum/context/agents_tier_foundations.ts";
import { AGENTS_TIER_BUILDING } from "../src/lib/curriculum/context/agents_tier_building.ts";
import { AGENTS_TIER_PRODUCTION } from "../src/lib/curriculum/context/agents_tier_production.ts";

// ── Module titles + tiers (mirrored from src-tauri/src/context.rs) ──────
// Kept in sync manually with context.rs `match_curriculum_context`.

const MODULE_META: Record<string, Record<string, { title: string; tier: string }>> = {
  pm_academy: {
    "01": { title: "Prediction Markets 101", tier: "Fundamentals" },
    "02": { title: "Implied Leverage", tier: "Fundamentals" },
    "03": { title: "Risk Management", tier: "Fundamentals" },
    "04": { title: "Hedging", tier: "Fundamentals" },
    "05": { title: "Order Book Mechanics", tier: "Advanced" },
    "06": { title: "Resolution & Settlement", tier: "Advanced" },
    "07": { title: "Market Analysis", tier: "Advanced" },
    "08": { title: "Portfolio Construction", tier: "Advanced" },
    "09": { title: "Arbitrage", tier: "Advanced" },
    "10": { title: "Sportsbook", tier: "Football" },
    "11": { title: "Football Analysis", tier: "Football" },
    "12": { title: "Social Alpha", tier: "Football" },
    "13": { title: "First Football Trade", tier: "Football" },
    "14": { title: "Crypto Market Structure", tier: "Crypto" },
    "15": { title: "Crypto Sentiment", tier: "Crypto" },
    "16": { title: "Volatility", tier: "Crypto" },
    "17": { title: "First Crypto Trade", tier: "Crypto" },
    "18": { title: "Equities", tier: "Equities" },
    "19": { title: "Macro", tier: "Equities" },
    "20": { title: "Earnings", tier: "Equities" },
    "21": { title: "Equities Trade", tier: "Equities" },
    "22": { title: "The 15-Minute Game", tier: "Speed" },
    "23": { title: "Hourly Market Strategies", tier: "Speed" },
    "24": { title: "Multi-Timeframe", tier: "Speed" },
  },
  api_academy: {
    "01": { title: "Infrastructure", tier: "Foundations" },
    "02": { title: "Trader Control Panel", tier: "Foundations" },
    "03": { title: "API 101", tier: "API basics" },
    "04": { title: "Markets", tier: "API basics" },
    "05": { title: "Orders", tier: "API basics" },
    "06": { title: "Positions", tier: "API basics" },
    "07": { title: "Websockets", tier: "Real-Time" },
    "08": { title: "Order Book Streams", tier: "Real-Time" },
    "09": { title: "Rate Limits", tier: "Real-Time" },
    "10": { title: "Error Handling", tier: "Real-Time" },
    "11": { title: "Historical Data", tier: "Data" },
    "12": { title: "Backtesting Framework", tier: "Data" },
    "13": { title: "PnL Analysis", tier: "Data" },
    "14": { title: "Risk Metrics", tier: "Data" },
    "15": { title: "Market Making", tier: "Production" },
    "16": { title: "Arbitrage", tier: "Production" },
    "17": { title: "Signal-Based Strategies", tier: "Production" },
    "18": { title: "Production Bot", tier: "Production" },
  },
  agents_academy: {
    "01": { title: "Infrastructure", tier: "Foundations" },
    "02": { title: "Your Dashboard", tier: "Foundations" },
    "03": { title: "Crash Course", tier: "Foundations" },
    "04": { title: "LLM Tool Use", tier: "Foundations" },
    "05": { title: "Agent Loop", tier: "Foundations" },
    "06": { title: "Memory & State", tier: "Foundations" },
    "07": { title: "limitless-cli", tier: "Building" },
    "08": { title: "Feed Health Checks", tier: "Building" },
    "09": { title: "agents-starter", tier: "Building" },
    "10": { title: "Custom Skills", tier: "Building" },
    "11": { title: "Deployment", tier: "Production" },
    "12": { title: "Monitoring", tier: "Production" },
    "13": { title: "Testing", tier: "Production" },
    "14": { title: "Kill Switches", tier: "Production" },
    "15": { title: "Prompt Injection", tier: "Production" },
    "16": { title: "First Agent", tier: "Production" },
  },
};

// ── Tier bundles (mirrors getContextReference in index.ts) ─────────────

const TIER_BUNDLES = {
  pm_academy: {
    default: LIMITLESS_PLATFORM + "\n" + MARKET_MECHANICS,
  },
  api_academy: {
    Foundations:
      LIMITLESS_API_REFERENCE + "\n" + API_TIER_FOUNDATIONS +
      "\n" + SDK_TYPESCRIPT + "\n" + SDK_PYTHON + "\n" + SDK_GO,
    "API basics":
      LIMITLESS_API_REFERENCE + "\n" + API_TIER_FOUNDATIONS +
      "\n" + SDK_TYPESCRIPT + "\n" + SDK_PYTHON + "\n" + SDK_GO,
    "Real-Time":
      LIMITLESS_API_REFERENCE + "\n" + API_TIER_REALTIME +
      "\n" + SDK_TYPESCRIPT + "\n" + SDK_PYTHON + "\n" + SDK_GO,
    Data:
      LIMITLESS_API_REFERENCE + "\n" + API_TIER_DATA +
      "\n" + SDK_TYPESCRIPT + "\n" + SDK_PYTHON + "\n" + SDK_GO,
    Production:
      LIMITLESS_API_REFERENCE + "\n" + API_TIER_PRODUCTION + "\n" + PROGRAMMATIC_API +
      "\n" + SDK_TYPESCRIPT + "\n" + SDK_PYTHON + "\n" + SDK_GO,
  },
  agents_academy: {
    Foundations:
      LIMITLESS_API_REFERENCE + "\n" + AGENTS_TIER_FOUNDATIONS +
      "\n" + SDK_TYPESCRIPT + "\n" + SDK_PYTHON,
    Building:
      LIMITLESS_API_REFERENCE + "\n" + AGENTS_TIER_BUILDING +
      "\n" + LIMITLESS_CLI + "\n" + LIMITLESS_FEEDS_CLI +
      "\n" + SDK_TYPESCRIPT + "\n" + SDK_PYTHON,
    Production:
      LIMITLESS_API_REFERENCE + "\n" + AGENTS_TIER_PRODUCTION + "\n" + PROGRAMMATIC_API +
      "\n" + SDK_TYPESCRIPT + "\n" + SDK_PYTHON,
  },
  limitless_trader_lab: {
    default:
      LIMITLESS_PLATFORM + "\n" + LIMITLESS_API_REFERENCE +
      "\n" + SDK_TYPESCRIPT + "\n" + SDK_PYTHON + "\n" + SDK_GO,
  },
};

// ── Lesson plans ────────────────────────────────────────────────────────

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.resolve(__dirname, "..");

// Paths read from the sibling limitless-academy repo (the canonical
// editorial source after the PM_Academy → Limitless Academy rename).
// The bundled copies under src-tauri/lesson_plans/ remain the runtime
// source — see scripts/sync-lesson-plans.{sh,ps1}.
//
// Resolution mirrors sync-lesson-plans.sh: ROOT is the workbuddy-followup
// repo root, and limitless-academy lives one level up as a sibling
// directory. The pre-rename script used "../../PM_Academy" assuming an
// older nested layout; the new install convention is sibling clones.
const LESSON_PLAN_ROOTS: Record<string, string> = {
  pm_academy: path.resolve(ROOT, "../limitless-academy/academies/pm_academy/lesson_plans"),
  api_academy: path.resolve(ROOT, "../limitless-academy/academies/api_academy/lesson_plans"),
  agents_academy: path.resolve(ROOT, "../limitless-academy/academies/agents_academy/lesson_plans"),
  limitless_trader_lab: path.resolve(ROOT, "../limitless-academy/programs/limitless_trader_lab/lesson_plans"),
};

function collectLessonPlans(program: string, dir: string): Record<string, string> {
  const plans: Record<string, string> = {};
  if (!fs.existsSync(dir)) {
    console.warn(`[gen] lesson plans dir missing for ${program}: ${dir}`);
    return plans;
  }
  for (const file of fs.readdirSync(dir)) {
    if (!file.endsWith(".md")) continue;
    // Match either "NN_Name.md" (academy) or "day-N.md" (Trader Lab).
    const academyMatch = file.match(/^(\d{2})_/);
    const dayMatch = file.match(/^(day-\d+)\.md$/);
    const key = academyMatch?.[1] ?? dayMatch?.[1];
    if (!key) continue;
    plans[key] = fs.readFileSync(path.join(dir, file), "utf-8");
  }
  return plans;
}

const lessonPlans: Record<string, Record<string, string>> = {};
for (const [program, dir] of Object.entries(LESSON_PLAN_ROOTS)) {
  lessonPlans[program] = collectLessonPlans(program, dir);
}

// ── Compose modules with has_lesson_plan flag ──────────────────────────

const programs: Record<string, unknown> = {};
for (const [program, modules] of Object.entries(MODULE_META)) {
  const moduleEntries: Record<string, unknown> = {};
  for (const [id, meta] of Object.entries(modules)) {
    moduleEntries[id] = {
      title: meta.title,
      tier: meta.tier,
      snippet_keys: MODULE_CONTEXT_MAP[program]?.[id] ?? [],
      has_lesson_plan: id in (lessonPlans[program] ?? {}),
    };
  }
  programs[program] = {
    modules: moduleEntries,
    tier_bundles: TIER_BUNDLES[program as keyof typeof TIER_BUNDLES] ?? {},
  };
}

// Trader Lab: no fixed module IDs; module_map has no entries. Provide
// tier_bundles for the resolver + pass-through lesson plans.
programs["limitless_trader_lab"] = {
  modules: {},
  tier_bundles: TIER_BUNDLES.limitless_trader_lab,
};

// ── Final document ──────────────────────────────────────────────────────

const curriculum = {
  schema_version: 1,
  generated_at: new Date().toISOString(),
  programs,
  snippets: CONTEXT_REGISTRY,
  ui_elements: UI_ELEMENTS,
  lesson_plans: lessonPlans,
};

const out = path.resolve(ROOT, "src-tauri/workbuddy-mcp/curriculum.json");
fs.writeFileSync(out, JSON.stringify(curriculum, null, 2));

const sizeKb = (fs.statSync(out).size / 1024).toFixed(1);
const moduleCount = Object.values(MODULE_META).reduce((n, p) => n + Object.keys(p).length, 0);
const snippetCount = Object.keys(CONTEXT_REGISTRY).length;
const planCount = Object.values(lessonPlans).reduce((n, p) => n + Object.keys(p).length, 0);

console.log(
  `[gen] Wrote ${out}\n` +
    `      ${sizeKb} KB | ${moduleCount} modules | ${snippetCount} snippets | ${planCount} lesson plans`,
);
