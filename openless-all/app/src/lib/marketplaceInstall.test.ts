import {
    canStartMarketplaceInstall,
    isMarketplaceInstallActive,
    isMarketplaceInstallErrorForPack,
    shouldCloseMarketplaceDetail,
    type MarketplaceInstallError,
} from "./marketplaceInstall"

function assert(condition: boolean, message: string) {
    if (!condition) throw new Error(message)
}

assert(canStartMarketplaceInstall(null), "an idle marketplace can start an install")
assert(!canStartMarketplaceInstall("pack-a"), "a second marketplace install is blocked")
assert(isMarketplaceInstallActive("pack-a", "pack-a"), "active state belongs to its pack")
assert(!isMarketplaceInstallActive("pack-a", "pack-b"), "active state does not leak to another pack")
assert(shouldCloseMarketplaceDetail("pack-a", "pack-a"), "completed pack closes its own detail")
assert(!shouldCloseMarketplaceDetail("pack-b", "pack-a"), "completed pack does not close another detail")

const error: MarketplaceInstallError = { packId: "pack-a", message: "install failed" }
assert(isMarketplaceInstallErrorForPack(error, "pack-a"), "error is shown for its pack")
assert(!isMarketplaceInstallErrorForPack(error, "pack-b"), "error is hidden from another pack")

console.log("marketplaceInstall.test.ts passed")
