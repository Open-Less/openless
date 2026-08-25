import { createHash } from "node:crypto"
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
} from "node:fs"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..")

function parseArgs(argv) {
  const options = {
    buildRoot: join(appRoot, "src-tauri", "target", "release", "build"),
    output: join(appRoot, "src-tauri", "target", "release", "openless-mlx", "mlx.metallib"),
  }
  for (let index = 0; index < argv.length; index += 1) {
    const value = argv[index + 1]
    if (argv[index] === "--build-root" && value) {
      options.buildRoot = resolve(value)
      index += 1
    } else if (argv[index] === "--output" && value) {
      options.output = resolve(value)
      index += 1
    } else {
      throw new Error(`未知参数或缺少参数值：${argv[index]}`)
    }
  }
  return options
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex")
}

function collectCandidates(buildRoot) {
  if (!existsSync(buildRoot)) return []
  return readdirSync(buildRoot, { withFileTypes: true })
    .filter(entry => entry.isDirectory() && entry.name.startsWith("qwen3-asr-rs-"))
    .map(entry => join(buildRoot, entry.name, "out", "lib", "mlx.metallib"))
    .filter(path => existsSync(path) && statSync(path).isFile() && statSync(path).size > 0)
    .sort()
}

export function stageMetallib({ buildRoot, output }) {
  rmSync(output, { force: true })
  const candidates = collectCandidates(buildRoot)
  if (candidates.length === 0) {
    throw new Error(`未找到 MLX metallib：${buildRoot}`)
  }

  const byHash = new Map()
  for (const candidate of candidates) {
    const hash = sha256(candidate)
    const paths = byHash.get(hash) ?? []
    paths.push(candidate)
    byHash.set(hash, paths)
  }
  if (byHash.size > 1) {
    const details = [...byHash.entries()]
      .flatMap(([hash, paths]) => paths.map(path => `  ${hash}  ${path}`))
      .join("\n")
    throw new Error(`发现多个内容冲突的 MLX metallib，拒绝猜测构建产物：\n${details}`)
  }

  mkdirSync(dirname(output), { recursive: true })
  copyFileSync(candidates[0], output)
  const hash = [...byHash.keys()][0]
  console.log(`✓ staged MLX metallib: ${output}`)
  console.log(`  sha256=${hash} candidates=${candidates.length}`)
}

try {
  stageMetallib(parseArgs(process.argv.slice(2)))
} catch (error) {
  console.error(`✗ ${error instanceof Error ? error.message : String(error)}`)
  process.exitCode = 1
}
