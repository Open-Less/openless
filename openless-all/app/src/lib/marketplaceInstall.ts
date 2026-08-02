export type MarketplaceInstallError = {
    packId: string
    message: string
}

export function canStartMarketplaceInstall(installingPackId: string | null): boolean {
    return installingPackId === null
}

export function isMarketplaceInstallActive(
    installingPackId: string | null,
    packId: string,
): boolean {
    return installingPackId === packId
}

export function isMarketplaceInstallErrorForPack(
    error: MarketplaceInstallError | null,
    packId: string,
): error is MarketplaceInstallError {
    return error?.packId === packId
}

export function shouldCloseMarketplaceDetail(
    selectedId: string | null,
    completedPackId: string,
): boolean {
    return selectedId === completedPackId
}
