import {
    githubFlowExpiresAt,
    githubPollIntervalMs,
    githubSlowDownIntervalMs,
} from "./ipc/github-oauth"

function assertEqual(actual: number, expected: number, name: string) {
    if (actual !== expected) {
        throw new Error(`${name}: expected ${expected}, got ${actual}`)
    }
}

assertEqual(githubPollIntervalMs(7), 7_000, "server interval")
assertEqual(githubPollIntervalMs(0), 5_000, "invalid zero interval fallback")
assertEqual(githubPollIntervalMs(Number.NaN), 5_000, "invalid NaN interval fallback")

let interval = githubPollIntervalMs(7)
interval = githubSlowDownIntervalMs(interval)
assertEqual(interval, 12_000, "first slow_down")
interval = githubSlowDownIntervalMs(interval)
assertEqual(interval, 17_000, "repeated slow_down")

assertEqual(githubFlowExpiresAt(1_000, 15), 16_000, "expiry deadline")
assertEqual(githubFlowExpiresAt(1_000, -1), 1_000, "negative expiry clamp")

console.log("githubOauth.test.ts passed")
