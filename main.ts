import { type MemoizationCacheResult, memoize, TtlCache } from "@std/cache";
import { MINUTE } from "@std/datetime";
import "@std/dotenv/load";
import { Hono } from "hono";
import { every } from "hono/combine";
import { compress } from "hono/compress";
import { cors } from "hono/cors";
import { serveStatic } from "hono/deno";
import { logger } from "hono/logger";
import UserAgent from "user-agents";
import { generate } from "youtube-po-token-generator";
import { Innertube, UniversalCache } from "youtubei.js";

const app = new Hono();

const HOST = Deno.env.get("HOST") || Deno.env.get("HOSTNAME") || "127.0.0.1";
const PORT = Number(Deno.env.get("PORT")) || 3000;

const API_KEY = Deno.env.get("API_KEY");

const CACHE_TTL = Number.isNaN(Number(Deno.env.get("CACHE_TTL")))
  ? 30
  : Number(Deno.env.get("CACHE_TTL"));
const USE_CACHE = CACHE_TTL > 0;
const CACHE_DIR = Deno.env.get("CACHE_DIR") || "./.cache";

const TTL_CACHE = new TtlCache<string, MemoizationCacheResult<any>>(
  CACHE_TTL * MINUTE,
);

const USER_AGENT = new UserAgent();
const { poToken: po_token, visitorData: visitor_data } = await generate();

// Session
const session = await Innertube.create({
  lang: "en",
  location: "US",
  user_agent: USER_AGENT.toString(),
  po_token,
  visitor_data,
  device_category: USER_AGENT?.data?.deviceCategory === "desktop"
    ? "desktop"
    : "mobile",
  cache: USE_CACHE ? new UniversalCache(true, CACHE_DIR) : undefined,
});

// Allowed params
const allowedParams = ["v", "c", "x"] as const;
type T_Params = { type: (typeof allowedParams)[number]; value: string };

// -----------------------------
// Memoized stream validator
// -----------------------------
const validateStream = memoize(
  async (param: T_Params): Promise<string | undefined> => {
    try {
      // HEAD is faster than GET for validation
      const response = await fetch(param.value, { method: "HEAD" }).catch(() =>
        fetch(param.value, { method: "GET" })
      );
      if (response.status < 400) return param.value;
    } catch (error) {
      console.error(`-> ❌ Failed to resolve stream: ${param.value}\n${error}`);
    }
  },
  { cache: USE_CACHE ? TTL_CACHE : undefined },
);

// -----------------------------
// Video ID resolver
// -----------------------------
async function getVideoId(url: string): Promise<string | undefined> {
  try {
    return (await session.resolveURL(url))?.payload?.videoId as
      | string
      | undefined;
  } catch (error) {
    console.error(`-> ❌ Failed to get videoId for ${url}\n${error}`);
  }
}

// -----------------------------
// Video Info resolver
// -----------------------------
const getVideoInfo = memoize(
  async (vid: string): Promise<string | undefined> => {
    try {
      const info = await session.getInfo(vid);
      return info?.streaming_data?.hls_manifest_url;
    } catch (error) {
      console.error(`-> ❌ Failed to fetch info for video: ${vid}\n${error}`);
    }
  },
  { cache: USE_CACHE ? TTL_CACHE : undefined },
);

// -----------------------------
// HLS Stream resolver
// -----------------------------
async function getHslStream(param: T_Params): Promise<string | undefined> {
  try {
    if (param.type === "x") {
      return await validateStream(param);
    }

    const vid = param.type === "v"
      ? param.value
      : await getVideoId(`https://www.youtube.com/${param.value}/live`);
    if (!vid) return undefined;

    return await getVideoInfo(vid);
  } catch (error) {
    console.error(
      `-> ❌ Failed to get HSL stream for param: ${param.value}\n${error}`,
    );
  }
}

// -----------------------------
// Middleware
// -----------------------------
app.use(
  "/*",
  every(
    logger(),
    compress(),
    cors({
      origin: Deno.env.get("CORS_ORIGIN") || "*",
      allowMethods: ["GET", "POST", "OPTIONS"],
    }),
  ),
);

if (Deno.args.includes("--health")) {
  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 5000); // 5 sec timeout

    const res = await fetch(`http://${HOST}:${PORT}/health`, {
      signal: controller.signal,
    });
    clearTimeout(timeout);

    if (res.ok) {
      console.log("Health check OK");
      Deno.exit(0); // healthy
    } else {
      console.error("Health check failed:", res.status);
      Deno.exit(1); // unhealthy
    }
  } catch (err) {
    console.error("Health check failed:", err);
    Deno.exit(1); // unhealthy
  }
}

app.get("/health", (c) => {
  return c.json({ status: "ok" }, 200);
});

// -----------------------------
// Favicon
// -----------------------------
app.get("/favicon.ico", serveStatic({ path: "./favicon.ico" }));

// -----------------------------
// Main endpoint
// -----------------------------
app.get("/", async (c) => {
  const hasAccess = API_KEY
    ? c.req.header("Authorization") === `Bearer ${API_KEY}` ||
      c.req.header("User-Agent")?.includes(API_KEY)
    : true;

  if (!hasAccess) {
    return c.json(
      { success: false, error: "Unauthorized. Missing API key!" },
      401,
    );
  }

  const url = new URL(c.req.url);
  const params: T_Params[] = [];

  for (const [key, value] of url.searchParams.entries()) {
    if ((allowedParams as readonly string[]).includes(key)) {
      params.push({ type: key as T_Params["type"], value });
    } else if (params.length > 0) {
      params[params.length - 1].value += `&${key}=${value}`;
    }
  }

  if (!params.length) {
    return c.json({ success: true, message: "Server running!" }, 200);
  }

  for (const param of params) {
    const streamUrl = await getHslStream(param);
    if (streamUrl) {
      return c.redirect(streamUrl);
    }
  }

  return c.json({ success: false, error: "No valid stream found!" }, 404);
});

// -----------------------------
// Clear cache endpoint
// -----------------------------
app.get("/clear-cache", async (c) => {
  const hasAccess = API_KEY
    ? c.req.header("Authorization") === `Bearer ${API_KEY}X` ||
      c.req.header("User-Agent")?.includes(`${API_KEY}X`)
    : true;

  if (!hasAccess) {
    return c.json(
      { success: false, error: "Unauthorized. Missing API key!" },
      401,
    );
  }

  if (TTL_CACHE.size) {
    TTL_CACHE.clear();
    return c.json({ success: true, message: "Cache cleared!" }, 200);
  }

  return c.json(
    { success: true, error: "Nothing to clear. Cache is empty!" },
    200,
  );
});

Deno.serve(
  {
    hostname: HOST,
    port: PORT,
  },
  app.fetch,
);
