#!/usr/bin/env -S deno run -A
// A simple release manager for Deno projects

import { parseArgs } from "@std/cli";
import "@std/dotenv/load";

const VERSION_FILE = "deno.json";
const APP_NAME = `${(await getName()).split("/")[1]}`;
const DOCKER_IMAGE = await getName();

// --- helpers ---
async function getName(): Promise<string> {
  const raw = await Deno.readTextFile(VERSION_FILE);
  const json = JSON.parse(raw);
  return json.name.replace(/^@/, "");
}

async function getVersion(): Promise<string> {
  const raw = await Deno.readTextFile(VERSION_FILE);
  const json = JSON.parse(raw);
  return json.version as string;
}

async function setVersion(newVersion: string) {
  const raw = await Deno.readTextFile(VERSION_FILE);
  const json = JSON.parse(raw);
  json.version = newVersion;
  await Deno.writeTextFile(VERSION_FILE, JSON.stringify(json, null, 2) + "\n");
}

async function run(cmd: string[], inherit = true) {
  const p = new Deno.Command(cmd[0], {
    args: cmd.slice(1),
    stdout: inherit ? "inherit" : "piped",
    stderr: inherit ? "inherit" : "piped",
  }).spawn();
  const status = await p.status;
  if (!status.success) {
    throw new Error(`❌ Command failed: ${cmd.join(" ")}`);
  }
}

// --- tasks ---
async function bump(
  versionArg?: string,
  bumpType: "major" | "minor" | "patch" = "patch",
) {
  console.log("📈 Bumping version...");
  let newVer: string;

  if (versionArg) {
    newVer = versionArg;
  } else {
    const oldVer = await getVersion();
    const [maj, min, patch] = oldVer.split(".").map(Number);

    switch (bumpType) {
      case "major":
        newVer = `${maj + 1}.0.0`;
        break;
      case "minor":
        newVer = `${maj}.${min + 1}.0`;
        break;
      case "patch":
      default:
        newVer = `${maj}.${min}.${patch + 1}`;
    }
  }

  await setVersion(newVer);

  await run(["git", "add", VERSION_FILE]);
  await run(["git", "commit", "-m", `release: bump version to ${newVer}`]);
  await run(["git", "tag", "-a", `v${newVer}`, "-m", `Release ${newVer}`]);
  await run(["git", "push"]);
  // await run(["git", "push", "--tags"]);
  await run(["git", "push", "origin", `v${newVer}`]);

  console.log(`✅ Version bumped to ${newVer}`);
  return newVer;
}

async function build() {
  console.log("⚒️  Building executables...");
  await Deno.remove("dist", { recursive: true });
  const targets = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
  ];
  await Deno.mkdir("dist", { recursive: true });
  for (const target of targets) {
    await run([
      "deno",
      "compile",
      "-A",
      "--target",
      target,
      "--output",
      `dist/${APP_NAME}-${target}`,
      "main.ts",
    ]);
  }
  console.log("✅ Built executables");
}

async function dockerhub() {
  console.log("🐳 Publishing multiplatform Docker image...");
  const version = await getVersion();
  await run([
    "docker",
    "buildx",
    "build",
    "--platform",
    "linux/amd64,linux/arm64",
    "-t",
    `${DOCKER_IMAGE}:${version}`,
    "-t",
    `${DOCKER_IMAGE}:latest`,
    "--push",
    ".",
  ]);
  console.log("✅ Docker Hub publish done (multi-arch)");
}

async function ghAssets() {
  console.log("📦 Uploading executables to GitHub Releases...");
  const version = await getVersion();
  const tag = `v${version}`;
  const githubToken = Deno.env.get("GITHUB_TOKEN");
  if (!githubToken) throw new Error("❌ GITHUB_TOKEN env var is required");

  // Get last release tag (excluding the current one we're about to create)
  let lastTag = "";
  try {
    const p = new Deno.Command("git", {
      args: ["describe", "--tags", "--abbrev=0", "--exclude", `v${version}*`],
      stdout: "piped",
      stderr: "null",
    }).spawn();
    const output = await p.output();
    lastTag = new TextDecoder().decode(output.stdout).trim();
  } catch {
    console.log("ℹ️ No previous tag found");
  }

  // Build changelog - use the correct range
  let changelog = "No changes since last release.";
  let compareUrl = `https://github.com/${DOCKER_IMAGE}/commits/${tag}`;

  if (lastTag) {
    const range = `${lastTag}..HEAD`;
    const logCmd = new Deno.Command("git", {
      args: ["log", "--oneline", "--format=%h %s", range],
      stdout: "piped",
    }).spawn();
    const logOutput = await logCmd.output();
    const commits = new TextDecoder().decode(logOutput.stdout).trim();

    if (commits) {
      changelog = commits.split("\n").map((line) => `- ${line}`).join("\n");
    }
    compareUrl =
      `https://github.com/${DOCKER_IMAGE}/compare/${lastTag}...${tag}`;
  }

  const releaseBody =
    `## Changes\n\n${changelog}\n\n🔗 [Compare changes](${compareUrl})`;

  // Check if release exists
  let release: any;
  const releaseUrl =
    `https://api.github.com/repos/${DOCKER_IMAGE}/releases/tags/${tag}`;
  const releaseResp = await fetch(releaseUrl, {
    headers: {
      Authorization: `token ${githubToken}`,
      Accept: "application/vnd.github+json",
    },
  });

  if (releaseResp.ok) {
    release = await releaseResp.json();
    console.log(`ℹ️ Release ${tag} already exists`);
  } else {
    const createResp = await fetch(
      `https://api.github.com/repos/${DOCKER_IMAGE}/releases`,
      {
        method: "POST",
        headers: {
          Authorization: `token ${githubToken}`,
          Accept: "application/vnd.github+json",
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          tag_name: tag,
          name: `LiveTube ${tag}`,
          body: releaseBody,
          draft: false,
          prerelease: false,
        }),
      },
    );

    if (!createResp.ok) {
      throw new Error(
        `❌ Failed to create release: ${createResp.status} ${createResp.statusText}`,
      );
    }

    release = await createResp.json();
    console.log(`✅ Release ${tag} created`);
  }

  const uploadUrl = release.upload_url.replace("{?name,label}", "");

  for await (const f of Deno.readDir("dist")) {
    if (f.isFile) {
      const filePath = `dist/${f.name}`;
      const fileData = await Deno.readFile(filePath);

      const uploadResp = await fetch(
        `${uploadUrl}?name=${encodeURIComponent(f.name)}`,
        {
          method: "POST",
          headers: {
            Authorization: `token ${githubToken}`,
            Accept: "application/vnd.github+json",
            "Content-Type": "application/octet-stream",
          },
          body: fileData,
        },
      );

      if (!uploadResp.ok) {
        throw new Error(
          `❌ Failed to upload ${f.name}: ${uploadResp.status} ${uploadResp.statusText}`,
        );
      }
      console.log(`✅ Uploaded ${f.name}`);
    }
  }
  console.log("🎉 All assets uploaded to GitHub release");
}

// --- main ---
const parsed = parseArgs(Deno.args, {
  string: ["version", "bump"],
  default: { bump: "patch" },
});

const args = parsed._.length ? parsed._ : ["all"];

for (const arg of args) {
  switch (arg) {
    case "bump":
      await bump(parsed.version, parsed.bump as "major" | "minor" | "patch");
      break;
    case "build":
      await build();
      break;
    case "dockerhub":
      await dockerhub();
      break;
    case "gh-assets":
      await ghAssets();
      break;
    case "all":
      await bump(parsed.version, parsed.bump as "major" | "minor" | "patch");
      await build();
      await dockerhub();
      await ghAssets();
      break;
    default:
      console.error(`❌ Unknown command: ${arg}`);
      Deno.exit(1);
  }
}
