import { type MemoizationCacheResult, memoize, TtlCache } from "@std/cache";
import { MINUTE } from "@std/datetime";
import { format as formatBytes } from "@std/fmt/bytes";
import { format as formatDuration } from "@std/fmt/duration";
import { Hono } from "hono";
import { bearerAuth } from "hono/bearer-auth";
import { serveStatic } from "hono/bun";
import { every, some } from "hono/combine";
import { compress } from "hono/compress";
import { cors } from "hono/cors";
import { logger } from "hono/logger";
import UserAgent from "user-agents";
import { generate } from "youtube-po-token-generator";
import { Innertube, UniversalCache } from "youtubei.js";

// ─────────────────────────────────────────────
// Config
// ─────────────────────────────────────────────

const BASE_URL = new URL(
  `http://${Bun.env.HOSTNAME ?? "localhost"}:${Bun.env.PORT ?? 3000}`,
);

const API_KEY = Bun.env.API_KEY;
const COOKIE = Bun.env.COOKIE;
const PO_TOKEN = Bun.env.PO_TOKEN;
const VISITOR_DATA = Bun.env.VISITOR_DATA;
const CACHE_DIR = Bun.env.CACHE_DIR ?? "./.cache";
const CUSTOM_HEADER_KEY = Bun.env.CUSTOM_X_HEADER ?? "custom-header";
const MEMOIZATION_TTL = Number(Bun.env.MEMOIZATION_TTL ?? 30) * MINUTE;
const MEMOIZE = MEMOIZATION_TTL > 0;

type MemoCache = TtlCache<
  string,
  MemoizationCacheResult<Promise<string | undefined>>
>;

const cache: MemoCache | undefined = MEMOIZE
  ? new TtlCache<string, MemoizationCacheResult<Promise<string | undefined>>>(
      MEMOIZATION_TTL,
    )
  : undefined;

// ─────────────────────────────────────────────
// Allowed query params
// ─────────────────────────────────────────────

const ALLOWED_PARAMS = ["v", "c", "x"] as const;
type ParamType = (typeof ALLOWED_PARAMS)[number];
type Param = { type: ParamType; value: string };

// ─────────────────────────────────────────────
// YT session
// ─────────────────────────────────────────────

const userAgent = new UserAgent();
const { poToken: po_token, visitorData: visitor_data } = await generate();

const session = await Innertube.create({
  lang: "en",
  location: "US",
  user_agent: userAgent.toString(),
  cookie: COOKIE,
  po_token: PO_TOKEN ?? po_token,
  visitor_data: VISITOR_DATA ?? visitor_data,
  device_category:
    userAgent.data?.deviceCategory === "desktop" ? "desktop" : "mobile",
  cache: MEMOIZE ? new UniversalCache(true, CACHE_DIR) : undefined,
});

// ─────────────────────────────────────────────
// Stream resolvers (memoized)
// ─────────────────────────────────────────────

const validateStream = memoize(
  async (url: string): Promise<string | undefined> => {
    try {
      console.log(`🔍 Validating stream: ${url}`);
      const urlObj = new URL(url);
      const headers: Record<string, string> = {
        "user-agent": userAgent.toString(),
      };

      // Parse any custom headers encoded in the URL params
      const customHeaders = urlObj.searchParams.get(CUSTOM_HEADER_KEY);
      if (customHeaders) {
        for (const pair of customHeaders.split(",")) {
          const colonIndex = pair.indexOf(":");
          if (colonIndex !== -1) {
            headers[pair.slice(0, colonIndex)] = pair.slice(colonIndex + 1);
          }
        }
        urlObj.searchParams.delete(CUSTOM_HEADER_KEY);
      }

      const cleanUrl = urlObj.toString();
      const init: RequestInit = { method: "HEAD", headers };
      const response = await fetch(cleanUrl, init).catch(() =>
        fetch(cleanUrl, { ...init, method: "GET" }),
      );

      if (response.status < 400) return cleanUrl;
    } catch (error) {
      console.error(`-> ❌ Failed to validate stream: ${url}\n${error}`);
    }
  },
  { cache, getKey: (url) => `x:${url}` },
);

const getVideoInfo = memoize(
  async (videoId: string): Promise<string | undefined> => {
    try {
      console.log(`📺 Fetching HLS stream for video: ${videoId}`);
      const info = await session.getInfo(videoId);
      return info?.streaming_data?.hls_manifest_url;
    } catch (error) {
      console.error(
        `-> ❌ Failed to fetch HLS stream for: ${videoId}\n${error}`,
      );
    }
  },
  { cache, getKey: (id) => `v:${id}` },
);

const getVideoId = memoize(
  async (channelHandle: string): Promise<string | undefined> => {
    try {
      console.log(`🔗 Resolving live stream for channel: ${channelHandle}`);
      const resolved = await session.resolveURL(
        `https://www.youtube.com/${channelHandle}/live`,
      );
      return resolved?.payload?.videoId as string | undefined;
    } catch (error) {
      console.error(
        `-> ❌ Failed to resolve live stream for: ${channelHandle}\n${error}`,
      );
    }
  },
  { cache, getKey: (handle) => `c:${handle}` },
);

// ─────────────────────────────────────────────
// HLS stream dispatcher
// ─────────────────────────────────────────────

async function resolveHlsStream(param: Param): Promise<string | undefined> {
  try {
    if (param.type === "x") return await validateStream(param.value);
    if (param.type === "v") return await getVideoInfo(param.value);
    if (param.type === "c") {
      const videoId = await getVideoId(param.value.toLowerCase());
      return videoId ? await getVideoInfo(videoId) : undefined;
    }
  } catch (error) {
    console.error(
      `-> ❌ Failed to resolve HLS for ${param.type}:${param.value}\n${error}`,
    );
  }
}

// ─────────────────────────────────────────────
// App
// ─────────────────────────────────────────────

const app = new Hono();

app.use(
  "*",
  every(
    logger((message, ...rest) => {
      console.log(
        `${message.startsWith("<-- ") ? "\n" : ""}${message}`,
        ...rest,
      );
    }),
    compress(),
    cors({
      origin: Bun.env.CORS_ORIGIN ?? "*",
      allowMethods: ["GET", "POST", "OPTIONS"],
    }),
    some(
      () => !API_KEY,
      bearerAuth({
        verifyToken: (token) => !!API_KEY && token.includes(API_KEY),
        prefix: "",
        headerName: "User-Agent",
      }),
      bearerAuth({ token: API_KEY ?? "" }),
    ),
  ),
);

app.get("/health", (c) => c.json({ status: "ok" }));

app.get("/favicon.ico", serveStatic({ path: "./favicon.ico" }));

app.get("/", async (c) => {
  const { searchParams } = new URL(c.req.url);
  const params: Param[] = [];

  for (const [key, value] of searchParams) {
    if ((ALLOWED_PARAMS as readonly string[]).includes(key)) {
      params.push({ type: key as ParamType, value });
    } else if (params.length > 0) {
      // Append unknown params back onto the last known param's value
      params[params.length - 1]!.value += `&${key}=${value}`;
    }
  }

  if (!params.length) {
    return c.json({
      message: "Server running!",
      memory: Object.fromEntries(
        Object.entries(process.memoryUsage()).map(([k, v]) => [
          k,
          formatBytes(v),
        ]),
      ),
      authentication: {
        enabled: API_KEY ? "Enabled" : "Disabled",
        header: API_KEY ? "Authorization / User-Agent" : "None",
      },
      memoization: {
        enabled: cache ? "Enabled" : "Disabled",
        entries: cache?.size,
      },
    });
  }

  for (const param of params) {
    const streamUrl = await resolveHlsStream(param);
    if (streamUrl) {
      console.log("✅ HLS stream found!");
      return c.redirect(streamUrl);
    }
  }

  return c.json({ success: false, error: "No valid stream found!" }, 404);
});

// ─────────────────────────────────────────────
// Health check CLI mode  (bun run server.ts --health)
// ─────────────────────────────────────────────

if (process.argv.includes("--health")) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 5_000);

  try {
    const res = await fetch(new URL("/health", BASE_URL), {
      signal: controller.signal,
    });
    clearTimeout(timeout);
    console.log(
      res.ok ? "Health check OK" : "Health check failed:",
      res.status,
    );
    process.exit(res.ok ? 0 : 1);
  } catch (err) {
    clearTimeout(timeout);
    console.error("Health check failed:", err);
    process.exit(1);
  }
}

// ─────────────────────────────────────────────
// Start server
// ─────────────────────────────────────────────

const server = Bun.serve({
  hostname: BASE_URL.hostname,
  port: Number(BASE_URL.port),
  fetch: app.fetch,
});

console.log(
  `\n🚀 Server started at ${new Date()}\nListening on ${server.url}\n`,
);
console.log(
  `🔐 Authentication: ${API_KEY ? `ENABLED (${API_KEY})` : "DISABLED"}`,
);
console.log(
  `💾 Memoization: ${
    MEMOIZE
      ? `ENABLED (${formatDuration(MEMOIZATION_TTL, { style: "full", ignoreZero: true })})`
      : "DISABLED"
  }`,
);
