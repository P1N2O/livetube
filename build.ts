const p = "dist/livetube-";

const targets = [
  { target: "bun-linux-x64", outfile: `${p}linux-x64` },
  { target: "bun-linux-arm64", outfile: `${p}linux-arm64` },
  { target: "bun-darwin-x64", outfile: `${p}macos-x64` },
  { target: "bun-darwin-arm64", outfile: `${p}macos-arm64` },
  { target: "bun-windows-x64", outfile: `${p}windows-x64` },
] as const;

for (const { target, outfile } of targets) {
  console.log(`Building ${target}...`);
  const result = await Bun.build({
    entrypoints: ["./index.ts"],
    compile: { target, outfile },
    minify: true,
  });
  if (!result.success) {
    console.error(`Failed: ${target}`, result.logs);
    process.exit(1);
  }
}
