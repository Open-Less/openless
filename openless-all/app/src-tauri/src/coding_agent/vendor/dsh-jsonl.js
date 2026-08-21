// Vendored from github.com/bigsongeth/dsh-jsonl v0.1.0 — do not edit here.
// Run scripts/vendor-dsh-jsonl.sh <path-to-checkout> to update, then bump
// VENDORED_DSH_JSONL_VERSION in dsh.rs to match.
//
// MIT License
//
// Copyright (c) 2026 bigsong
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
//
// dsh-jsonl — structured event output for `dsh --profile headless`.
//
// `dsh --profile headless "<task>"` prints one line: the final assistant message.
// Everything that happened on the way — file reads, commands, streaming text,
// token usage — is discarded. Not because dsh lacks the data: its session event
// bus carries all of it, the headless bundle just summarizes and drops it.
//
// This plugin subscribes to that bus and mirrors a documented, versioned subset
// as NDJSON. stdout is left untouched, so every existing consumer of the final
// line keeps working.
//
// See SCHEMA.md for the event vocabulary and the compatibility contract.

import { appendFileSync } from 'node:fs'

/** Schema major version. Bumped only on breaking changes. See SCHEMA.md. */
export const SCHEMA_VERSION = 1

/** Stable Cordis plugin name. */
export const name = 'dsh-jsonl'

/**
 * Where the NDJSON goes.
 *
 * Default is stderr, not stdout: stdout carries dsh's own final-message line,
 * which is its existing contract with every current consumer. Taking it over
 * would silently break them. Set DSH_JSONL_OUT=<file> to write to a file
 * instead, or DSH_JSONL_OUT=stdout if you genuinely want them interleaved.
 */
function makeWriter() {
  const target = process.env.DSH_JSONL_OUT
  if (!target || target === 'stderr') return line => process.stderr.write(line + '\n')
  if (target === 'stdout') return line => process.stdout.write(line + '\n')
  return line => appendFileSync(target, line + '\n')
}

/** Concatenate the text parts of a tool result's nested content. */
function toolResultText(part) {
  if (!Array.isArray(part?.content)) return ''
  return part.content
    .filter(c => c?.type === 'text' && typeof c.text === 'string')
    .map(c => c.text)
    .join('')
}

export function apply(ctx) {
  const write = makeWriter()
  let emitted = 0

  const emit = (type, event, fields) => {
    try {
      write(JSON.stringify({
        v: SCHEMA_VERSION,
        seq: event?.seq ?? emitted,
        ts: event?.time ?? null,
        type,
        ...fields,
      }))
      emitted += 1
    } catch {
      // Never let logging break the run. A dropped line is always better than
      // a failed task.
    }
  }

  // The three permission events arrive before any turn. They describe one thing
  // — this run's sandbox posture — so they are buffered and flushed as a single
  // `guard` event once the run actually starts.
  const guard = {}
  // Keep the seq of the last permission event so `guard` reports where it
  // actually came from. Reusing the flushing event's seq would produce two
  // lines with the same seq, and seq is meant to be a unique ordering key.
  let guardSeq = null
  let guardFlushed = false
  const flushGuard = () => {
    if (guardFlushed) return
    guardFlushed = true
    if (Object.keys(guard).length > 0) emit('guard', { seq: guardSeq }, guard)
  }

  const raw = process.env.DSH_JSONL_RAW === '1'

  // `session.start` is emitted lazily on the first event, not at mount time:
  // the session identity does not exist yet when the plugin is applied.
  let started = false

  ctx.on('session/event', (session, event) => {
    try {
      if (!started) {
        started = true
        emit('session.start', null, {
          sessionId: session?.id ?? null,
          cwd: process.cwd(),
          schema: SCHEMA_VERSION,
        })
      }
      const d = event?.data
      switch (event?.type) {
        // ── run-level context ────────────────────────────────────────────
        case 'sandbox/mode':
          guard.sandbox = d?.mode ?? null
          guardSeq = event?.seq ?? guardSeq
          return
        case 'approval/policy':
          guard.approval = d?.policy ?? null
          guardSeq = event?.seq ?? guardSeq
          return
        case 'permission/preset':
          guard.preset = d?.preset ?? null
          guardSeq = event?.seq ?? guardSeq
          return
        case 'request/context':
          emit('run.config', event, {
            provider: d?.provider ?? null,
            model: d?.model ?? null,
            contextWindow: d?.contextWindow ?? null,
          })
          return

        // ── turn / step lifecycle ────────────────────────────────────────
        case 'turn/start':
          flushGuard()
          emit('turn.start', event, { turn: d?.turn ?? null })
          return
        case 'turn/end': {
          const reason = d?.reason
          const out = { turn: d?.turn ?? null, ok: reason?.kind === 'completed' }
          if (reason?.error) {
            out.error = {
              code: reason.error.code ?? null,
              message: reason.error.message ?? null,
            }
          }
          emit('turn.end', event, out)
          return
        }
        case 'step/start':
          emit('step.start', event, { turn: d?.turn ?? null, step: d?.step ?? null })
          return
        case 'step/end':
          emit('step.end', event, { turn: d?.turn ?? null, step: d?.step ?? null })
          return

        // ── assistant streaming ──────────────────────────────────────────
        case 'assistant/chunk': {
          const c = d?.chunk
          const at = { turn: d?.turn ?? null, step: d?.step ?? null }
          if (c?.type === 'text-delta' && c.text) {
            emit('text.delta', event, { ...at, index: c.index ?? 0, text: c.text })
          } else if (c?.type === 'reasoning-delta' && c.text) {
            emit('reasoning.delta', event, { ...at, index: c.index ?? 0, text: c.text })
          } else if (c?.type === 'block-end' && c.block?.type === 'text') {
            emit('text.end', event, { ...at, index: c.index ?? 0, text: c.block.text ?? '' })
          } else if (c?.type === 'block-end' && c.block?.type === 'reasoning') {
            emit('reasoning.end', event, { ...at, index: c.index ?? 0, text: c.block.text ?? '' })
          } else if (c?.type === 'usage') {
            emit('usage', event, { ...at, ...(c.usage ?? {}) })
          }
          // block-start / tool-call-delta / finish are deliberately dropped:
          // tool calls are reported in full by `tool/call`, and `finish` carries
          // a large provider-specific `replayState` blob.
          return
        }

        // ── tools ────────────────────────────────────────────────────────
        case 'tool/call':
          emit('tool.call', event, {
            turn: d?.turn ?? null,
            step: d?.step ?? null,
            callId: d?.callId ?? null,
            name: d?.name ?? null,
            arguments: d?.arguments ?? null,
          })
          return
        case 'tool/result': {
          const part = d?.message?.content?.find(c => c?.type === 'tool-result')
          emit('tool.result', event, {
            turn: d?.turn ?? null,
            step: d?.step ?? null,
            callId: part?.toolCallId ?? d?.message?.source?.callId ?? null,
            ok: part?.isError !== true,
            text: toolResultText(part),
            diffs: d?.meta?.diffs ?? [],
          })
          return
        }
      }

      // Anything not in the vocabulary above. Off by default — `request/header`
      // alone is ~20KB and contains the entire system prompt, the tool
      // definitions, and the user's skill list. A "just dump everything" tool
      // would quietly write all of that into logs.
      if (raw) emit('raw', event, { event })
    } catch {
      // Same rule as above: logging must never break the run.
    }
  })
}
