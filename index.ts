import { type MemoizationCacheResult, memoize, TtlCache } from "@std/cache";
import { MINUTE } from "@std/datetime";
import { format as formatBytes } from "@std/fmt/bytes";
import { format as formatDuration } from "@std/fmt/duration";
import { spawn } from "bun";
import { Hono } from "hono";
import { bearerAuth } from "hono/bearer-auth";
import { every, some } from "hono/combine";
import { compress } from "hono/compress";
import { cors } from "hono/cors";
import { logger } from "hono/logger";
import UserAgent from "user-agents";

// ─────────────────────────────────────────────
// Config
// ─────────────────────────────────────────────

const BASE_URL = new URL(
  `http://${Bun.env.HOSTNAME ?? "localhost"}:${Bun.env.PORT ?? 3000}`,
);

const API_KEY = Bun.env.API_KEY;
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

const userAgent = new UserAgent();

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

const getStream = memoize(
  async (url: string): Promise<string | undefined> => {
    try {
      console.log(`📺 Fetching HLS stream for: ${url}`);

      const proc = spawn([
        "yt-dlp",
        "--js-runtimes",
        "bun:/usr/local/bin/bun",
        "--quiet",
        "--no-warnings",
        "--no-progress",
        "--no-update",
        "-f",
        "best",
        "-g",
        url,
      ]);

      const stdout = await proc.stdout.text();

      const exitCode = await proc.exited;

      if (exitCode !== 0) {
        throw new Error(`yt-dlp exited with ${exitCode}`);
      }

      return stdout.trim().split("\n")[0];
    } catch (error) {
      console.error(`-> ❌ Failed to fetch HLS stream for: ${url}\n${error}`);
    }
  },
  { cache, getKey: (id) => `v:${id}` },
);

// ─────────────────────────────────────────────
// HLS stream dispatcher
// ─────────────────────────────────────────────

async function resolveHlsStream(param: Param): Promise<string | undefined> {
  try {
    return param.type === "x"
      ? await validateStream(param.value)
      : getStream(
          param.type === "v"
            ? `https://www.youtube.com/watch?v=${param.value}`
            : `https://www.youtube.com/${param.value}/live`,
        );
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
