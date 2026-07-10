import type { MarketplaceDetail, MarketplaceListItem } from "../types"

const MARKETPLACE_LIST_CACHE_KEY = "ol-marketplace-list-cache-v2"
const MARKETPLACE_DETAIL_CACHE_KEY = "ol-marketplace-detail-cache-v2"
const MARKETPLACE_LIST_TTL_MS = 24 * 60 * 60 * 1000
const MARKETPLACE_DETAIL_TTL_MS = 30 * 24 * 60 * 60 * 1000
const MARKETPLACE_DETAIL_MAX_ENTRIES = 64
const MARKETPLACE_DETAIL_MAX_PROMPT_CHARS = 200_000

const PACK_ID_RE =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i

function isValidMarketplacePackId(id: unknown): id is string {
    return typeof id === "string" && PACK_ID_RE.test(id)
}

function detailCacheKey(
    id: string,
    version: string,
    updatedAt: string,
): string {
    return `${id}::${version ?? ""}::${updatedAt ?? ""}`
}

export function readMarketplaceListCache(): MarketplaceListItem[] | null {
    try {
        const raw = localStorage.getItem(MARKETPLACE_LIST_CACHE_KEY)
        if (!raw) return null
        const parsed = JSON.parse(raw) as {
            items: MarketplaceListItem[]
            ts: number
        }
        if (!parsed || !Array.isArray(parsed.items)) return null
        if (Date.now() - parsed.ts > MARKETPLACE_LIST_TTL_MS) return null
        return parsed.items.filter(
            (it) => it && isValidMarketplacePackId(it.id),
        )
    } catch {
        return null
    }
}

export function writeMarketplaceListCache(items: MarketplaceListItem[]): void {
    try {
        const sanitized = items.filter(
            (it) => it && isValidMarketplacePackId(it.id),
        )
        localStorage.setItem(
            MARKETPLACE_LIST_CACHE_KEY,
            JSON.stringify({ items: sanitized, ts: Date.now() }),
        )
        const keepKeys = new Set(
            sanitized.map((it) =>
                detailCacheKey(it.id, it.version ?? "", it.updatedAt ?? ""),
            ),
        )
        pruneMarketplaceDetailCache(keepKeys)
    } catch {
        // quota exceeded / disabled — silent
    }
}

type MarketplaceDetailCacheEntry = {
    key: string
    detail: MarketplaceDetail
    ts: number
}

function readMarketplaceDetailStore(): Record<
    string,
    MarketplaceDetailCacheEntry
> {
    try {
        const raw = localStorage.getItem(MARKETPLACE_DETAIL_CACHE_KEY)
        if (!raw) return {}
        const parsed = JSON.parse(raw) as Record<
            string,
            MarketplaceDetailCacheEntry
        > | null
        return parsed && typeof parsed === "object" ? parsed : {}
    } catch {
        return {}
    }
}

function writeMarketplaceDetailStore(
    store: Record<string, MarketplaceDetailCacheEntry>,
): void {
    try {
        localStorage.setItem(
            MARKETPLACE_DETAIL_CACHE_KEY,
            JSON.stringify(store),
        )
    } catch {
        // quota exceeded
    }
}

export function readMarketplaceDetailCache(
    packId: string,
    version: string,
    updatedAt: string,
): MarketplaceDetail | null {
    if (!isValidMarketplacePackId(packId)) return null
    const store = readMarketplaceDetailStore()
    const entry = store[detailCacheKey(packId, version, updatedAt)]
    if (!entry) return null
    if (Date.now() - entry.ts > MARKETPLACE_DETAIL_TTL_MS) return null
    if (!entry.detail || entry.detail.id !== packId) return null
    return entry.detail
}

export function writeMarketplaceDetailCache(detail: MarketplaceDetail): void {
    if (!isValidMarketplacePackId(detail.id)) return
    if (
        typeof detail.prompt === "string" &&
        detail.prompt.length > MARKETPLACE_DETAIL_MAX_PROMPT_CHARS
    ) {
        return
    }
    const store = readMarketplaceDetailStore()
    const key = detailCacheKey(
        detail.id,
        detail.version ?? "",
        detail.updatedAt ?? "",
    )
    store[key] = { key, detail, ts: Date.now() }
    const entries = Object.values(store).sort((a, b) => a.ts - b.ts)
    while (entries.length > MARKETPLACE_DETAIL_MAX_ENTRIES) {
        const oldest = entries.shift()
        if (oldest) delete store[oldest.key]
    }
    writeMarketplaceDetailStore(store)
}

function pruneMarketplaceDetailCache(keepKeys: Set<string>): void {
    const store = readMarketplaceDetailStore()
    let changed = false
    for (const key of Object.keys(store)) {
        if (!keepKeys.has(key)) {
            delete store[key]
            changed = true
        }
    }
    if (changed) writeMarketplaceDetailStore(store)
}
