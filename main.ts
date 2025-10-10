import {
  LruCache,
  type MemoizationCacheResult,
  memoize,
  TtlCache,
} from "@std/cache";
import { MINUTE } from "@std/datetime";
import "@std/dotenv/load";
import { format } from "@std/fmt/bytes";
import { Hono } from "hono";
import { bearerAuth } from "hono/bearer-auth";
import { every, some } from "hono/combine";
import { compress } from "hono/compress";
import { cors } from "hono/cors";
import { serveStatic } from "hono/deno";
import { logger } from "hono/logger";
import UserAgent from "user-agents";
import { generate } from "youtube-po-token-generator";
import { Innertube, UniversalCache } from "youtubei.js";

const app = new Hono();

const BASE_URL = new URL(
  `http://${Deno.env.get("HOSTNAME") || "localhost"}:${
    Deno.env.get("PORT") || 3000
  }`,
);
const API_KEY = Deno.env.get("API_KEY");

const STREAM_VALIDATION_CACHE_TTL = Number(
  Deno.env.get("STREAM_VALIDATION_CACHE_TTL") || 30,
);
// Stream Validation Cache (LRU)
const STREAM_VALIDATION_CACHE = new LruCache<
  string,
  MemoizationCacheResult<Promise<string | undefined>>
>(STREAM_VALIDATION_CACHE_TTL * MINUTE);

const VIDEO_INFO_CACHE_SIZE = Number(
  Deno.env.get("VIDEO_INFO_CACHE_SIZE") || 1_00_000,
);
// Video Info Cache (TTL)
const VIDEO_INFO_CACHE = new TtlCache<
  string,
  MemoizationCacheResult<Promise<string | undefined>>
>(VIDEO_INFO_CACHE_SIZE * MINUTE);

const USE_CACHE = !!(STREAM_VALIDATION_CACHE_TTL || VIDEO_INFO_CACHE_SIZE);
const CACHE_DIR = Deno.env.get("CACHE_DIR") || "./.cache";

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
// Stream validator (memoized)
// -----------------------------
const validateStream = memoize(
  async (url: string): Promise<string | undefined> => {
    try {
      console.log(`🔍 Validating stream: ${url}`);
      const urlObj = new URL(url);
      const headers: Record<string, string> = {
        "user-agent": USER_AGENT.toString(),
      };
      const customHeaderKey = Deno.env.get("CUSTOM_X_HEADER") ||
        "custom-header";
      const customHeaders = urlObj.searchParams.get(customHeaderKey);
      if (customHeaders) {
        customHeaders.split(",").forEach((pair) => {
          const [key, ...valueParts] = pair.split(":");
          if (key && valueParts.length) {
            headers[key] = valueParts.join(":");
          }
        });
        urlObj.searchParams.delete(customHeaderKey);
      }
      const cleanUrl = urlObj.toString();
      const options: RequestInit = { method: "HEAD" };
      if (Object.keys(headers).length > 0) options.headers = headers;
      const response = await fetch(cleanUrl, options).catch(() =>
        fetch(cleanUrl, { ...options, method: "GET" })
      );
      if (response.status < 400) return cleanUrl;
    } catch (error) {
      console.error(`-> ❌ Failed to resolve stream: ${url}\n${error}`);
    }
  },
  {
    cache: USE_CACHE ? STREAM_VALIDATION_CACHE : undefined,
    getKey: (url: string) => `stream:${url}`,
  },
);

// -----------------------------
// Video info (memoized)
// -----------------------------
const getVideoInfo = memoize(
  async (vid: string): Promise<string | undefined> => {
    try {
      console.log(`🎥 Fetching info for video: ${vid}`);
      const info = await session.getInfo(vid);
      return info?.streaming_data?.hls_manifest_url;
    } catch (error) {
      console.error(`-> ❌ Failed to fetch info for video: ${vid}\n${error}`);
    }
  },
  {
    cache: USE_CACHE ? VIDEO_INFO_CACHE : undefined,
    getKey: (vid: string) => `video:${vid}`,
  },
);

const getVideoId = memoize(
  async (url: string): Promise<string | undefined> => {
    try {
      console.log(`🔗 Resolving URL: ${url}`);
      return (await session.resolveURL(url))?.payload?.videoId as
        | string
        | undefined;
    } catch (error) {
      console.error(`-> ❌ Failed to get videoId for ${url}\n${error}`);
    }
  },
  {
    cache: USE_CACHE ? VIDEO_INFO_CACHE : undefined,
    getKey: (url: string) => `resolve:${url}`,
  },
);

// -----------------------------
// HLS Stream resolver
// -----------------------------
async function getHslStream(param: T_Params): Promise<string | undefined> {
  try {
    if (param.type === "x") {
      // Direct URL validation
      return await validateStream(param.value);
    }

    if (param.type === "v") {
      // Direct video ID
      return await getVideoInfo(param.value);
    }

    if (param.type === "c") {
      // Channel/URL resolution
      const vid = await getVideoId(
        `https://www.youtube.com/${param.value}/live`,
      );
      if (!vid) return undefined;
      return await getVideoInfo(vid);
    }
  } catch (error) {
    console.error(
      `-> ❌ Failed to get HSL stream for ${param.type}:${param.value}\n${error}`,
    );
  }
}

// -----------------------------
// Middleware
// -----------------------------
app.use(
  "*",
  every(
    logger(),
    compress(),
    cors({
      origin: Deno.env.get("CORS_ORIGIN") || "*",
      allowMethods: ["GET", "POST", "OPTIONS"],
    }),
    some(
      () => !API_KEY,
      bearerAuth({
        verifyToken: (token: string) => {
          if (!API_KEY) return true;
          return token.includes(API_KEY);
        },
        prefix: "",
        headerName: "User-Agent",
      }),
    ),
  ),
);

if (Deno.args.includes("--health")) {
  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 5000); // 5 sec timeout

    const res = await fetch(new URL("/health", BASE_URL), {
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
    return c.json({
      message: "Server running!",
      memory: Object.fromEntries(
        Object.entries(Deno.memoryUsage()).map((
          [key, value],
        ) => [key, format(value)]),
      ),
      cache: USE_CACHE
        ? {
          streamValidation: STREAM_VALIDATION_CACHE.size,
          videoInfo: VIDEO_INFO_CACHE.size,
          total: STREAM_VALIDATION_CACHE.size + VIDEO_INFO_CACHE.size,
        }
        : undefined,
    }, 200);
  }

  for (const param of params) {
    const streamUrl = await getHslStream(param);
    if (streamUrl) {
      console.log(`✅ Stream found for ${param.type}:${param.value}`);
      return c.redirect(streamUrl);
    }
  }

  return c.json({ success: false, error: "No valid stream found!" }, 404);
});

Deno.serve(
  {
    hostname: BASE_URL.hostname,
    port: Number(BASE_URL.port),
    onListen: () => {
      console.log(`\n🚀 Server is running on ${BASE_URL}`);
      console.log(
        `🔐 Authentication: ${API_KEY ? `ENABLED (${API_KEY})` : "DISABLED"}`,
      );
      console.log(
        `💾 Caching: ${USE_CACHE ? "ENABLED" : "DISABLED"}`,
      );
    },
  },
  app.fetch,
);
