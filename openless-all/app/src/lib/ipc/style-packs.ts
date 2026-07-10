import type { PolishMode, StylePack, StylePackRuntimeDiagnostics } from "../types"
import { invokeOrMock } from "./shared"
import {
    cloneMockStylePacks,
    cloneStylePack,
    composeMockStylePackRuntimeDiagnostics,
    mockSetDefaultPolishMode,
    mockSetStyleEnabled,
    mockSaveStylePack,
    mockCreateStylePackFromTemplate,
    mockSetActiveStylePack,
    mockSetStylePackEnabled,
    mockResetBuiltinStylePack,
    mockDeleteStylePack,
    mockImportStylePackFromZip,
} from "./mock-data"

export function setDefaultPolishMode(mode: PolishMode): Promise<void> {
    return invokeOrMock("set_default_polish_mode", { mode }, () => {
        mockSetDefaultPolishMode(mode)
        return undefined
    })
}

export function setStyleEnabled(
    mode: PolishMode,
    enabled: boolean,
): Promise<void> {
    return invokeOrMock("set_style_enabled", { mode, enabled }, () => {
        mockSetStyleEnabled(mode, enabled)
        return undefined
    })
}

export function listStylePacks(): Promise<StylePack[]> {
    return invokeOrMock("list_style_packs", undefined, () =>
        cloneMockStylePacks(),
    )
}

export function saveStylePack(stylePack: StylePack): Promise<StylePack> {
    return invokeOrMock("save_style_pack", { stylePack }, () =>
        mockSaveStylePack(stylePack),
    )
}

export function createStylePackFromTemplate(
    template: StylePack,
): Promise<StylePack> {
    return invokeOrMock("create_style_pack_from_template", { template }, () =>
        mockCreateStylePackFromTemplate(template),
    )
}

export function previewStylePackRuntime(
    stylePack: StylePack,
): Promise<StylePackRuntimeDiagnostics> {
    return invokeOrMock("preview_style_pack_runtime", { stylePack }, () =>
        composeMockStylePackRuntimeDiagnostics(stylePack),
    )
}

export function setActiveStylePack(id: string): Promise<StylePack> {
    return invokeOrMock("set_active_style_pack", { id }, () =>
        mockSetActiveStylePack(id),
    )
}

export function setStylePackEnabled(
    id: string,
    enabled: boolean,
): Promise<StylePack[]> {
    return invokeOrMock("set_style_pack_enabled", { id, enabled }, () =>
        mockSetStylePackEnabled(id, enabled),
    )
}

export function resetBuiltinStylePack(id: string): Promise<StylePack> {
    return invokeOrMock("reset_builtin_style_pack", { id }, () =>
        mockResetBuiltinStylePack(id),
    )
}

export function deleteStylePack(id: string): Promise<void> {
    return invokeOrMock("delete_style_pack", { id }, () => {
        mockDeleteStylePack(id)
        return undefined
    })
}

export function importStylePackFromZip(zipPath: string): Promise<StylePack> {
    return invokeOrMock("import_style_pack_from_zip", { zipPath }, () =>
        mockImportStylePackFromZip(zipPath),
    )
}

export function exportStylePackToZip(
    id: string,
    targetPath: string,
): Promise<string> {
    return invokeOrMock(
        "export_style_pack_to_zip",
        { id, targetPath },
        () => targetPath,
    )
}

export function repolish(rawText: string, mode: PolishMode): Promise<string> {
    return invokeOrMock("repolish", { rawText, mode }, () => rawText)
}
