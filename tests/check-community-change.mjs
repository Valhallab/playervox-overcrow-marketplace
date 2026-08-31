#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const MAX_CHANGED_BYTES = 262_144;
const MAX_CHANGED_PATHS = 512;
const MAX_CHANGED_PATH_BYTES = 512;
const MAX_COMMUNITY_ROOTS = 100;
const MAX_PLAN_BYTES = 131_072;
const MAX_PLAN_ENTRIES = 500;
const MAX_SOURCE_BYTES = 192;
const IDENTIFIER = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/;
const SAFE_PATH = /^[A-Za-z0-9._+@/-]+$/;
const SAFE_SOURCE = /^[A-Za-z0-9._/-]+$/;

function reject() {
  console.error("error: community change policy rejected");
  process.exit(1);
}

function readRegularFile(file, maximumBytes) {
  let descriptor;
  try {
    descriptor = fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW);
    const metadata = fs.fstatSync(descriptor);
    if (!metadata.isFile() || metadata.size > maximumBytes) {
      reject();
    }
    const bytes = fs.readFileSync(descriptor);
    if (bytes.length !== metadata.size) {
      reject();
    }
    return bytes;
  } catch {
    reject();
  } finally {
    if (descriptor !== undefined) {
      try {
        fs.closeSync(descriptor);
      } catch {
        reject();
      }
    }
  }
}

function decode(bytes) {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    reject();
  }
}

function safeRelativePath(relative, maximumBytes, allowed) {
  if (
    relative.length === 0 ||
    Buffer.byteLength(relative) > maximumBytes ||
    !allowed.test(relative) ||
    relative.startsWith("/") ||
    relative.endsWith("/") ||
    relative.includes("//") ||
    relative.includes("\\") ||
    relative.includes("\n") ||
    relative.includes("\r")
  ) {
    return false;
  }
  const parts = relative.split("/");
  return parts.every((part) => part !== "" && part !== "." && part !== "..");
}

function changedCommunityRoots(file) {
  const bytes = readRegularFile(file, MAX_CHANGED_BYTES);
  if (bytes.length === 0) {
    return new Set();
  }
  if (bytes[bytes.length - 1] !== 0) {
    reject();
  }
  const records = decode(bytes.subarray(0, bytes.length - 1)).split("\0");
  if (
    records.length === 0 ||
    records.length > MAX_CHANGED_PATHS ||
    records.some((record) => !safeRelativePath(record, MAX_CHANGED_PATH_BYTES, SAFE_PATH))
  ) {
    reject();
  }

  const roots = new Set();
  for (const record of records) {
    if (!record.startsWith("community/") || record === "community/README.md") {
      continue;
    }
    const parts = record.split("/");
    if (
      parts.length < 4 ||
      !IDENTIFIER.test(parts[1]) ||
      !IDENTIFIER.test(parts[2])
    ) {
      reject();
    }
    roots.add(parts.slice(0, 3).join("/"));
    if (roots.size > MAX_COMMUNITY_ROOTS) {
      reject();
    }
  }
  return roots;
}

function plannedSources(file) {
  const bytes = readRegularFile(file, MAX_PLAN_BYTES);
  const text = decode(bytes);
  if (text.length === 0 || !text.endsWith("\n") || text.includes("\0") || text.includes("\r")) {
    reject();
  }
  const lines = text.slice(0, -1).split("\n");
  if (lines.length === 0 || lines.length > MAX_PLAN_ENTRIES) {
    reject();
  }

  const sources = new Set();
  for (const line of lines) {
    const fields = line.split("\t");
    if (fields.length !== 3) {
      reject();
    }
    const [cargoPackage, componentArtifact, source] = fields;
    if (
      !/^[a-z0-9][a-z0-9-]{0,127}$/.test(cargoPackage) ||
      !/^[a-z0-9][a-z0-9_]{0,127}$/.test(componentArtifact) ||
      !safeRelativePath(source, MAX_SOURCE_BYTES, SAFE_SOURCE) ||
      sources.has(source)
    ) {
      reject();
    }
    if (source.startsWith("community/")) {
      const parts = source.split("/");
      if (
        parts.length !== 3 ||
        !IDENTIFIER.test(parts[1]) ||
        !IDENTIFIER.test(parts[2])
      ) {
        reject();
      }
    }
    sources.add(source);
  }
  return sources;
}

if (process.argv.length !== 5) {
  console.error(
    "usage: check-community-change.mjs REPOSITORY BUILD-PLAN CHANGED-PATHS-NUL",
  );
  process.exit(2);
}

const repository = process.argv[2];
if (!path.isAbsolute(repository) || repository === path.parse(repository).root) {
  reject();
}
let repositoryMetadata;
try {
  repositoryMetadata = fs.lstatSync(repository);
  if (
    !repositoryMetadata.isDirectory() ||
    repositoryMetadata.isSymbolicLink() ||
    fs.realpathSync(repository) !== repository
  ) {
    reject();
  }
} catch {
  reject();
}

const roots = changedCommunityRoots(process.argv[4]);
const sources = plannedSources(process.argv[3]);
for (const root of roots) {
  const submission = path.join(repository, ...root.split("/"));
  let metadata;
  let resolved;
  try {
    metadata = fs.lstatSync(submission);
    resolved = fs.realpathSync(submission);
  } catch (error) {
    if (error?.code === "ENOENT" && !sources.has(root)) {
      continue;
    }
    reject();
  }
  if (
    !metadata.isDirectory() ||
    metadata.isSymbolicLink() ||
    resolved !== submission ||
    !sources.has(root)
  ) {
    reject();
  }
}
