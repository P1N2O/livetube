import { $ } from "bun";

const version = process.argv[2];

if (!version) {
  console.error("Usage: bun run release vX.Y.Z");
  process.exit(1);
}

const tag = version.startsWith("v") ? version : `v${version}`;

console.log(`Creating release ${tag}`);

await $`git tag -a ${tag} -m ${`Release ${tag}`}`;
await $`git push origin ${tag}`;

console.log(`✔ Release pushed: ${tag}`);
