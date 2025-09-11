import { mkdir, rm } from "node:fs/promises";

export async function ensureDir(dir: string) {
  await mkdir(dir, { recursive: true });
}

export async function emptyDir(dir: string) {
  await rm(dir, { recursive: true, force: true });
}
