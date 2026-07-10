// localAsr.ts — IPC + 事件类型 for 本地 ASR 引擎与模型管理。
//
// 后端命令定义：openless-all/app/src-tauri/src/commands.rs `local_asr_*`
// 事件：local-asr-download-progress / local-asr-token
//
// 注意：模型文件清单与尺寸不在此处硬编码 —— 通过
// `fetchLocalAsrRemoteInfo()` 实时从 HuggingFace tree API 拉取。

import { invokeOrMock } from "./ipc"

export type LocalAsrMirror = "huggingface" | "hf-mirror"

export interface LocalAsrSettings {
    providerId: string
    activeModel: string
    mirror: string
    modelsBaseDir: string | null
    modelsRootDir: string
    /** macOS 才编入 vendored Open-Less/qwen-asr 引擎；Win 端 UI 据此把"开始"按钮灰掉。 */
    engineAvailable: boolean
}

export interface LocalAsrStorageSettings {
    modelsBaseDir: string | null
    modelsRootDir: string
    isDefault: boolean
}

export interface LocalAsrModelStatus {
    id: string
    hfRepo: string
    downloadedBytes: number
    isDownloaded: boolean
}

export interface LocalAsrRemoteFile {
    path: string
    size: number
}

export interface LocalAsrRemoteInfo {
    modelId: string
    mirror: string
    files: LocalAsrRemoteFile[]
    totalBytes: number
}

export type LocalAsrDownloadPhase =
    | "started"
    | "progress"
    | "finished"
    | "cancelled"
    | "failed"

export interface LocalAsrDownloadProgress {
    modelId: string
    file: string
    fileIndex: number
    fileCount: number
    bytesDownloaded: number
    bytesTotal: number
    phase: LocalAsrDownloadPhase
    error: string | null
}

export interface FoundryLocalAsrStatus {
    providerId: string
    available: boolean
    runtimeReady: boolean
    runtimeSource: FoundryRuntimeSource
    activeModel: string
    loadedModelId: string | null
    endpoint: string | null
    error: string | null
}

export const FOUNDRY_LOCAL_ASR_MODEL_ALIASES = [
    "whisper-small",
    "whisper-medium",
    "whisper-large-v3-turbo",
    "whisper-base",
    "whisper-tiny",
] as const

export type FoundryLocalAsrModelAlias =
    (typeof FOUNDRY_LOCAL_ASR_MODEL_ALIASES)[number]
export type FoundryLocalAsrLanguageHint = "" | "zh" | "en"
export type FoundryRuntimeSource = "auto" | "nuget" | "ort-nightly"

export interface FoundryLocalAsrCatalogModel {
    alias: FoundryLocalAsrModelAlias
    displayName: string
    cached: boolean
    fileSizeMb: number | null
}

export type FoundryPreparePhase =
    | "runtime"
    | "model"
    | "load"
    | "finished"
    | "failed"

export interface FoundryPrepareProgress {
    phase: FoundryPreparePhase
    modelAlias: string
    label: string
    percent: number | null
    error: string | null
}

export interface FoundryLocalAsrModelOption {
    alias: FoundryLocalAsrModelAlias
    labelKey: `localAsr.foundryModel${"Small" | "Medium" | "Large" | "Base" | "Tiny"}`
    descKey: `localAsr.foundryModel${"Small" | "Medium" | "Large" | "Base" | "Tiny"}Desc`
}

export const FOUNDRY_LOCAL_ASR_MODELS: FoundryLocalAsrModelOption[] = [
    {
        alias: "whisper-small",
        labelKey: "localAsr.foundryModelSmall",
        descKey: "localAsr.foundryModelSmallDesc",
    },
    {
        alias: "whisper-medium",
        labelKey: "localAsr.foundryModelMedium",
        descKey: "localAsr.foundryModelMediumDesc",
    },
    {
        alias: "whisper-large-v3-turbo",
        labelKey: "localAsr.foundryModelLarge",
        descKey: "localAsr.foundryModelLargeDesc",
    },
    {
        alias: "whisper-base",
        labelKey: "localAsr.foundryModelBase",
        descKey: "localAsr.foundryModelBaseDesc",
    },
    {
        alias: "whisper-tiny",
        labelKey: "localAsr.foundryModelTiny",
        descKey: "localAsr.foundryModelTinyDesc",
    },
]

const MOCK_FOUNDRY_CATALOG: FoundryLocalAsrCatalogModel[] = [
    {
        alias: "whisper-small",
        displayName: "Whisper Small",
        cached: false,
        fileSizeMb: 967,
    },
    {
        alias: "whisper-medium",
        displayName: "Whisper Medium",
        cached: false,
        fileSizeMb: 937,
    },
    {
        alias: "whisper-large-v3-turbo",
        displayName: "Whisper Large V3 Turbo",
        cached: false,
        fileSizeMb: 1285,
    },
    {
        alias: "whisper-base",
        displayName: "Whisper Base",
        cached: true,
        fileSizeMb: 291,
    },
    {
        alias: "whisper-tiny",
        displayName: "Whisper Tiny",
        cached: false,
        fileSizeMb: 151,
    },
]

const MOCK_SETTINGS: LocalAsrSettings = {
    providerId: "local-qwen3",
    activeModel: "qwen3-asr-0.6b",
    mirror: "huggingface",
    modelsBaseDir: null,
    modelsRootDir: "~/Library/Application Support/OpenLess/models",
    engineAvailable: false,
}

const MOCK_MODELS: LocalAsrModelStatus[] = [
    {
        id: "qwen3-asr-0.6b",
        hfRepo: "Qwen/Qwen3-ASR-0.6B",
        downloadedBytes: 0,
        isDownloaded: false,
    },
    {
        id: "qwen3-asr-1.7b",
        hfRepo: "Qwen/Qwen3-ASR-1.7B",
        downloadedBytes: 0,
        isDownloaded: false,
    },
]

export function getLocalAsrSettings(): Promise<LocalAsrSettings> {
    return invokeOrMock(
        "local_asr_get_settings",
        undefined,
        () => MOCK_SETTINGS,
    )
}

export function getLocalAsrStorageSettings(): Promise<LocalAsrStorageSettings> {
    return invokeOrMock("local_asr_storage_settings", undefined, () => ({
        modelsBaseDir: null,
        modelsRootDir: MOCK_SETTINGS.modelsRootDir,
        isDefault: true,
    }))
}

export function setLocalAsrModelsBaseDir(
    modelsBaseDir: string | null,
): Promise<LocalAsrStorageSettings> {
    return invokeOrMock(
        "local_asr_set_models_base_dir",
        { modelsBaseDir },
        () => ({
            modelsBaseDir,
            modelsRootDir: modelsBaseDir
                ? `${modelsBaseDir}/OpenLess/models`
                : MOCK_SETTINGS.modelsRootDir,
            isDefault: !modelsBaseDir,
        }),
    )
}

export function setLocalAsrActiveModel(modelId: string): Promise<void> {
    return invokeOrMock(
        "local_asr_set_active_model",
        { modelId },
        () => undefined,
    )
}

export function setLocalAsrMirror(mirror: string): Promise<void> {
    return invokeOrMock("local_asr_set_mirror", { mirror }, () => undefined)
}

export function listLocalAsrModels(): Promise<LocalAsrModelStatus[]> {
    return invokeOrMock("local_asr_list_models", undefined, () => MOCK_MODELS)
}

export function fetchLocalAsrRemoteInfo(
    modelId: string,
    mirror?: string,
): Promise<LocalAsrRemoteInfo> {
    return invokeOrMock(
        "local_asr_fetch_remote_info",
        { modelId, mirror },
        () => ({
            modelId,
            mirror: mirror ?? "huggingface",
            files: [],
            totalBytes: 0,
        }),
    )
}

export function downloadLocalAsrModel(
    modelId: string,
    mirror?: string,
): Promise<void> {
    return invokeOrMock(
        "local_asr_download_model",
        { modelId, mirror },
        () => undefined,
    )
}

export function cancelLocalAsrDownload(modelId: string): Promise<void> {
    return invokeOrMock(
        "local_asr_cancel_download",
        { modelId },
        () => undefined,
    )
}

export function deleteLocalAsrModel(modelId: string): Promise<void> {
    return invokeOrMock("local_asr_delete_model", { modelId }, () => undefined)
}

export function getLocalAsrModelDir(modelId: string): Promise<string> {
    return invokeOrMock("local_asr_model_dir", { modelId }, () => "")
}

export function revealLocalAsrModelDir(modelId: string): Promise<void> {
    return invokeOrMock(
        "local_asr_reveal_model_dir",
        { modelId },
        () => undefined,
    )
}

export function revealLocalAsrModelsRoot(): Promise<void> {
    return invokeOrMock(
        "local_asr_reveal_models_root",
        undefined,
        () => undefined,
    )
}

export interface LocalAsrTestResult {
    backend: string
    modelId: string
    expectedText: string
    transcribedText: string
    audioMs: number
    loadMs: number
    transcribeMs: number
}

export function testLocalAsrModel(
    modelId: string,
): Promise<LocalAsrTestResult> {
    return invokeOrMock("local_asr_test_model", { modelId }, () => ({
        backend: "mock",
        modelId,
        expectedText:
            "Hello. This is a test of the Voxtrail speech-to-text system.",
        transcribedText: "(浏览器 dev mock，实际推理需要在 Tauri 应用内)",
        audioMs: 3000,
        loadMs: 0,
        transcribeMs: 0,
    }))
}

export interface LocalAsrEngineStatus {
    loaded: boolean
    modelId: string | null
    keepLoadedSecs: number
}

export function getLocalAsrEngineStatus(): Promise<LocalAsrEngineStatus> {
    return invokeOrMock("local_asr_engine_status", undefined, () => ({
        loaded: false,
        modelId: null,
        keepLoadedSecs: 300,
    }))
}

export function releaseLocalAsrEngine(): Promise<void> {
    return invokeOrMock("local_asr_release_engine", undefined, () => undefined)
}

export function preloadLocalAsr(): Promise<void> {
    return invokeOrMock("local_asr_preload", undefined, () => undefined)
}

export function setLocalAsrKeepLoadedSecs(seconds: number): Promise<void> {
    return invokeOrMock(
        "local_asr_set_keep_loaded_secs",
        { seconds },
        () => undefined,
    )
}

export function getFoundryLocalAsrStatus(): Promise<FoundryLocalAsrStatus> {
    return invokeOrMock("foundry_local_asr_status", undefined, () => ({
        providerId: "foundry-local-whisper",
        available: true,
        runtimeReady: false,
        runtimeSource: "auto",
        activeModel: "whisper-small",
        loadedModelId: null,
        endpoint: null,
        error: null,
    }))
}

export function getFoundryLocalAsrCatalog(): Promise<
    FoundryLocalAsrCatalogModel[]
> {
    return invokeOrMock(
        "foundry_local_asr_catalog",
        undefined,
        () => MOCK_FOUNDRY_CATALOG,
    )
}

export function setFoundryLocalAsrModel(modelAlias: string): Promise<void> {
    return invokeOrMock(
        "foundry_local_asr_set_model",
        { modelAlias },
        () => undefined,
    )
}

export function setFoundryLocalAsrLanguageHint(
    languageHint: string,
): Promise<void> {
    return invokeOrMock(
        "foundry_local_asr_set_language_hint",
        { languageHint },
        () => undefined,
    )
}

export function setFoundryLocalRuntimeSource(source: string): Promise<void> {
    return invokeOrMock(
        "foundry_local_asr_set_runtime_source",
        { source },
        () => undefined,
    )
}

export function prepareFoundryLocalAsr(modelAlias: string): Promise<string> {
    return invokeOrMock(
        "foundry_local_asr_prepare",
        { modelAlias },
        () => `mock-${modelAlias}`,
    )
}

export function cancelFoundryLocalAsrPrepare(): Promise<void> {
    return invokeOrMock(
        "foundry_local_asr_cancel_prepare",
        undefined,
        () => undefined,
    )
}

export function releaseFoundryLocalAsr(): Promise<void> {
    return invokeOrMock("foundry_local_asr_release", undefined, () => undefined)
}

export function getFoundryLocalAsrModelDir(modelAlias: string): Promise<string> {
    return invokeOrMock(
        "foundry_local_asr_model_dir",
        { modelAlias },
        () => "",
    )
}

export function deleteFoundryLocalAsrModel(modelAlias: string): Promise<void> {
    return invokeOrMock(
        "foundry_local_asr_delete_model",
        { modelAlias },
        () => undefined,
    )
}

export function revealFoundryLocalAsrModelDir(
    modelAlias: string,
): Promise<void> {
    return invokeOrMock(
        "foundry_local_asr_reveal_model_dir",
        { modelAlias },
        () => undefined,
    )
}

// ─── Sherpa-Onnx Local ASR ───────────────────────────────────────────

export type SherpaOnnxModelAlias =
    | "sense-voice-small-zh"
    | "paraformer-zh"
    | "whisper-small-multi"
    | "qwen3-asr-0.6b-int8"

export type SherpaOnnxMirror = "huggingface" | "hf-mirror" | "github-release"

export interface SherpaOnnxAsrStatus {
    providerId: string
    available: boolean
    runtimeReady: boolean
    activeModel: string
    loadedModelId: string | null
    error: string | null
}

export interface SherpaOnnxCatalogModel {
    alias: SherpaOnnxModelAlias
    displayName: string
    cached: boolean
    downloadedBytes: number
    fileSizeMb: number | null
}

export interface SherpaOnnxModelOption {
    alias: SherpaOnnxModelAlias
    labelKey: string
    descKey: string
}

export const SHERPA_ONNX_ASR_MODELS: SherpaOnnxModelOption[] = [
    {
        alias: "sense-voice-small-zh",
        labelKey: "localAsr.sherpaModelSenseVoice",
        descKey: "localAsr.sherpaModelSenseVoiceDesc",
    },
    {
        alias: "paraformer-zh",
        labelKey: "localAsr.sherpaModelParaformer",
        descKey: "localAsr.sherpaModelParaformerDesc",
    },
    {
        alias: "whisper-small-multi",
        labelKey: "localAsr.sherpaModelWhisper",
        descKey: "localAsr.sherpaModelWhisperDesc",
    },
    {
        alias: "qwen3-asr-0.6b-int8",
        labelKey: "localAsr.sherpaModelQwen3",
        descKey: "localAsr.sherpaModelQwen3Desc",
    },
]

export function getSherpaOnnxAsrStatus(): Promise<SherpaOnnxAsrStatus> {
    return invokeOrMock("sherpa_onnx_asr_status", undefined, () => ({
        providerId: "sherpa-onnx-local",
        available: true,
        runtimeReady: false,
        activeModel: "sense-voice-small-zh",
        loadedModelId: null,
        error: null,
    }))
}

export function getSherpaOnnxAsrCatalog(): Promise<SherpaOnnxCatalogModel[]> {
    return invokeOrMock("sherpa_onnx_asr_catalog", undefined, () => [
        {
            alias: "sense-voice-small-zh" as const,
            displayName: "SenseVoice Small",
            cached: false,
            downloadedBytes: 0,
            fileSizeMb: 230,
        },
        {
            alias: "paraformer-zh" as const,
            displayName: "Paraformer ZH",
            cached: false,
            downloadedBytes: 0,
            fileSizeMb: 220,
        },
        {
            alias: "whisper-small-multi" as const,
            displayName: "Whisper Small",
            cached: false,
            downloadedBytes: 0,
            fileSizeMb: 480,
        },
        {
            alias: "qwen3-asr-0.6b-int8" as const,
            displayName: "Qwen3-ASR 0.6B INT8",
            cached: false,
            downloadedBytes: 0,
            fileSizeMb: 700,
        },
    ])
}

export function setSherpaOnnxAsrModel(modelAlias: string): Promise<void> {
    return invokeOrMock(
        "sherpa_onnx_asr_set_model",
        { modelAlias },
        () => undefined,
    )
}

export function setSherpaOnnxAsrLanguageHint(
    languageHint: string,
): Promise<void> {
    return invokeOrMock(
        "sherpa_onnx_asr_set_language_hint",
        { languageHint },
        () => undefined,
    )
}

export function prepareSherpaOnnxAsr(modelAlias: string): Promise<string> {
    return invokeOrMock(
        "sherpa_onnx_asr_prepare",
        { modelAlias },
        () => `mock-${modelAlias}`,
    )
}

export function cancelSherpaOnnxAsrPrepare(): Promise<void> {
    return invokeOrMock(
        "sherpa_onnx_asr_cancel_prepare",
        undefined,
        () => undefined,
    )
}

export function releaseSherpaOnnxAsr(): Promise<void> {
    return invokeOrMock("sherpa_onnx_asr_release", undefined, () => undefined)
}

export function getSherpaOnnxAsrModelDir(modelAlias: string): Promise<string> {
    return invokeOrMock("sherpa_onnx_asr_model_dir", { modelAlias }, () => "")
}

export function revealSherpaOnnxAsrModelDir(modelAlias: string): Promise<void> {
    return invokeOrMock(
        "sherpa_onnx_asr_reveal_model_dir",
        { modelAlias },
        () => undefined,
    )
}

export function deleteSherpaOnnxAsrModel(modelAlias: string): Promise<void> {
    return invokeOrMock(
        "sherpa_onnx_asr_delete_model",
        { modelAlias },
        () => undefined,
    )
}

export interface SherpaOnnxRemoteInfo {
    modelAlias: string
    mirror: string
    files: {
        path: string
        localPath: string
        size: number
        sha256?: string | null
    }[]
    totalBytes: number
}

export function fetchSherpaOnnxAsrRemoteInfo(
    modelAlias: string,
    mirror?: string,
): Promise<SherpaOnnxRemoteInfo> {
    return invokeOrMock(
        "sherpa_onnx_asr_fetch_remote_info",
        { modelAlias, mirror },
        () => ({
            modelAlias,
            mirror: mirror ?? "huggingface",
            files: [],
            totalBytes: 0,
        }),
    )
}

export function downloadSherpaOnnxAsrModel(
    modelAlias: string,
    mirror?: string,
): Promise<void> {
    return invokeOrMock(
        "sherpa_onnx_asr_download_model",
        { modelAlias, mirror },
        () => undefined,
    )
}

export function cancelSherpaOnnxAsrDownload(modelAlias: string): Promise<void> {
    return invokeOrMock(
        "sherpa_onnx_asr_cancel_download",
        { modelAlias },
        () => undefined,
    )
}

export type SherpaOnnxLanguageHint =
    | ""
    | "auto"
    | "zh"
    | "en"
    | "ja"
    | "ko"
    | "yue"

export type SherpaPreparePhase =
    | "runtime"
    | "model"
    | "load"
    | "finished"
    | "failed"

export interface SherpaPrepareProgress {
    phase: SherpaPreparePhase
    modelAlias: string
    label: string
    percent: number | null
    error: string | null
}
