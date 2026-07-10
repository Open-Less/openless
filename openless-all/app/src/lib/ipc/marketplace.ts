import type { MarketplaceDetail, MarketplaceListItem, MarketplaceMyPackItem, StylePack } from "../types"
import { invokeOrMock } from "./shared"
import { mockStylePacks } from "./mock-data"

const MOCK_MARKETPLACE: MarketplaceListItem[] = [
    {
        id: "00000000-0000-0000-0000-000000000001",
        slug: "demo-pack",
        name: "示范风格包",
        description: "Mock 数据 - vite dev 模式下显示",
        authorLogin: "demo",
        version: "1.0.0",
        baseMode: "structured",
        tags: ["demo"],
        likeCount: 12,
        downloadCount: 50,
        publishedAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
    },
]

export function listMarketplace(
    options: { query?: string; sort?: "new" | "popular"; limit?: number } = {},
): Promise<MarketplaceListItem[]> {
    return invokeOrMock("marketplace_list", options, () => MOCK_MARKETPLACE)
}

export function fetchMarketplaceDetail(
    packId: string,
): Promise<MarketplaceDetail> {
    return invokeOrMock("marketplace_detail", { packId }, () => ({
        ...MOCK_MARKETPLACE[0],
        prompt: "# 角色\n你是测试用 polish 助手。\n\n# 任务\n按整体意图整理转写。",
        state: "approved" as const,
    }))
}

export function installMarketplacePack(packId: string): Promise<StylePack> {
    return invokeOrMock(
        "marketplace_install",
        { packId },
        () => mockStylePacks[0],
    )
}

export function uploadMarketplacePack(
    packId: string,
    originPackId?: string | null,
): Promise<{ id: string; state: string; message: string }> {
    return invokeOrMock(
        "marketplace_upload",
        { packId, originPackId: originPackId ?? null },
        () => ({
            id: "mock-uploaded",
            state: "pending",
            message: "Mock 上传成功（vite dev）",
        }),
    )
}

export function likeMarketplacePack(
    packId: string,
): Promise<{ likeCount: number; alreadyLiked: boolean }> {
    return invokeOrMock("marketplace_like", { packId }, () => ({
        likeCount: 13,
        alreadyLiked: false,
    }))
}

export function marketplaceMyLikes(): Promise<string[]> {
    return invokeOrMock<string[]>("marketplace_my_likes", undefined, () => [])
}

export function marketplaceMyPacks(): Promise<MarketplaceMyPackItem[]> {
    return invokeOrMock<MarketplaceMyPackItem[]>(
        "marketplace_my_packs",
        undefined,
        () => [],
    )
}

export function marketplaceDelete(packId: string): Promise<void> {
    return invokeOrMock<void>("marketplace_delete", { packId }, () => undefined)
}
