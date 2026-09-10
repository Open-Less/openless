// Vendored from github.com/bigsongeth/dsh-events v0.1.0.
// MIT License, Copyright (c) 2026 bigsong.
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND.

import { appendFileSync } from 'node:fs'

export const SCHEMA_VERSION = 1
export const name = 'dsh-events'

function makeWriter() {
  const target = process.env.DSH_EVENTS_OUT
  if (!target || target === 'stderr') return line => process.stderr.write(line + '\n')
  if (target === 'stdout') return line => process.stdout.write(line + '\n')
  return line => appendFileSync(target, line + '\n')
}

export function apply(ctx) {
  const write = makeWriter()
  let emitted = 0
  let started = false
  const guard = {}
  let guardSeq = null
  let guardFlushed = false
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
    } catch {}
  }
  const flushGuard = () => {
    if (guardFlushed) return
    guardFlushed = true
    if (Object.keys(guard).length > 0) emit('guard', { seq: guardSeq }, guard)
  }

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
        case 'turn/start':
          flushGuard()
          emit('turn.start', event, { turn: d?.turn ?? null })
          return
        case 'turn/end': {
          const reason = d?.reason
          const fields = { turn: d?.turn ?? null, ok: reason?.kind === 'completed' }
          if (reason?.error) {
            fields.error = {
              code: reason.error.code ?? null,
              message: reason.error.message ?? null,
            }
          }
          emit('turn.end', event, fields)
          return
        }
        case 'assistant/chunk': {
          const chunk = d?.chunk
          if (chunk?.type === 'text-delta' && chunk.text) {
            emit('text.delta', event, {
              turn: d?.turn ?? null,
              step: d?.step ?? null,
              index: chunk.index ?? 0,
              text: chunk.text,
            })
          }
          return
        }
        case 'tool/call':
          emit('tool.call', event, {
            turn: d?.turn ?? null,
            step: d?.step ?? null,
            callId: d?.callId ?? null,
            name: d?.name ?? null,
            arguments: d?.arguments ?? null,
          })
          return
      }
    } catch {}
  })
}
