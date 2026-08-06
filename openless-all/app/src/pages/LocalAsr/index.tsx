// LocalAsr.tsx — 本地 ASR 模型管理页。
//
// 功能：
//  - 顶部：当前激活模型 + 镜像源切换
//  - 模型列表：每行模型 = 真实尺寸 / 进度 / [下载|取消|删除|设为默认]
//  - 真实尺寸通过 fetchLocalAsrRemoteInfo 实时从 HuggingFace API 拉，**不硬编码**
//  - 监听 `local-asr-download-progress` 事件实时刷新进度
//  - Win 端引擎不可用时禁用下载按钮，提示见 issue #256

import {
    useEffect,
    useLayoutEffect,
    useMemo,
    useRef,
    useState,
    type ReactNode,
} from "react"
import { useTranslation } from "react-i18next"
import { isTauri, setActiveAsrProvider } from "../../lib/ipc"
import {
    FOUNDRY_LOCAL_ASR_MODELS,
    SHERPA_ONNX_ASR_MODELS,
    cancelFoundryLocalAsrPrepare,
    cancelSherpaOnnxAsrDownload,
    cancelSherpaOnnxAsrPrepare,
    cancelLocalAsrDownload,
    deleteFoundryLocalAsrModel,
    deleteSherpaOnnxAsrModel,
    deleteLocalAsrModel,
    downloadLocalAsrModel,
    downloadSherpaOnnxAsrModel,
    fetchLocalAsrHfCard,
    fetchLocalAsrRemoteInfo,
    fetchSherpaOnnxAsrRemoteInfo,
    getFoundryLocalAsrModelDir,
    getFoundryLocalAsrCatalog,
    getFoundryLocalAsrStatus,
    getLocalAsrEngineStatus,
    getLocalAsrModelDir,
    getLocalAsrSettings,
    getSherpaOnnxAsrCatalog,
    getSherpaOnnxAsrModelDir,
    getSherpaOnnxAsrStatus,
    listLocalAsrModels,
    prepareFoundryLocalAsr,
    prepareSherpaOnnxAsr,
    preloadLocalAsr,
    releaseFoundryLocalAsr,
    releaseLocalAsrEngine,
    releaseSherpaOnnxAsr,
    revealFoundryLocalAsrModelDir,
    revealLocalAsrModelDir,
    revealLocalAsrModelsRoot,
    revealSherpaOnnxAsrModelDir,
    setLocalAsrModelsBaseDir,
    setFoundryLocalAsrLanguageHint,
    setFoundryLocalAsrModel,
    setFoundryLocalRuntimeSource,
    setLocalAsrActiveModel,
    setLocalAsrKeepLoadedSecs,
    setLocalAsrMirror,
    setSherpaOnnxAsrLanguageHint,
    setSherpaOnnxAsrModel,
    testLocalAsrModel,
    type FoundryLocalAsrCatalogModel,
    type FoundryLocalAsrLanguageHint,
    type FoundryLocalAsrModelAlias,
    type FoundryLocalAsrStatus,
    type FoundryRuntimeSource,
    type FoundryPrepareProgress,
    type HfModelCard,
    type LocalAsrDownloadProgress,
    type LocalAsrEngineStatus,
    type LocalAsrModelStatus,
    type LocalAsrSettings,
    type LocalAsrTestResult,
    type SherpaOnnxAsrStatus,
    type SherpaOnnxCatalogModel,
    type SherpaOnnxLanguageHint,
    type SherpaOnnxModelAlias,
    type SherpaPrepareProgress,
} from "../../lib/localAsr"
import { useHotkeySettings } from "../../state/HotkeySettingsContext"
import { detectOS } from "../../components/WindowChrome"
import { SelectLite } from "../../components/ui/SelectLite"
import { Btn, Card, Collapsible, PageHeader, Pill } from "../_atoms"
import {
    formatBytes,
    formatFoundrySizeMb,
    isFoundryAlias,
    isSherpaAlias,
    isWindowsLikePlatform,
    normalizeFoundryLanguageHintForUi,
    normalizeFoundryRuntimeSourceForUi,
    normalizeSherpaLanguageHintForUi,
} from "./helpers"
import {
    DownloadProgressBlock,
    FoundryPrepareProgressBlock,
    ModelDetailPanel,
    ModelSidebar,
    type SidebarModelEntry,
    DownloadDialog,
} from "./components"
import type { RemoteSize } from "./types"

// Foundry Local Whisper 后端只在 Windows 编译实体（foundry_local_sdk 仅 Windows），
// 非 Windows 平台 runtime 是 stub 永远 unavailable。前端这一页对应的卡片、状态拉取、
// 事件订阅都必须按 OS 隔离，避免 macOS / Linux 用户看到 Windows 专属的 UI。
//
// 同理 Qwen3-ASR 后端只在 macOS 编译实体（qwen_engine / cache / local_provider 全是
// `#[cfg(target_os = "macos")]`），Qwen3 模型管理 UI 也按 IS_MAC 守严——之前用
// `!IS_WINDOWS` 会让假设的 Linux 渲染路径暴露死 UI（pr_agent #403 'Linux regression'
// 修法）。
const IS_WINDOWS = detectOS() === "win"
const IS_MAC = detectOS() === "mac"

interface LocalAsrProps {
    /// `embedded=true` 表示作为子组件嵌入「高级」设置页（Settings → Advanced）；
    /// 此时跳过外层 page padding/height、PageHeader 与独立警告 Card —— 这些由
    /// 宿主 AdvancedSection 决定（包括把警告统一到页面顶部的浮层 popup 上）。
    /// `embedded=false`（默认）保留原全屏页样式，供 v 旧版本的独立「模型设置」
    /// 页面入口使用——但当前代码里该入口已删，本分支会一并移除。
    embedded?: boolean
}

export function LocalAsr({ embedded = false }: LocalAsrProps = {}) {
    const { t } = useTranslation()
    const { prefs, updatePrefs } = useHotkeySettings()
    const [settings, setSettings] = useState<LocalAsrSettings | null>(null)
    const [models, setModels] = useState<LocalAsrModelStatus[]>([])
    // 两栏看板：右侧当前选中的模型（默认选第一个已下载的）。
    const [selectedModelId, setSelectedModelId] = useState<string | null>(null)
    // 下载弹框开关：点侧栏「下载新模型」/ 看板「下载」打开。
    const [downloadDialogOpen, setDownloadDialogOpen] = useState(false)
    const [modelDirs, setModelDirs] = useState<Record<string, string>>({})
    const [progress, setProgress] = useState<
        Record<string, LocalAsrDownloadProgress>
    >({})
    const [remoteSizes, setRemoteSizes] = useState<Record<string, RemoteSize>>(
        {},
    )
    // HF 模型卡片（下载量/收藏/简介）——弹窗右侧展示；成功结果缓存，
    // 失败记 { loading:false, error } 允许重试。
    const [hfCards, setHfCards] = useState<
        Record<string, HfModelCard | { loading: boolean; error: string | null }>
    >({})
    const [error, setError] = useState<string | null>(null)
    const [busyModelId, setBusyModelId] = useState<string | null>(null)
    const [storageBusy, setStorageBusy] = useState(false)
    const [foundryStatus, setFoundryStatus] =
        useState<FoundryLocalAsrStatus | null>(null)
    const [foundryCatalog, setFoundryCatalog] = useState<
        FoundryLocalAsrCatalogModel[]
    >([])
    const [selectedFoundryAlias, setSelectedFoundryAlias] =
        useState<FoundryLocalAsrModelAlias>("whisper-small")
    const [foundryBusy, setFoundryBusy] = useState<
        "enable" | "prepare" | "release" | "delete" | "reveal" | null
    >(null)
    const [foundryProgress, setFoundryProgress] =
        useState<FoundryPrepareProgress | null>(null)
    const [foundryCancelRequested, setFoundryCancelRequested] = useState(false)
    const [foundryModelDir, setFoundryModelDir] = useState<{
        alias: FoundryLocalAsrModelAlias
        dir: string
    } | null>(null)
    const [sherpaStatus, setSherpaStatus] =
        useState<SherpaOnnxAsrStatus | null>(null)
    const [sherpaCatalog, setSherpaCatalog] = useState<
        SherpaOnnxCatalogModel[]
    >([])
    const [selectedSherpaAlias, setSelectedSherpaAlias] =
        useState<SherpaOnnxModelAlias>("sense-voice-small-zh")
    const [sherpaBusy, setSherpaBusy] = useState<
        | "enable"
        | "prepare"
        | "download"
        | "release"
        | "delete"
        | "reveal"
        | null
    >(null)
    const [sherpaProgress, setSherpaProgress] =
        useState<SherpaPrepareProgress | null>(null)
    const [sherpaDownloadProgress, setSherpaDownloadProgress] = useState<
        Record<string, LocalAsrDownloadProgress>
    >({})
    const [sherpaRemoteSizes, setSherpaRemoteSizes] = useState<
        Record<string, RemoteSize>
    >({})
    const [sherpaCancelRequested, setSherpaCancelRequested] = useState(false)
    const [sherpaDownloadCancelRequested, setSherpaDownloadCancelRequested] =
        useState(false)
    const [sherpaModelDir, setSherpaModelDir] = useState("")
    const [testingModelId, setTestingModelId] = useState<string | null>(null)
    const [testResults, setTestResults] = useState<
        Record<string, LocalAsrTestResult | { error: string }>
    >({})
    const [engineStatus, setEngineStatus] =
        useState<LocalAsrEngineStatus | null>(null)
    const refreshTimer = useRef<number | null>(null)
    const foundryRefreshTimer = useRef<number | null>(null)
    const sherpaRefreshTimer = useRef<number | null>(null)
    const sherpaDownloadRefreshTimer = useRef<number | null>(null)
    const foundrySelectionDirty = useRef(false)
    const selectedFoundryAliasRef =
        useRef<FoundryLocalAsrModelAlias>("whisper-small")
    const sherpaSelectionDirty = useRef(false)
    const sherpaAnchorRef = useRef<HTMLDivElement>(null)
    const scrollGuard = useRef<{ scroller: HTMLElement; top: number } | null>(
        null,
    )
    const scrollGuardTimer = useRef<number | null>(null)
    const scrollGuardCleanup = useRef<(() => void) | null>(null)

    const restoreScrollGuard = () => {
        const guard = scrollGuard.current
        if (!guard) return
        if (guard.scroller.scrollTop !== guard.top) {
            guard.scroller.scrollTop = guard.top
        }
    }

    const scheduleScrollGuardRestore = () => {
        // issue #470：立即帧由下面的 rAF + 嵌套 rAF 覆盖（≈0~32ms），故移除等价的 setTimeout(…,0)；
        // 80ms / 200ms 两枪保留，用于兜住 rAF 之后才发生的异步重排（如图片晚加载）。
        window.setTimeout(restoreScrollGuard, 80)
        window.setTimeout(restoreScrollGuard, 200)
        window.requestAnimationFrame(() => {
            restoreScrollGuard()
            window.requestAnimationFrame(restoreScrollGuard)
        })
    }

    const activateScrollGuard = () => {
        if (scrollGuardCleanup.current) scrollGuardCleanup.current()
        const scroller = sherpaAnchorRef.current?.closest(
            ".ol-thinscroll",
        ) as HTMLElement | null
        if (!scroller) return
        scrollGuard.current = { scroller, top: scroller.scrollTop }
        scheduleScrollGuardRestore()

        const deactivate = () => {
            scrollGuard.current = null
            scroller.removeEventListener("wheel", deactivate)
            scroller.removeEventListener("pointerdown", deactivate)
            if (scrollGuardTimer.current) {
                window.clearTimeout(scrollGuardTimer.current)
                scrollGuardTimer.current = null
            }
            scrollGuardCleanup.current = null
        }
        scrollGuardCleanup.current = deactivate
        scroller.addEventListener("wheel", deactivate, {
            once: true,
            passive: true,
        })
        scroller.addEventListener("pointerdown", deactivate, { once: true })
        if (scrollGuardTimer.current)
            window.clearTimeout(scrollGuardTimer.current)
        scrollGuardTimer.current = window.setTimeout(deactivate, 10_000)
    }

    useLayoutEffect(() => {
        restoreScrollGuard()
    })

    const preserveEmbeddedScroll = (element: Element | null) => {
        const scroller = element?.closest(
            ".ol-thinscroll",
        ) as HTMLElement | null
        if (!scroller) return () => undefined
        const top = scroller.scrollTop
        return () => {
            window.requestAnimationFrame(() => {
                scroller.scrollTop = top
            })
        }
    }

    const setCurrentFoundryAlias = (alias: FoundryLocalAsrModelAlias) => {
        if (selectedFoundryAliasRef.current !== alias) {
            setFoundryModelDir(null)
        }
        selectedFoundryAliasRef.current = alias
        setSelectedFoundryAlias(alias)
    }

    const refreshEngineStatus = async () => {
        try {
            const status = await getLocalAsrEngineStatus()
            setEngineStatus(status)
        } catch (err) {
            console.warn("[localAsr] engine status query failed", err)
        }
    }

    const refreshFoundryStatus = async () => {
        try {
            const status = await getFoundryLocalAsrStatus()
            setFoundryStatus(status)
            if (
                !foundrySelectionDirty.current &&
                isFoundryAlias(status.activeModel)
            ) {
                setCurrentFoundryAlias(status.activeModel)
                void refreshFoundryModelDir(status.activeModel)
            }
        } catch (err) {
            const message = err instanceof Error ? err.message : String(err)
            setFoundryStatus({
                providerId: "foundry-local-whisper",
                available: false,
                runtimeReady: false,
                runtimeSource: selectedFoundryRuntimeSource,
                activeModel: selectedFoundryAlias,
                loadedModelId: null,
                endpoint: null,
                error: message,
            })
        }
    }

    const refreshFoundryCatalog = async () => {
        try {
            const catalog = await getFoundryLocalAsrCatalog()
            setFoundryCatalog(catalog)
        } catch (err) {
            console.warn("[localAsr] Foundry catalog query failed", err)
        }
    }

    const refreshFoundryModelDir = async (
        modelAlias: FoundryLocalAsrModelAlias,
    ) => {
        try {
            const dir = await getFoundryLocalAsrModelDir(modelAlias)
            setFoundryModelDir((current) => {
                if (selectedFoundryAliasRef.current !== modelAlias) {
                    return current
                }
                if (current?.alias === modelAlias && current.dir === dir) {
                    return current
                }
                return {
                    alias: modelAlias,
                    dir,
                }
            })
        } catch (err) {
            console.warn("[localAsr] Foundry model dir query failed", err)
            setFoundryModelDir((current) =>
                selectedFoundryAliasRef.current === modelAlias &&
                current?.alias === modelAlias
                    ? null
                    : current,
            )
        }
    }

    const refreshSherpaStatus = async () => {
        try {
            const status = await getSherpaOnnxAsrStatus()
            setSherpaStatus(status)
            if (
                !sherpaSelectionDirty.current &&
                isSherpaAlias(status.activeModel)
            ) {
                setSelectedSherpaAlias(status.activeModel)
                void refreshSherpaModelDir(status.activeModel)
            }
        } catch (err) {
            const message = err instanceof Error ? err.message : String(err)
            setSherpaStatus({
                providerId: "sherpa-onnx-local",
                available: false,
                runtimeReady: false,
                activeModel: selectedSherpaAlias,
                loadedModelId: null,
                error: message,
            })
        }
    }

    const refreshSherpaCatalog = async () => {
        try {
            const catalog = await getSherpaOnnxAsrCatalog()
            setSherpaCatalog(catalog)
        } catch (err) {
            console.warn("[localAsr] Sherpa catalog query failed", err)
        }
    }

    const refreshSherpaModelDir = async (modelAlias: string) => {
        try {
            const dir = await getSherpaOnnxAsrModelDir(modelAlias)
            setSherpaModelDir((current) => (current === dir ? current : dir))
        } catch (err) {
            console.warn("[localAsr] Sherpa model dir query failed", err)
        }
    }

    const refresh = async () => {
        try {
            setError(null)
            const [s, list] = await Promise.all([
                getLocalAsrSettings(),
                listLocalAsrModels(),
            ])
            setSettings(s)
            setModels(list)
            void Promise.all(
                list.map(async (m) => {
                    try {
                        const dir = await getLocalAsrModelDir(m.id)
                        setModelDirs((current) =>
                            current[m.id] === dir
                                ? current
                                : { ...current, [m.id]: dir },
                        )
                    } catch (err) {
                        console.warn("[localAsr] Qwen3 model dir query failed", err)
                    }
                }),
            )
            void refreshEngineStatus()
            if (IS_WINDOWS) {
                void refreshFoundryStatus()
                void refreshFoundryCatalog()
                void refreshFoundryModelDir(selectedFoundryAlias)
                void refreshSherpaStatus()
                void refreshSherpaCatalog()
                void refreshSherpaModelDir(selectedSherpaAlias)
                void Promise.all(
                    SHERPA_ONNX_ASR_MODELS.map((m) =>
                        ensureSherpaRemoteSize(m.alias, s.mirror),
                    ),
                )
            }
            // 拉远端真实尺寸（每个模型一次，结果留缓存）
            void Promise.all(
                list.map(async (m) => {
                    await ensureRemoteSize(m.id, s.mirror)
                }),
            )
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        }
    }

    const ensureRemoteSize = async (modelId: string, mirror: string) => {
        setRemoteSizes((prev) => {
            if (prev[modelId] && !prev[modelId].error) return prev
            return {
                ...prev,
                [modelId]: {
                    totalBytes: 0,
                    fileCount: 0,
                    loading: true,
                    error: null,
                },
            }
        })
        try {
            const info = await fetchLocalAsrRemoteInfo(modelId, mirror)
            setRemoteSizes((prev) => ({
                ...prev,
                [modelId]: {
                    totalBytes: info.totalBytes,
                    fileCount: info.files.length,
                    loading: false,
                    error: null,
                },
            }))
        } catch (e) {
            setRemoteSizes((prev) => ({
                ...prev,
                [modelId]: {
                    totalBytes: 0,
                    fileCount: 0,
                    loading: false,
                    error: e instanceof Error ? e.message : String(e),
                },
            }))
        }
    }

    // HF 模型卡片按需抓取（弹窗选中模型时），成功结果缓存不重复请求。
    const ensureHfCard = async (modelId: string, mirror: string) => {
        const current = hfCards[modelId]
        if (current) {
            if (!("loading" in current)) return // 已有成功缓存
            if (current.loading) return // 请求进行中
            // 失败结果允许重试
        }
        setHfCards((prev) => ({
            ...prev,
            [modelId]: { loading: true, error: null },
        }))
        try {
            const card = await fetchLocalAsrHfCard(modelId, mirror)
            setHfCards((prev) => ({ ...prev, [modelId]: card }))
        } catch (e) {
            setHfCards((prev) => ({
                ...prev,
                [modelId]: {
                    loading: false,
                    error: e instanceof Error ? e.message : String(e),
                },
            }))
        }
    }

    const ensureSherpaRemoteSize = async (
        modelAlias: string,
        mirror: string,
    ) => {
        setSherpaRemoteSizes((prev) => {
            if (prev[modelAlias] && !prev[modelAlias].error) return prev
            return {
                ...prev,
                [modelAlias]: {
                    totalBytes: 0,
                    fileCount: 0,
                    loading: true,
                    error: null,
                },
            }
        })
        try {
            const info = await fetchSherpaOnnxAsrRemoteInfo(modelAlias, mirror)
            setSherpaRemoteSizes((prev) => ({
                ...prev,
                [modelAlias]: {
                    totalBytes: info.totalBytes,
                    fileCount: info.files.length,
                    loading: false,
                    error: null,
                },
            }))
        } catch (e) {
            setSherpaRemoteSizes((prev) => ({
                ...prev,
                [modelAlias]: {
                    totalBytes: 0,
                    fileCount: 0,
                    loading: false,
                    error: e instanceof Error ? e.message : String(e),
                },
            }))
        }
    }

    useEffect(() => {
        void refresh()
        // 3s 轮询磁盘状态：模型被外部删除 / 下载中断时前端自动跟随（删除后
        // 看板选中自动回落、下拉回到引擎级入口），不用等重开页面。qwen3 的
        // list 是本地 fs walk，很轻；远端尺寸有缓存不会重复请求。
        const pollTimer = window.setInterval(() => {
            void refresh()
        }, 3000)
        return () => {
            window.clearInterval(pollTimer)
            if (scrollGuardCleanup.current) scrollGuardCleanup.current()
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])

    // 下载弹窗打开期间暂停 3s 轮询：弹窗是静态目录选择，轮询 setState 会
    // 重排遮罩后的看板内容，透过半透明遮罩看得到内容在跳。弹窗关闭后轮询
    // 自动重启（依赖 downloadDialogOpen 的 effect 重建 interval）。
    useEffect(() => {
        if (downloadDialogOpen) return
        const pollTimer = window.setInterval(() => {
            void refresh()
        }, 3000)
        return () => window.clearInterval(pollTimer)
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [downloadDialogOpen])

    // 引擎状态改由后端主动 emit（加载/释放/keepLoadedSecs 变更），前端零轮询。
    // 挂载时仍拉一次初值，之后 listen `local-asr:engine-changed` 增量更新。
    // 仅 Tauri 环境（浏览器 dev mock 无事件）。
    useEffect(() => {
        if (!isTauri) return
        void refreshEngineStatus()
        let unlisten: undefined | (() => void)
        let cancelled = false
        ;(async () => {
            const { listen } = await import("@tauri-apps/api/event")
            const off = await listen<LocalAsrEngineStatus>(
                "local-asr:engine-changed",
                (e) => {
                    setEngineStatus(e.payload)
                },
            )
            if (cancelled) {
                off()
            } else {
                unlisten = off
            }
        })().catch((err) =>
            console.warn("[localAsr] engine status subscribe failed", err),
        )
        return () => {
            cancelled = true
            if (unlisten) unlisten()
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])

    // 镜像变更后重拉一次远端尺寸（不同镜像 API 返回的 size 数值是一致的，
    // 但请求路径不同——切镜像时强制刷新一次让用户看到新源能否访通）。
    useEffect(() => {
        if (!settings) return
        setRemoteSizes({})
        setSherpaRemoteSizes({})
        void Promise.all(
            models.map((m) => ensureRemoteSize(m.id, settings.mirror)),
        )
        if (IS_WINDOWS) {
            void Promise.all(
                SHERPA_ONNX_ASR_MODELS.map((m) =>
                    ensureSherpaRemoteSize(m.alias, settings.mirror),
                ),
            )
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [settings?.mirror])

    // 选中模型变化时按需拉 HF 模型卡片（只请求当前选中项，不做全目录预加载
    // ——打开瞬间并行发多个网络请求 + setState 是 WKWebView 重栅格化闪烁的
    // 峰值源）。成功结果缓存，切换回已加载的模型零请求；失败条目在此重试。
    useEffect(() => {
        if (!downloadDialogOpen || !selectedModelId || !settings) return
        const entry = allSidebarEntries.find((e) => e.id === selectedModelId)
        if (!entry?.repo) return
        void ensureHfCard(selectedModelId, settings.mirror)
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [downloadDialogOpen, selectedModelId, settings?.mirror])

    // 订阅下载进度事件 — 仅 Tauri 环境（浏览器 dev mock 无事件）。
    useEffect(() => {
        if (!isTauri) return
        let unlisten: undefined | (() => void)
        let cancelled = false
        ;(async () => {
            const { listen } = await import("@tauri-apps/api/event")
            const off = await listen<LocalAsrDownloadProgress>(
                "local-asr-download-progress",
                (e) => {
                    const payload = e.payload
                    if (payload.phase === "cancelled") {
                        // 取消时清条目，bar 是否还显示交给 hasPartial 判断
                        setProgress((prev) => {
                            const next = { ...prev }
                            delete next[payload.modelId]
                            return next
                        })
                    } else {
                        setProgress((prev) => ({
                            ...prev,
                            [payload.modelId]: payload,
                        }))
                    }
                    if (
                        payload.phase === "finished" ||
                        payload.phase === "cancelled" ||
                        payload.phase === "failed"
                    ) {
                        if (refreshTimer.current)
                            window.clearTimeout(refreshTimer.current)
                        refreshTimer.current = window.setTimeout(() => {
                            void refresh()
                        }, 200)
                    }
                },
            )
            if (cancelled) {
                off()
            } else {
                unlisten = off
            }
        })().catch((err) => console.warn("[localAsr] subscribe failed", err))
        return () => {
            cancelled = true
            if (unlisten) unlisten()
            if (refreshTimer.current) window.clearTimeout(refreshTimer.current)
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])

    useEffect(() => {
        if (!isTauri || !IS_WINDOWS) return
        let unlisten: undefined | (() => void)
        let cancelled = false
        ;(async () => {
            const { listen } = await import("@tauri-apps/api/event")
            const off = await listen<FoundryPrepareProgress>(
                "foundry-local-asr-prepare-progress",
                (e) => {
                    const payload = e.payload
                    setFoundryProgress(payload)
                    if (
                        payload.phase === "finished" ||
                        payload.phase === "failed"
                    ) {
                        if (foundryRefreshTimer.current)
                            window.clearTimeout(foundryRefreshTimer.current)
                        foundryRefreshTimer.current = window.setTimeout(() => {
                            void refreshFoundryStatus()
                            void refreshFoundryCatalog()
                        }, 200)
                    }
                },
            )
            if (cancelled) {
                off()
            } else {
                unlisten = off
            }
        })().catch((err) =>
            console.warn("[localAsr] Foundry prepare subscribe failed", err),
        )
        return () => {
            cancelled = true
            if (unlisten) unlisten()
            if (foundryRefreshTimer.current)
                window.clearTimeout(foundryRefreshTimer.current)
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])

    useEffect(() => {
        if (!isTauri || !IS_WINDOWS) return
        let unlisten: undefined | (() => void)
        let cancelled = false
        ;(async () => {
            const { listen } = await import("@tauri-apps/api/event")
            const off = await listen<SherpaPrepareProgress>(
                "sherpa-onnx-asr-prepare-progress",
                (e) => {
                    const payload = e.payload
                    setSherpaProgress(payload)
                    if (
                        payload.phase === "finished" ||
                        payload.phase === "failed"
                    ) {
                        if (sherpaRefreshTimer.current)
                            window.clearTimeout(sherpaRefreshTimer.current)
                        sherpaRefreshTimer.current = window.setTimeout(() => {
                            void refreshSherpaStatus()
                            void refreshSherpaCatalog()
                        }, 200)
                    }
                },
            )
            if (cancelled) {
                off()
            } else {
                unlisten = off
            }
        })().catch((err) =>
            console.warn("[localAsr] Sherpa prepare subscribe failed", err),
        )
        return () => {
            cancelled = true
            if (unlisten) unlisten()
            if (sherpaRefreshTimer.current)
                window.clearTimeout(sherpaRefreshTimer.current)
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])

    useEffect(() => {
        if (!isTauri || !IS_WINDOWS) return
        let unlisten: undefined | (() => void)
        let cancelled = false
        ;(async () => {
            const { listen } = await import("@tauri-apps/api/event")
            const off = await listen<LocalAsrDownloadProgress>(
                "sherpa-onnx-asr-download-progress",
                (e) => {
                    const payload = e.payload
                    setSherpaDownloadProgress((prev) => ({
                        ...prev,
                        [payload.modelId]: payload,
                    }))
                    if (
                        payload.phase === "finished" ||
                        payload.phase === "cancelled" ||
                        payload.phase === "failed"
                    ) {
                        setSherpaBusy((current) =>
                            current === "download" ? null : current,
                        )
                        setSherpaDownloadCancelRequested(false)
                        if (sherpaDownloadRefreshTimer.current) {
                            window.clearTimeout(
                                sherpaDownloadRefreshTimer.current,
                            )
                        }
                        sherpaDownloadRefreshTimer.current = window.setTimeout(
                            () => {
                                void refreshSherpaStatus()
                                void refreshSherpaCatalog()
                                void refreshSherpaModelDir(payload.modelId)
                            },
                            200,
                        )
                    }
                },
            )
            if (cancelled) {
                off()
            } else {
                unlisten = off
            }
        })().catch((err) =>
            console.warn("[localAsr] Sherpa download subscribe failed", err),
        )
        return () => {
            cancelled = true
            if (unlisten) unlisten()
            if (sherpaDownloadRefreshTimer.current)
                window.clearTimeout(sherpaDownloadRefreshTimer.current)
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])

    const applyModelsBaseDir = async (modelsBaseDir: string | null) => {
        setStorageBusy(true)
        try {
            setError(null)
            const next = await setLocalAsrModelsBaseDir(modelsBaseDir)
            setSettings((current) =>
                current
                    ? {
                          ...current,
                          modelsBaseDir: next.modelsBaseDir,
                          modelsRootDir: next.modelsRootDir,
                      }
                    : current,
            )
            await refresh()
            void refreshFoundryModelDir(selectedFoundryAlias)
            void refreshSherpaModelDir(selectedSherpaAlias)
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            setStorageBusy(false)
        }
    }

    const handleChooseModelsBaseDir = async () => {
        if (!isTauri) {
            await applyModelsBaseDir("~/OpenLessModels")
            return
        }
        const { open } = await import("@tauri-apps/plugin-dialog")
        const picked = await open({
            directory: true,
            multiple: false,
            title: t("localAsr.storageChooseTitle"),
        })
        if (!picked || Array.isArray(picked)) return
        if (
            !window.confirm(
                t("localAsr.storageChangeConfirm", {
                    path: picked,
                }),
            )
        ) {
            return
        }
        await applyModelsBaseDir(picked)
    }

    const handleResetModelsBaseDir = async () => {
        if (
            !window.confirm(
                t("localAsr.storageResetConfirm", {
                    path: settings?.modelsRootDir ?? "",
                }),
            )
        ) {
            return
        }
        await applyModelsBaseDir(null)
    }

    const handleRevealModelsRoot = async () => {
        try {
            setError(null)
            await revealLocalAsrModelsRoot()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        }
    }

    const syncFoundryPrefs = async (
        modelAlias: FoundryLocalAsrModelAlias,
        enableProvider: boolean,
    ) => {
        await updatePrefs((current) => {
            const nextProvider = enableProvider
                ? "foundry-local-whisper"
                : current.activeAsrProvider
            if (
                current.activeAsrProvider === nextProvider &&
                current.foundryLocalAsrModel === modelAlias
            ) {
                return current
            }
            return {
                ...current,
                activeAsrProvider: nextProvider,
                foundryLocalAsrModel: modelAlias,
            }
        })
    }

    const handleFoundryLanguageChange = async (
        languageHint: FoundryLocalAsrLanguageHint,
        restoreScroll?: () => void,
    ) => {
        try {
            setError(null)
            await setFoundryLocalAsrLanguageHint(languageHint)
            await updatePrefs((current) =>
                current.foundryLocalAsrLanguageHint === languageHint
                    ? current
                    : {
                          ...current,
                          foundryLocalAsrLanguageHint: languageHint,
                      },
            )
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            restoreScroll?.()
        }
    }

    const handleFoundryRuntimeSourceChange = async (
        runtimeSource: FoundryRuntimeSource,
        restoreScroll?: () => void,
    ) => {
        try {
            setError(null)
            await setFoundryLocalRuntimeSource(runtimeSource)
            await updatePrefs((current) =>
                current.foundryLocalRuntimeSource === runtimeSource
                    ? current
                    : {
                          ...current,
                          foundryLocalRuntimeSource: runtimeSource,
                      },
            )
            await refreshFoundryStatus()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            restoreScroll?.()
        }
    }

    const handleEnableFoundry = async (aliasOverride?: FoundryLocalAsrModelAlias) => {
        if (!foundryAvailable) return
        const alias = aliasOverride ?? selectedFoundryAlias
        setFoundryBusy("enable")
        try {
            setError(null)
            await setFoundryLocalAsrModel(alias)
            await setActiveAsrProvider("foundry-local-whisper")
            await syncFoundryPrefs(alias, true)
            foundrySelectionDirty.current = false
            await refreshFoundryStatus()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            setFoundryBusy(null)
        }
    }

    const handlePrepareFoundry = async (aliasOverride?: FoundryLocalAsrModelAlias) => {
        if (!foundryAvailable) return
        const alias = aliasOverride ?? selectedFoundryAlias
        setFoundryBusy("prepare")
        setFoundryCancelRequested(false)
        setFoundryProgress({
            phase: "runtime",
            modelAlias: alias,
            label: t("localAsr.foundryPrepareRuntime"),
            percent: 0,
            error: null,
        })
        try {
            setError(null)
            await setFoundryLocalAsrModel(alias)
            await syncFoundryPrefs(alias, false)
            await prepareFoundryLocalAsr(alias)
            foundrySelectionDirty.current = false
            await refreshFoundryStatus()
            await refreshFoundryCatalog()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
            await refreshFoundryStatus()
            await refreshFoundryCatalog()
        } finally {
            setFoundryBusy(null)
            setFoundryCancelRequested(false)
        }
    }

    // 侧栏「下载」动作：先启用（切供应商 + 写模型），再顺序准备/下载/加载。
    // 不能并行跑 handleEnableFoundry + handlePrepareFoundry——两者都写 foundryBusy
    // 与 syncFoundryPrefs，竞态会留下互相矛盾的启用状态（pr-agent #922）。
    const handleEnableAndPrepareFoundry = async (alias: FoundryLocalAsrModelAlias) => {
        await handleEnableFoundry(alias)
        await handlePrepareFoundry(alias)
    }

    const handleCancelFoundryPrepare = async () => {
        if (foundryBusy !== "prepare") return
        setFoundryCancelRequested(true)
        try {
            await cancelFoundryLocalAsrPrepare()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        }
    }

    const handleReleaseFoundry = async () => {
        setFoundryBusy("release")
        try {
            setError(null)
            await releaseFoundryLocalAsr()
            await refreshFoundryStatus()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            setFoundryBusy(null)
        }
    }

    const handleRevealFoundryDir = async () => {
        setFoundryBusy("reveal")
        try {
            setError(null)
            await revealFoundryLocalAsrModelDir(selectedFoundryAlias)
            await refreshFoundryModelDir(selectedFoundryAlias)
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            setFoundryBusy(null)
        }
    }

    const handleDeleteFoundry = async (aliasOverride?: FoundryLocalAsrModelAlias) => {
        const alias = aliasOverride ?? selectedFoundryAlias
        const displayName =
            foundryCatalog.find((m) => m.alias === alias)?.displayName ??
            t(
                (FOUNDRY_LOCAL_ASR_MODELS.find((m) => m.alias === alias) ??
                    FOUNDRY_LOCAL_ASR_MODELS[0]).labelKey,
            )
        if (
            !window.confirm(
                t("localAsr.deleteConfirm", {
                    name: displayName,
                }),
            )
        ) {
            return
        }
        setFoundryBusy("delete")
        try {
            setError(null)
            await deleteFoundryLocalAsrModel(alias)
            await refreshFoundryStatus()
            await refreshFoundryCatalog()
            await refreshFoundryModelDir(alias)
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            setFoundryBusy(null)
        }
    }

    const syncSherpaPrefs = async (
        modelAlias: SherpaOnnxModelAlias,
        enableProvider: boolean,
    ) => {
        await updatePrefs((current) => {
            const nextProvider = enableProvider
                ? "sherpa-onnx-local"
                : current.activeAsrProvider
            if (
                current.activeAsrProvider === nextProvider &&
                current.sherpaOnnxModel === modelAlias
            ) {
                return current
            }
            return {
                ...current,
                activeAsrProvider: nextProvider,
                sherpaOnnxModel: modelAlias,
            }
        })
    }

    const activateSherpaProvider = async (modelAlias: SherpaOnnxModelAlias) => {
        await setSherpaOnnxAsrModel(modelAlias)
        await setActiveAsrProvider("sherpa-onnx-local")
        await syncSherpaPrefs(modelAlias, true)
        sherpaSelectionDirty.current = false
    }

    const handleSherpaModelChange = async (alias: SherpaOnnxModelAlias) => {
        activateScrollGuard()
        sherpaSelectionDirty.current = true
        setSelectedSherpaAlias(alias)
        void refreshSherpaModelDir(alias)
        try {
            setError(null)
            await activateSherpaProvider(alias)
            await refreshSherpaStatus()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        }
    }

    const handleSherpaLanguageChange = async (
        languageHint: SherpaOnnxLanguageHint,
        restoreScroll?: () => void,
    ) => {
        try {
            setError(null)
            await setSherpaOnnxAsrLanguageHint(languageHint)
            await updatePrefs((current) =>
                current.sherpaOnnxLanguageHint === languageHint
                    ? current
                    : {
                          ...current,
                          sherpaOnnxLanguageHint: languageHint,
                      },
            )
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            restoreScroll?.()
        }
    }

    const handleEnableSherpa = async () => {
        if (!sherpaAvailable) return
        setSherpaBusy("enable")
        try {
            setError(null)
            await activateSherpaProvider(selectedSherpaAlias)
            await refreshSherpaStatus()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            setSherpaBusy(null)
        }
    }

    const handlePrepareSherpa = async () => {
        if (!sherpaAvailable) return
        setSherpaBusy("prepare")
        setSherpaCancelRequested(false)
        setSherpaProgress({
            phase: "model",
            modelAlias: selectedSherpaAlias,
            label: t("localAsr.sherpaPrepareLocalFiles"),
            percent: 0,
            error: null,
        })
        try {
            setError(null)
            await activateSherpaProvider(selectedSherpaAlias)
            await prepareSherpaOnnxAsr(selectedSherpaAlias)
            sherpaSelectionDirty.current = false
            await refreshSherpaStatus()
            await refreshSherpaCatalog()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
            await refreshSherpaStatus()
            await refreshSherpaCatalog()
        } finally {
            setSherpaBusy(null)
            setSherpaCancelRequested(false)
        }
    }

    const handleCancelSherpaPrepare = async () => {
        if (sherpaBusy !== "prepare") return
        setSherpaCancelRequested(true)
        try {
            await cancelSherpaOnnxAsrPrepare()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        }
    }

    const handleReleaseSherpa = async () => {
        setSherpaBusy("release")
        try {
            setError(null)
            await releaseSherpaOnnxAsr()
            await refreshSherpaStatus()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            setSherpaBusy(null)
        }
    }

    const handleRevealSherpaDir = async () => {
        setSherpaBusy("reveal")
        try {
            setError(null)
            await revealSherpaOnnxAsrModelDir(selectedSherpaAlias)
            await refreshSherpaModelDir(selectedSherpaAlias)
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            setSherpaBusy(null)
        }
    }

    const handleDeleteSherpa = async (aliasOverride?: SherpaOnnxModelAlias) => {
        const alias = aliasOverride ?? selectedSherpaAlias
        const displayName =
            sherpaCatalog.find((m) => m.alias === alias)?.displayName ??
            t(
                (SHERPA_ONNX_ASR_MODELS.find((m) => m.alias === alias) ??
                    SHERPA_ONNX_ASR_MODELS[0]).labelKey,
            )
        if (
            !window.confirm(
                t("localAsr.deleteConfirm", {
                    name: displayName,
                }),
            )
        ) {
            return
        }
        setSherpaBusy("delete")
        try {
            setError(null)
            await deleteSherpaOnnxAsrModel(alias)
            setSherpaDownloadProgress((prev) => {
                const next = { ...prev }
                delete next[alias]
                return next
            })
            await refreshSherpaStatus()
            await refreshSherpaCatalog()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            setSherpaBusy(null)
        }
    }

    const handleDownloadSherpa = async (aliasOverride?: SherpaOnnxModelAlias) => {
        if (!sherpaAvailable) return
        const modelAlias = aliasOverride ?? selectedSherpaAlias
        const remoteSize = sherpaRemoteSizes[modelAlias]
        const model = sherpaCatalog.find((item) => item.alias === modelAlias)
        const initialDownloaded =
            sherpaDownloadProgress[modelAlias]?.bytesDownloaded ??
            model?.downloadedBytes ??
            0
        setSherpaBusy("download")
        setSherpaDownloadCancelRequested(false)
        setSherpaDownloadProgress((prev) => ({
            ...prev,
            [modelAlias]: {
                modelId: modelAlias,
                file: "",
                fileIndex: 0,
                fileCount: remoteSize?.fileCount ?? 0,
                bytesDownloaded: initialDownloaded,
                bytesTotal: remoteSize?.totalBytes ?? 0,
                phase: "started",
                error: null,
            },
        }))
        try {
            setError(null)
            await activateSherpaProvider(modelAlias)
            await downloadSherpaOnnxAsrModel(modelAlias, settings?.mirror)
        } catch (e) {
            const message = e instanceof Error ? e.message : String(e)
            setError(message)
            setSherpaDownloadProgress((prev) => {
                const cur = prev[modelAlias]
                return {
                    ...prev,
                    [modelAlias]: {
                        modelId: modelAlias,
                        file: cur?.file ?? "",
                        fileIndex: cur?.fileIndex ?? 0,
                        fileCount: cur?.fileCount ?? remoteSize?.fileCount ?? 0,
                        bytesDownloaded: cur?.bytesDownloaded ?? 0,
                        bytesTotal:
                            cur?.bytesTotal ?? remoteSize?.totalBytes ?? 0,
                        phase: "failed",
                        error: message,
                    },
                }
            })
            setSherpaBusy(null)
        }
    }

    const handleCancelSherpaDownload = async () => {
        if (sherpaBusy !== "download") return
        setSherpaDownloadCancelRequested(true)
        try {
            await cancelSherpaOnnxAsrDownload(selectedSherpaAlias)
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
            setSherpaDownloadCancelRequested(false)
        }
    }

    const handleDownload = async (modelId: string) => {
        setBusyModelId(modelId)
        // 重下载时，第一个后端事件到达前先用本地已知值占位，避免进度条从 0% 跳到真实位置。
        // 优先级：上一次 progress（取消后已删，通常没有）→ models 里的 downloadedBytes（cancel 时乐观写入）
        const model = models.find((m) => m.id === modelId)
        const initialDownloaded =
            progress[modelId]?.bytesDownloaded ?? model?.downloadedBytes ?? 0
        setProgress((prev) => ({
            ...prev,
            [modelId]: {
                modelId,
                file: "",
                fileIndex: 0,
                fileCount: remoteSizes[modelId]?.fileCount ?? 0,
                bytesDownloaded: initialDownloaded,
                bytesTotal: remoteSizes[modelId]?.totalBytes ?? 0,
                phase: "started",
                error: null,
            },
        }))
        try {
            await downloadLocalAsrModel(modelId, settings?.mirror)
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
            setProgress((prev) => {
                const cur = prev[modelId]
                if (cur?.phase === "started") {
                    return {
                        ...prev,
                        [modelId]: {
                            ...cur,
                            phase: "failed",
                            error: e instanceof Error ? e.message : String(e),
                        },
                    }
                }
                return prev
            })
        } finally {
            setBusyModelId(null)
        }
    }

    const handleCancel = async (modelId: string) => {
        // Progress 事件里的 bytesDownloaded 是后端 in_flight + already_done，是真实字节
        const lastBytes = progress[modelId]?.bytesDownloaded ?? 0
        try {
            await cancelLocalAsrDownload(modelId)
            setProgress((prev) => {
                const next = { ...prev }
                delete next[modelId]
                return next
            })
            // 乐观更新：让 hasPartial 立刻翻 true，不等 listener 200ms 后的 refresh
            if (lastBytes > 0) {
                setModels((prev) =>
                    prev.map((m) =>
                        m.id === modelId
                            ? { ...m, downloadedBytes: lastBytes }
                            : m,
                    ),
                )
            }
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        }
    }

    const handleDelete = async (modelId: string) => {
        if (
            !window.confirm(
                t("localAsr.deleteConfirm", {
                    name: modelId,
                }),
            )
        ) {
            return
        }
        setBusyModelId(modelId)
        try {
            await deleteLocalAsrModel(modelId)
            setProgress((prev) => {
                const next = { ...prev }
                delete next[modelId]
                return next
            })
            await refresh()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            setBusyModelId(null)
        }
    }

    const handleRevealModelDir = async (modelId: string) => {
        setBusyModelId(modelId)
        try {
            setError(null)
            await revealLocalAsrModelDir(modelId)
            const dir = await getLocalAsrModelDir(modelId)
            setModelDirs((current) => ({ ...current, [modelId]: dir }))
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        } finally {
            setBusyModelId(null)
        }
    }

    const handleKeepLoadedChange = async (seconds: number) => {
        try {
            await setLocalAsrKeepLoadedSecs(seconds)
            await refresh()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        }
    }

    const handleReleaseEngine = async () => {
        try {
            await releaseLocalAsrEngine()
            await refreshEngineStatus()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        }
    }

    const handlePreload = async () => {
        try {
            // 加载完成后后端会 emit `local-asr:engine-changed`，前端零轮询更新状态。
            await preloadLocalAsr()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        }
    }

    // 「加载并测试」（qwen3）：先设为当前模型（含把 active provider 切到本地
    // —— 与 ProvidersSection 的本地模型下拉一致），再跑内置音频测试。不再单独
    // 提供「设为默认」按钮：激活 = 在 ASR 语音转写里选择本地模型供应商。
    const handleTest = async (modelId: string) => {
        try {
            await setLocalAsrActiveModel(modelId)
            await setActiveAsrProvider("local-qwen3")
            await updatePrefs((current) =>
                current.activeAsrProvider === "local-qwen3" &&
                current.localAsrActiveModel === modelId
                    ? current
                    : {
                          ...current,
                          activeAsrProvider: "local-qwen3",
                          localAsrActiveModel: modelId,
                      },
            )
            await refresh()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
            return
        }
        setTestingModelId(modelId)
        setTestResults((prev) => {
            const next = { ...prev }
            delete next[modelId]
            return next
        })
        try {
            const result = await testLocalAsrModel(modelId)
            setTestResults((prev) => ({ ...prev, [modelId]: result }))
        } catch (e) {
            const message = e instanceof Error ? e.message : String(e)
            setTestResults((prev) => ({
                ...prev,
                [modelId]: { error: message },
            }))
        } finally {
            setTestingModelId(null)
        }
    }

    const handleMirrorChange = async (mirror: string) => {
        try {
            await setLocalAsrMirror(mirror)
            await refresh()
        } catch (e) {
            setError(e instanceof Error ? e.message : String(e))
        }
    }

    const engineAvailable = settings?.engineAvailable ?? false
    // 真实「下载中」判定：busyModelId 在下载启动后立即清空（Rust 命令同步返回，
    // 下载跑在后端线程），下载中的可靠标志是 progress 条目的 phase。用于：
    // 1) 下载弹窗 busy —— 遮罩点击不误关（用户点遮罩下的设置项时弹窗不能
    //    「像按了叉一样消失」）；2) 「＋ 下载新模型」按钮下载中禁用。
    const anyDownloadInFlight =
        Object.values(progress).some(
            (p) => p.phase === "started" || p.phase === "progress",
        ) ||
        Object.values(sherpaDownloadProgress).some(
            (p) => p.phase === "started" || p.phase === "progress",
        )
    const foundryPlatformAvailable = isWindowsLikePlatform()
    const foundryAvailable =
        foundryStatus?.available === true ||
        (foundryPlatformAvailable && foundryStatus?.available !== false)
    const foundryDefault = prefs?.activeAsrProvider === "foundry-local-whisper"
    const selectedFoundryModel =
        FOUNDRY_LOCAL_ASR_MODELS.find(
            (model) => model.alias === selectedFoundryAlias,
        ) ?? FOUNDRY_LOCAL_ASR_MODELS[0]
    const selectedFoundryCatalog = foundryCatalog.find(
        (model) => model.alias === selectedFoundryAlias,
    )
    const selectedFoundryDisplayName =
        selectedFoundryCatalog?.displayName ?? t(selectedFoundryModel.labelKey)
    const selectedFoundrySizeMb = formatFoundrySizeMb(
        selectedFoundryCatalog?.fileSizeMb,
    )
    const selectedFoundrySizeLabel = selectedFoundrySizeMb
        ? t("localAsr.foundryApproxSizeMb", { mb: selectedFoundrySizeMb })
        : t("localAsr.sizeUnknown")
    const selectedFoundryDownloadLabel = selectedFoundryCatalog?.cached
        ? t("localAsr.downloadedBadge")
        : t("localAsr.notDownloadedBadge")
    const selectedFoundryLanguageHint = normalizeFoundryLanguageHintForUi(
        prefs?.foundryLocalAsrLanguageHint ?? "",
    )
    const selectedFoundryRuntimeSource = normalizeFoundryRuntimeSourceForUi(
        prefs?.foundryLocalRuntimeSource ??
            foundryStatus?.runtimeSource ??
            "auto",
    )
    const foundryPrepareLabel =
        foundryBusy === "prepare"
            ? foundryCancelRequested
                ? t("localAsr.foundryCancelling")
                : t("localAsr.foundryPreparing")
            : foundryProgress?.phase === "failed"
              ? t("localAsr.foundryRetryPrepare")
              : t("localAsr.foundryPrepare")
    const sherpaAvailable =
        sherpaStatus?.available === true ||
        (foundryPlatformAvailable && sherpaStatus?.available !== false)
    const sherpaDefault = prefs?.activeAsrProvider === "sherpa-onnx-local"
    const selectedSherpaModel =
        SHERPA_ONNX_ASR_MODELS.find(
            (model) => model.alias === selectedSherpaAlias,
        ) ?? SHERPA_ONNX_ASR_MODELS[0]
    const selectedSherpaUsesReleaseArchive =
        selectedSherpaAlias === "qwen3-asr-0.6b-int8"
    const selectedSherpaMirrorValue = selectedSherpaUsesReleaseArchive
        ? "github-release"
        : (settings?.mirror ?? "huggingface")
    const selectedSherpaCatalog = sherpaCatalog.find(
        (model) => model.alias === selectedSherpaAlias,
    )
    const selectedSherpaDisplayName =
        selectedSherpaCatalog?.displayName ?? t(selectedSherpaModel.labelKey)
    const selectedSherpaRemoteSize = sherpaRemoteSizes[selectedSherpaAlias]
    const selectedSherpaDownloadProgress =
        sherpaDownloadProgress[selectedSherpaAlias]
    const selectedSherpaDownloadedBytes =
        selectedSherpaCatalog?.downloadedBytes ?? 0
    const selectedSherpaProgressBytes =
        selectedSherpaDownloadProgress?.bytesDownloaded ?? 0
    const selectedSherpaPartialBytes = Math.max(
        selectedSherpaProgressBytes,
        selectedSherpaDownloadedBytes,
    )
    const isSherpaDownloading =
        selectedSherpaDownloadProgress?.phase === "started" ||
        selectedSherpaDownloadProgress?.phase === "progress"
    const hasSherpaPartial =
        selectedSherpaCatalog?.cached !== true &&
        selectedSherpaDownloadProgress?.phase !== "finished" &&
        selectedSherpaPartialBytes > 0
    const selectedSherpaHasLocalFiles =
        selectedSherpaCatalog?.cached === true ||
        selectedSherpaDownloadedBytes > 0
    const canDeleteSelectedSherpa =
        selectedSherpaHasLocalFiles || hasSherpaPartial
    const showSherpaDownloadProgress =
        isSherpaDownloading ||
        selectedSherpaDownloadProgress?.phase === "failed" ||
        hasSherpaPartial
    const selectedSherpaDownloadProgressForDisplay =
        selectedSherpaDownloadProgress ??
        (hasSherpaPartial
            ? {
                  modelId: selectedSherpaAlias,
                  file: "",
                  fileIndex: 0,
                  fileCount: selectedSherpaRemoteSize?.fileCount ?? 0,
                  bytesDownloaded: selectedSherpaDownloadedBytes,
                  bytesTotal: selectedSherpaRemoteSize?.totalBytes ?? 0,
                  phase: "progress" as const,
                  error: null,
              }
            : undefined)
    const selectedSherpaSizeMb = formatFoundrySizeMb(
        selectedSherpaCatalog?.fileSizeMb,
    )
    const selectedSherpaSizeLabel = selectedSherpaRemoteSize?.loading
        ? t("localAsr.sizeLoading")
        : selectedSherpaRemoteSize?.totalBytes
          ? `${formatBytes(selectedSherpaRemoteSize.totalBytes)} · ${selectedSherpaRemoteSize.fileCount} ${t("localAsr.files")}`
          : selectedSherpaSizeMb
            ? t("localAsr.foundryApproxSizeMb", { mb: selectedSherpaSizeMb })
            : t("localAsr.sizeUnknown")
    const selectedSherpaDownloadLabel = selectedSherpaCatalog?.cached
        ? t("localAsr.downloadedBadge")
        : t("localAsr.notDownloadedBadge")
    const selectedSherpaLanguageHint = normalizeSherpaLanguageHintForUi(
        prefs?.sherpaOnnxLanguageHint ?? "",
    )
    const sherpaModelOptions = useMemo(
        () =>
            SHERPA_ONNX_ASR_MODELS.map((model) => {
                const catalog = sherpaCatalog.find(
                    (item) => item.alias === model.alias,
                )
                const remoteSize = sherpaRemoteSizes[model.alias]
                const sizeMb = formatFoundrySizeMb(catalog?.fileSizeMb)
                const sizeLabel = remoteSize?.totalBytes
                    ? formatBytes(remoteSize.totalBytes)
                    : sizeMb
                      ? t("localAsr.foundryApproxSizeMb", { mb: sizeMb })
                      : ""
                return {
                    value: model.alias,
                    label: `${t(model.labelKey)}${
                        sizeLabel ? ` · ${sizeLabel}` : ""
                    }`,
                }
            }),
        [sherpaCatalog, sherpaRemoteSizes, t],
    )
    const sherpaLanguageOptions = useMemo(
        () => [
            {
                value: "",
                label: t("localAsr.foundryLanguageAuto"),
            },
            {
                value: "zh",
                label: t("localAsr.foundryLanguageZh"),
            },
            {
                value: "en",
                label: t("localAsr.foundryLanguageEn"),
            },
            {
                value: "ja",
                label: t("localAsr.sherpaLanguageJa"),
            },
            {
                value: "ko",
                label: t("localAsr.sherpaLanguageKo"),
            },
            {
                value: "yue",
                label: t("localAsr.sherpaLanguageYue"),
            },
        ],
        [t],
    )
    const sherpaPrepareLabel =
        sherpaBusy === "prepare"
            ? sherpaCancelRequested
                ? t("localAsr.foundryCancelling")
                : t("localAsr.sherpaPreparing")
            : sherpaProgress?.phase === "failed"
              ? t("localAsr.foundryRetryPrepare")
              : t("localAsr.sherpaPrepare")

    // embedded=true 嵌入「高级」设置：跳过外层 page padding/height、PageHeader，
    // 与独立警告 Card——AdvancedSection 自己负责标题与短警告 + 启用时的浮层 popup，
    // LocalAsr 只输出实际功能 Cards（Foundry / Qwen3 模型状态 / 模型列表）。
    const Wrapper = embedded
        ? (props: { children: ReactNode }) => <>{props.children}</>
        : (props: { children: ReactNode }) => (
              <div
                  style={{
                      padding: "20px 28px 32px",
                      overflowY: "auto",
                      height: "100%",
                  }}
              >
                  {props.children}
              </div>
          )

    // 本地模型页分组标题：模型选择 / 下载与管理 / 其他。
    const GroupTitle = ({ children }: { children: ReactNode }) => (
        <div
            style={{
                fontSize: 12.5,
                fontWeight: 600,
                color: "var(--ol-ink-3)",
                letterSpacing: "0.02em",
                margin: "18px 0 8px",
            }}
        >
            {children}
        </div>
    )

    // ─── 两栏看板的统一模型条目（Qwen3 / sherpa-onnx / foundry 归一化） ───
    // allSidebarEntries = 全目录（下载弹窗用，未下载/下载中/已下载全列出，
    // 让「下载新模型」弹窗能选到所有可获取的模型）；
    // sidebarEntries = 只列已下载 / 下载中的模型（看板用，未下载的走
    // 「＋ 下载新模型」弹窗获取）。
    const allSidebarEntries = useMemo<SidebarModelEntry[]>(() => {
        const entries: SidebarModelEntry[] = []
        // macOS：Qwen3 引擎
        for (const m of models) {
            const isDownloading =
                Boolean(progress[m.id]) &&
                (progress[m.id]?.phase === "started" ||
                    progress[m.id]?.phase === "progress")
            entries.push({
                id: m.id,
                name: m.id,
                repo: m.hfRepo,
                remoteBytes:
                    remoteSizes[m.id]?.totalBytes || m.downloadedBytes || undefined,
                isDownloaded: m.isDownloaded,
                isDownloading,
                percent: isDownloading
                    ? progress[m.id] && progress[m.id]?.bytesTotal > 0
                        ? (progress[m.id]!.bytesDownloaded /
                              progress[m.id]!.bytesTotal) *
                          100
                        : 0
                    : null,
                isActive:
                    settings?.activeModel === m.id &&
                    prefs?.activeAsrProvider === "local-qwen3",
                engine: "qwen3",
            })
        }
        // Windows：sherpa-onnx + foundry
        for (const c of sherpaCatalog) {
            const isDownloading =
                Boolean(sherpaDownloadProgress[c.alias]) &&
                (sherpaDownloadProgress[c.alias]?.phase === "started" ||
                    sherpaDownloadProgress[c.alias]?.phase === "progress")
            entries.push({
                id: c.alias,
                name: c.displayName || c.alias,
                remoteBytes:
                    sherpaRemoteSizes[c.alias]?.totalBytes ||
                    (c.fileSizeMb != null ? c.fileSizeMb * 1024 * 1024 : undefined),
                isDownloaded: c.cached,
                isDownloading,
                percent: isDownloading
                    ? sherpaDownloadProgress[c.alias] &&
                      sherpaDownloadProgress[c.alias]?.bytesTotal > 0
                        ? (sherpaDownloadProgress[c.alias]!.bytesDownloaded /
                              sherpaDownloadProgress[c.alias]!.bytesTotal) *
                          100
                        : 0
                    : null,
                isActive:
                    sherpaStatus?.activeModel === c.alias &&
                    prefs?.activeAsrProvider === "sherpa-onnx-local",
                engine: "sherpa",
            })
        }
        for (const c of foundryCatalog) {
            // foundry 下载发生在 prepare 内（runtime/model/load 阶段），cached
            // 仍是 false，靠 prepare 进度判定「下载中」保住条目。
            const isDownloading =
                foundryProgress?.modelAlias === c.alias &&
                (foundryProgress.phase === "runtime" ||
                    foundryProgress.phase === "model" ||
                    foundryProgress.phase === "load")
            entries.push({
                id: c.alias,
                name: c.displayName || c.alias,
                remoteBytes:
                    c.fileSizeMb != null ? c.fileSizeMb * 1024 * 1024 : undefined,
                isDownloaded: c.cached,
                isDownloading,
                percent:
                    isDownloading && foundryProgress?.percent != null
                        ? foundryProgress.percent
                        : null,
                isActive:
                    foundryStatus?.activeModel === c.alias &&
                    prefs?.activeAsrProvider === "foundry-local-whisper",
                engine: "foundry",
            })
        }
        return entries
    }, [
        models,
        remoteSizes,
        progress,
        settings?.activeModel,
        prefs?.activeAsrProvider,
        sherpaCatalog,
        sherpaRemoteSizes,
        sherpaDownloadProgress,
        sherpaStatus?.activeModel,
        foundryCatalog,
        foundryProgress,
        foundryStatus?.activeModel,
    ])

    // 看板只展示已下载 / 下载中的模型（下载中必须有实时进度可见）。
    const sidebarEntries = useMemo<SidebarModelEntry[]>(
        () => allSidebarEntries.filter((e) => e.isDownloaded || e.isDownloading),
        [allSidebarEntries],
    )

    // 弹窗打开时若看板选中项不在全目录里（零下载用户未选中任何模型），把
    // 弹窗默认高亮写回 selectedModelId——弹窗高亮与看板 state 一致，后续
    // 切换 / 开始下载都基于同一值，没有「弹窗内显示 A、逻辑上是 B」的分叉。
    useEffect(() => {
        if (!downloadDialogOpen) return
        const valid = allSidebarEntries.some((e) => e.id === selectedModelId)
        if (valid) return
        const fallback =
            allSidebarEntries.find((e) => !e.isDownloaded) ??
            allSidebarEntries[0] ??
            null
        setSelectedModelId(fallback?.id ?? null)
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [downloadDialogOpen, allSidebarEntries, selectedModelId])

    const selectedEntry =
        sidebarEntries.find((e) => e.id === selectedModelId) ?? null

    // 侧栏选中默认：首次渲染后若没有选中项，选中第一个已下载模型。
    useLayoutEffect(() => {
        // 下载弹窗打开时弹窗内高亮未下载模型是合法的（选中即准备下载），
        // 不能让看板的回落逻辑把弹窗高亮抢走；弹窗关闭后再回落。
        if (downloadDialogOpen) return
        // 选中项被删除（或从未选中）时回落到第一个已下载模型，避免侧栏无高亮、
        // 详情面板停在空态。
        const stillExists =
            selectedModelId !== null &&
            sidebarEntries.some((e) => e.id === selectedModelId)
        if (stillExists) return
        const firstDownloaded = sidebarEntries.find((e) => e.isDownloaded)
        setSelectedModelId(firstDownloaded?.id ?? sidebarEntries[0]?.id ?? null)
    }, [sidebarEntries, selectedModelId, downloadDialogOpen])

    // 从侧栏/看板分派引擎动作。不再有 setActive——激活 = 在 ASR 语音转写里
    // 选本地模型供应商，「加载并测试」负责把模型设为当前使用。
    const dispatchEntryAction = (entry: SidebarModelEntry, action: "download" | "delete" | "reveal") => {
        if (entry.engine === "qwen3") {
            if (action === "download") void handleDownload(entry.id)
            else if (action === "delete") void handleDelete(entry.id)
            else if (action === "reveal") void handleRevealModelDir(entry.id)
        } else if (entry.engine === "sherpa") {
            const alias = entry.id as SherpaOnnxModelAlias
            if (action === "download") {
                setSelectedSherpaAlias(alias)
                // 显式传 alias：setTimeout 里的闭包拿不到新 state（handler 读的是
                // 当前 render 的 selectedSherpaAlias），不传会操作到上一个模型。
                window.setTimeout(() => void handleDownloadSherpa(alias), 0)
            } else if (action === "delete") {
                setSelectedSherpaAlias(alias)
                window.setTimeout(() => void handleDeleteSherpa(alias), 0)
            }
        } else if (entry.engine === "foundry") {
            const alias = entry.id as FoundryLocalAsrModelAlias
            if (action === "download") {
                setSelectedFoundryAlias(alias)
                void handleEnableAndPrepareFoundry(alias)
            } else if (action === "delete") {
                setSelectedFoundryAlias(alias)
                void handleDeleteFoundry(alias)
            }
        }
    }

    // 下载弹框「开始下载」：把弹框当前选中项分派到对应引擎的下载入口。
    // 弹框列表是全目录（allSidebarEntries），选中项可能不在看板过滤列表里；
    // 弹框默认选中第一项时 selectedModelId 可能还是 null，回退到第一个未下载条目。
    const startDownloadFromDialog = () => {
        const dialogEntry =
            allSidebarEntries.find((e) => e.id === selectedModelId) ??
            allSidebarEntries.find((e) => !e.isDownloaded) ??
            null
        if (!dialogEntry || dialogEntry.isDownloaded) return
        dispatchEntryAction(dialogEntry, "download")
        setDownloadDialogOpen(false)
    }

    const selectedEntryRemote = selectedEntry
        ? selectedEntry.engine === "qwen3"
            ? remoteSizes[selectedEntry.id]
            : selectedEntry.engine === "sherpa"
              ? sherpaRemoteSizes[selectedEntry.id]
              : null
        : null
    const selectedEntryProgress =
        selectedEntry?.engine === "qwen3"
            ? progress[selectedEntry.id]
            : selectedEntry?.engine === "sherpa"
              ? sherpaDownloadProgress[selectedEntry.id]
              : undefined
    const selectedEntryPercent =
        selectedEntryProgress && selectedEntryProgress.bytesTotal > 0
            ? (selectedEntryProgress.bytesDownloaded /
                  selectedEntryProgress.bytesTotal) *
              100
            : null

    return (
        <Wrapper>
            {!embedded && (
                <PageHeader
                    kicker={t("localAsr.kicker")}
                    title={t("localAsr.title")}
                    desc={t("localAsr.desc")}
                />
            )}

            {/* ─── 右上角下载进度浮层已全局化（App 根挂载，任何页面常驻），
                 此处不再渲染；页面内进度仍由 progress / sherpaDownloadProgress
                 驱动看板详情条。 ─── */}

            {!embedded && (
                /* 性能/质量预期警告 —— embedded 模式下由 AdvancedSection 自己渲染，避免重复。 */
                <Card
                    style={{
                        marginBottom: 16,
                        background: "rgba(255, 215, 130, 0.18)",
                    }}
                >
                    <div
                        style={{
                            fontSize: 13,
                            color: "var(--ol-ink-2)",
                            lineHeight: 1.6,
                        }}
                    >
                        ⚠️ {t("localAsr.performanceWarning")}
                    </div>
                </Card>
            )}

            {/* ─── 模型管理看板：左侧模型选择（竖排，已下载打绿勾），右侧详情
                 （HF 实时抓取的尺寸/文件数）+ 操作。全平台归一化（Qwen3 /
                 sherpa-onnx / foundry），作为设置里的单独板块而非独立窗口。 ─── */}
            <Card style={{ marginBottom: 16 }}>
                <div
                    style={{
                        fontSize: 14,
                        fontWeight: 700,
                        color: "var(--ol-ink)",
                        marginBottom: 2,
                    }}
                >
                    {t("localAsr.modelSelectTitle")}
                </div>
                <div
                    style={{
                        display: "grid",
                        gridTemplateColumns: "minmax(0, 240px) minmax(0, 1fr)",
                        gap: 16,
                    }}
                >
                    <ModelSidebar
                        entries={sidebarEntries}
                        selectedId={selectedModelId}
                        onSelect={(id) => {
                            setSelectedModelId(id)
                            // 选中瞬间校验磁盘状态（模型文件可能已被外部删除），
                            // 立刻反映到列表与详情，不等 3s 轮询。
                            void refresh()
                        }}
                        onOpenDownload={() => setDownloadDialogOpen(true)}
                        downloadDisabled={
                            busyModelId !== null ||
                            sherpaBusy !== null ||
                            anyDownloadInFlight
                        }
                    />                    <div
                        style={{
                            paddingLeft: 16,
                            borderLeft: "0.5px solid var(--ol-line)",
                            minWidth: 0,
                        }}
                    >
                        <ModelDetailPanel
                            entry={selectedEntry}
                            fileCount={selectedEntryRemote?.fileCount ?? null}
                            mirrorLabel={
                                selectedEntry?.engine === "qwen3"
                                    ? settings?.mirror === "hf-mirror"
                                        ? "hf-mirror"
                                        : "huggingface"
                                    : selectedEntry?.engine === "sherpa"
                                      ? (settings?.mirror ?? "huggingface")
                                      : undefined
                            }
                            downloading={selectedEntry ? Boolean(selectedEntryProgress) : false}
                            progressPercent={selectedEntryPercent}
                            busy={busyModelId !== null || sherpaBusy !== null}
                            onDownload={() =>
                                selectedEntry && dispatchEntryAction(selectedEntry, "download")
                            }
                            onCancel={() => {
                                if (!selectedEntry) return
                                if (selectedEntry.engine === "qwen3")
                                    void handleCancel(selectedEntry.id)
                                else if (selectedEntry.engine === "sherpa")
                                    void handleCancelSherpaDownload()
                            }}
                            onDelete={() =>
                                selectedEntry && dispatchEntryAction(selectedEntry, "delete")
                            }
                            onReveal={() =>
                                selectedEntry && dispatchEntryAction(selectedEntry, "reveal")
                            }
                            onTest={() =>
                                selectedEntry?.engine === "qwen3" &&
                                void handleTest(selectedEntry.id)
                            }
                            showTest={selectedEntry?.engine === "qwen3"}
                            testResult={
                                selectedEntry
                                    ? (testResults[selectedEntry.id] ?? null)
                                    : null
                            }
                            testing={
                                selectedEntry?.engine === "qwen3" &&
                                testingModelId === selectedEntry.id
                            }
                        />
                    </div>
                </div>
            </Card>

            {/* ─── 收纳：下载与存储设置（镜像源 · 模型存储位置 · 内存引擎）——默认收起，
                 需要手动点开。日常的下载 / 管理 / 测试不依赖这些低频配置。 ─── */}
            <div style={{ marginBottom: 16 }}>
                <Collapsible
                    title={t("localAsr.downloadSettingsTitle")}
                    desc={t("localAsr.downloadSettingsDesc")}
                >
                    {IS_MAC && (
                        <>
                        <Card style={{ marginBottom: 16 }}>
                            <div
                                style={{
                                    display: "flex",
                                    alignItems: "center",
                                    justifyContent: "space-between",
                                    gap: 16,
                                }}
                            >
                                <div>
                                    <div
                                        style={{
                                            fontSize: 12,
                                            fontWeight: 600,
                                            color: "var(--ol-ink-4)",
                                            marginBottom: 4,
                                        }}
                                    >
                                        {t("localAsr.mirrorLabel")}
                                    </div>
                                    <div
                                        style={{
                                            fontSize: 13,
                                            color: "var(--ol-ink-3)",
                                        }}
                                    >
                                        {t("localAsr.mirrorDesc")}
                                    </div>
                                </div>
                                <select
                                    value={settings?.mirror ?? "huggingface"}
                                    onChange={(e) =>
                                        void handleMirrorChange(e.target.value)
                                    }
                                    style={{
                                        fontSize: 13,
                                        padding: "6px 10px",
                                        borderRadius: 8,
                                        border: "0.5px solid rgba(0,0,0,0.12)",
                                        background: "var(--ol-surface)",
                                        color: "var(--ol-ink)",
                                        minWidth: 200,
                                    }}
                                >
                                    <option value="huggingface">
                                        {t("localAsr.mirrorHuggingface")}
                                    </option>
                                    <option value="hf-mirror">
                                        {t("localAsr.mirrorHfMirror")}
                                    </option>
                                </select>
                            </div>
                        </Card>
                        {/* 运行时设置卡：内存中的引擎状态 + 多久释放 + 立即释放 */}
                        {engineAvailable && (
                            <Card style={{ marginBottom: 16 }}>
                                <div
                                    style={{
                                        display: "flex",
                                        flexDirection: "column",
                                        gap: 12,
                                    }}
                                >
                                    <div
                                        style={{
                                            display: "flex",
                                            alignItems: "center",
                                            justifyContent: "space-between",
                                            gap: 12,
                                            flexWrap: "wrap",
                                        }}
                                    >
                                        <div>
                                            <div
                                                style={{
                                                    fontSize: 12,
                                                    fontWeight: 600,
                                                    color: "var(--ol-ink-4)",
                                                    marginBottom: 4,
                                                }}
                                            >
                                                {t("localAsr.engineStatusLabel")}
                                            </div>
                                            <div
                                                style={{
                                                    fontSize: 13,
                                                    color: "var(--ol-ink-3)",
                                                }}
                                            >
                                                {engineStatus?.loaded
                                                    ? t("localAsr.engineLoaded", {
                                                          model:
                                                              engineStatus.modelId ??
                                                              "",
                                                      })
                                                    : t("localAsr.engineUnloaded")}
                                            </div>
                                        </div>
                                        <div style={{ display: "flex", gap: 8 }}>
                                            {engineStatus?.loaded ? (
                                                <Btn
                                                    variant="ghost"
                                                    size="sm"
                                                    onClick={() =>
                                                        void handleReleaseEngine()
                                                    }
                                                >
                                                    {t("localAsr.releaseNow")}
                                                </Btn>
                                            ) : (
                                                <Btn
                                                    variant="ghost"
                                                    size="sm"
                                                    onClick={() =>
                                                        void handlePreload()
                                                    }
                                                >
                                                    {t("localAsr.loadNow")}
                                                </Btn>
                                            )}
                                        </div>
                                    </div>
                                    <div
                                        style={{
                                            display: "flex",
                                            alignItems: "center",
                                            justifyContent: "space-between",
                                            gap: 12,
                                            flexWrap: "wrap",
                                        }}
                                    >
                                        <div style={{ minWidth: 0 }}>
                                            <div
                                                style={{
                                                    fontSize: 12,
                                                    fontWeight: 600,
                                                    color: "var(--ol-ink-4)",
                                                    marginBottom: 4,
                                                }}
                                            >
                                                {t("localAsr.keepLoadedLabel")}
                                            </div>
                                            <div
                                                style={{
                                                    fontSize: 12,
                                                    color: "var(--ol-ink-3)",
                                                    lineHeight: 1.5,
                                                }}
                                            >
                                                {t("localAsr.keepLoadedDesc")}
                                            </div>
                                        </div>
                                        <select
                                            value={
                                                engineStatus?.keepLoadedSecs ?? 300
                                            }
                                            onChange={(e) =>
                                                void handleKeepLoadedChange(
                                                    Number(e.target.value),
                                                )
                                            }
                                            style={{
                                                fontSize: 13,
                                                padding: "6px 10px",
                                                borderRadius: 8,
                                                border: "0.5px solid rgba(0,0,0,0.12)",
                                                background: "var(--ol-surface)",
                                                color: "var(--ol-ink)",
                                                minWidth: 200,
                                            }}
                                        >
                                            <option value={0}>
                                                {t("localAsr.keepImmediate")}
                                            </option>
                                            <option value={60}>
                                                {t("localAsr.keep1min")}
                                            </option>
                                            <option value={300}>
                                                {t("localAsr.keep5min")}
                                            </option>
                                            <option value={1800}>
                                                {t("localAsr.keep30min")}
                                            </option>
                                            <option value={86400}>
                                                {t("localAsr.keepForever")}
                                            </option>
                                        </select>
                                    </div>
                                </div>
                            </Card>
                        )}
                        </>
                    )}
                    <Card style={{ marginBottom: 16 }}>
                        <div
                            style={{
                                display: "flex",
                                flexDirection: "column",
                                gap: 12,
                            }}
                        >
                            <div
                                style={{
                                    display: "flex",
                                    justifyContent: "space-between",
                                    gap: 16,
                                    flexWrap: "wrap",
                                }}
                            >
                                <div style={{ minWidth: 0, flex: "1 1 360px" }}>
                                    <div
                                        style={{
                                            fontSize: 14,
                                            fontWeight: 700,
                                            color: "var(--ol-ink)",
                                            marginBottom: 6,
                                        }}
                                    >
                                        {t("localAsr.storageTitle")}
                                    </div>
                                    <div
                                        style={{
                                            fontSize: 12.5,
                                            color: "var(--ol-ink-3)",
                                            lineHeight: 1.6,
                                        }}
                                    >
                                        <div>
                                            <span
                                                style={{ color: "var(--ol-ink-4)" }}
                                            >
                                                {t("localAsr.storageBaseDir")}:{" "}
                                            </span>
                                            <code>
                                                {settings?.modelsBaseDir ??
                                                    t("localAsr.storageDefault")}
                                            </code>
                                        </div>
                                        <div>
                                            <span
                                                style={{ color: "var(--ol-ink-4)" }}
                                            >
                                                {t("localAsr.storageModelsRoot")}:{" "}
                                            </span>
                                            <code>{settings?.modelsRootDir ?? "—"}</code>
                                        </div>
                                    </div>
                                </div>
                                <div
                                    style={{
                                        display: "flex",
                                        gap: 8,
                                        flexWrap: "wrap",
                                        justifyContent: "flex-end",
                                        alignContent: "flex-start",
                                    }}
                                >
                                    <Btn
                                        variant="primary"
                                        size="sm"
                                        disabled={storageBusy}
                                        onClick={() => void handleChooseModelsBaseDir()}
                                    >
                                        {storageBusy
                                            ? t("common.loading")
                                            : t("localAsr.storageChoose")}
                                    </Btn>
                                    <Btn
                                        variant="ghost"
                                        size="sm"
                                        disabled={storageBusy || !settings?.modelsBaseDir}
                                        onClick={() => void handleResetModelsBaseDir()}
                                    >
                                        {t("localAsr.storageReset")}
                                    </Btn>
                                    <Btn
                                        variant="ghost"
                                        size="sm"
                                        disabled={storageBusy}
                                        onClick={() => void handleRevealModelsRoot()}
                                    >
                                        {t("localAsr.storageReveal")}
                                    </Btn>
                                </div>
                            </div>
                            <div
                                style={{
                                    fontSize: 12,
                                    color: "var(--ol-ink-4)",
                                    lineHeight: 1.55,
                                }}
                            >
                                {t("localAsr.storageDesc")}
                            </div>
                        </div>
                    </Card>
                </Collapsible>
            </div>
{/* ─── 下载弹框：左侧模型选择 + 右侧详情，最下方开始下载。 ─── */}
            {downloadDialogOpen && (
                <DownloadDialog
                    entries={allSidebarEntries}
                    selectedId={selectedModelId}
                    onSelect={setSelectedModelId}
                    sizeOf={(id) => {
                        const entry = allSidebarEntries.find((e) => e.id === id)
                        return entry?.remoteBytes ?? null
                    }}
                    fileCountOf={(id) => {
                        const entry = allSidebarEntries.find((e) => e.id === id)
                        if (!entry) return null
                        const remote =
                            entry.engine === "qwen3"
                                ? remoteSizes[id]
                                : entry.engine === "sherpa"
                                  ? sherpaRemoteSizes[id]
                                  : null
                        return remote?.fileCount ?? null
                    }}
                    busy={busyModelId !== null || anyDownloadInFlight}
                    hfCardOf={(id) => {
                        const state = hfCards[id]
                        if (!state) return null
                        if ("loading" in state) {
                            return state.loading
                                ? { status: "loading" as const }
                                : {
                                      status: "error" as const,
                                      message: state.error ?? "",
                                  }
                        }
                        return { status: "ok" as const, card: state }
                    }}
                    onStart={startDownloadFromDialog}
                    onClose={() => setDownloadDialogOpen(false)}
                />
            )}

            {/* ─── 分组：下载与管理（各引擎的模型获取/准备/下载） ─── */}
            <GroupTitle>{t("localAsr.groupDownload")}</GroupTitle>


            {IS_WINDOWS && (
                <Card style={{ marginBottom: 16 }}>
                    <div
                        style={{
                            display: "flex",
                            flexDirection: "column",
                            gap: 14,
                        }}
                    >
                        <div
                            style={{
                                display: "flex",
                                justifyContent: "space-between",
                                gap: 16,
                                flexWrap: "wrap",
                            }}
                        >
                            <div style={{ minWidth: 0, flex: "1 1 360px" }}>
                                <div
                                    style={{
                                        display: "flex",
                                        alignItems: "center",
                                        gap: 8,
                                        marginBottom: 6,
                                        flexWrap: "wrap",
                                    }}
                                >
                                    <div
                                        style={{
                                            fontSize: 14,
                                            fontWeight: 700,
                                            color: "var(--ol-ink)",
                                        }}
                                    >
                                        {t("localAsr.foundryTitle")}
                                    </div>
                                    {foundryDefault && (
                                        <Pill tone="blue" size="sm">
                                            {t("localAsr.activeBadge")}
                                        </Pill>
                                    )}
                                    <Pill
                                        tone={
                                            foundryStatus?.available
                                                ? "ok"
                                                : "outline"
                                        }
                                        size="sm"
                                    >
                                        {foundryStatus?.available
                                            ? t("localAsr.foundryAvailable")
                                            : t("localAsr.foundryUnavailable")}
                                    </Pill>
                                    <Pill
                                        tone={
                                            foundryStatus?.runtimeReady
                                                ? "ok"
                                                : "outline"
                                        }
                                        size="sm"
                                    >
                                        {foundryStatus?.runtimeReady
                                            ? t("localAsr.foundryRuntimeReady")
                                            : t(
                                                  "localAsr.foundryRuntimeMissing",
                                              )}
                                    </Pill>
                                </div>
                                <div
                                    style={{
                                        fontSize: 13,
                                        color: "var(--ol-ink-3)",
                                        lineHeight: 1.55,
                                    }}
                                >
                                    {t("localAsr.foundryDesc")}
                                </div>
                            </div>
                            <div
                                style={{
                                    display: "flex",
                                    gap: 10,
                                    flexWrap: "wrap",
                                    justifyContent: "flex-end",
                                }}
                            >
                                <label
                                    style={{
                                        display: "flex",
                                        flexDirection: "column",
                                        gap: 4,
                                        fontSize: 11,
                                        color: "var(--ol-ink-4)",
                                    }}
                                >
                                    {t("localAsr.foundrySelectedModel")}
                                    <select
                                        value={selectedFoundryAlias}
                                        onChange={(e) => {
                                            const restoreScroll =
                                                preserveEmbeddedScroll(
                                                    e.currentTarget,
                                                )
                                            const nextAlias = e.target
                                                .value as FoundryLocalAsrModelAlias
                                            foundrySelectionDirty.current = true
                                            setCurrentFoundryAlias(nextAlias)
                                            void refreshFoundryModelDir(nextAlias)
                                            restoreScroll()
                                        }}
                                        disabled={foundryBusy !== null}
                                        style={{
                                            fontSize: 13,
                                            padding: "6px 10px",
                                            borderRadius: 8,
                                            border: "0.5px solid rgba(0,0,0,0.12)",
                                            background: "var(--ol-surface)",
                                            color: "var(--ol-ink)",
                                            minWidth: 260,
                                        }}
                                    >
                                        {FOUNDRY_LOCAL_ASR_MODELS.map(
                                            (model) => {
                                                const catalog =
                                                    foundryCatalog.find(
                                                        (item) =>
                                                            item.alias ===
                                                            model.alias,
                                                    )
                                                const sizeMb =
                                                    formatFoundrySizeMb(
                                                        catalog?.fileSizeMb,
                                                    )
                                                return (
                                                    <option
                                                        key={model.alias}
                                                        value={model.alias}
                                                    >
                                                        {t(model.labelKey)}
                                                        {sizeMb
                                                            ? ` · ${t("localAsr.foundryApproxSizeMb", { mb: sizeMb })}`
                                                            : ""}
                                                    </option>
                                                )
                                            },
                                        )}
                                    </select>
                                </label>
                                <label
                                    style={{
                                        display: "flex",
                                        flexDirection: "column",
                                        gap: 4,
                                        fontSize: 11,
                                        color: "var(--ol-ink-4)",
                                    }}
                                >
                                    {t("localAsr.foundryRuntimeSourceLabel")}
                                    <select
                                        value={selectedFoundryRuntimeSource}
                                        onChange={(e) => {
                                            const restoreScroll =
                                                preserveEmbeddedScroll(
                                                    e.currentTarget,
                                                )
                                            void handleFoundryRuntimeSourceChange(
                                                e.target
                                                    .value as FoundryRuntimeSource,
                                                restoreScroll,
                                            )
                                        }}
                                        disabled={foundryBusy !== null}
                                        style={{
                                            fontSize: 13,
                                            padding: "6px 10px",
                                            borderRadius: 8,
                                            border: "0.5px solid rgba(0,0,0,0.12)",
                                            background: "var(--ol-surface)",
                                            color: "var(--ol-ink)",
                                            minWidth: 200,
                                        }}
                                    >
                                        <option value="auto">
                                            {t(
                                                "localAsr.foundryRuntimeSourceAuto",
                                            )}
                                        </option>
                                        <option value="nuget">
                                            {t(
                                                "localAsr.foundryRuntimeSourceNuget",
                                            )}
                                        </option>
                                        <option value="ort-nightly">
                                            {t(
                                                "localAsr.foundryRuntimeSourceOrtNightly",
                                            )}
                                        </option>
                                    </select>
                                </label>
                                <label
                                    style={{
                                        display: "flex",
                                        flexDirection: "column",
                                        gap: 4,
                                        fontSize: 11,
                                        color: "var(--ol-ink-4)",
                                    }}
                                >
                                    {t("localAsr.foundryLanguageLabel")}
                                    <select
                                        value={selectedFoundryLanguageHint}
                                        onChange={(e) => {
                                            const restoreScroll =
                                                preserveEmbeddedScroll(
                                                    e.currentTarget,
                                                )
                                            void handleFoundryLanguageChange(
                                                e.target
                                                    .value as FoundryLocalAsrLanguageHint,
                                                restoreScroll,
                                            )
                                        }}
                                        disabled={foundryBusy !== null}
                                        style={{
                                            fontSize: 13,
                                            padding: "6px 10px",
                                            borderRadius: 8,
                                            border: "0.5px solid rgba(0,0,0,0.12)",
                                            background: "var(--ol-surface)",
                                            color: "var(--ol-ink)",
                                            minWidth: 132,
                                        }}
                                    >
                                        <option value="">
                                            {t("localAsr.foundryLanguageAuto")}
                                        </option>
                                        <option value="zh">
                                            {t("localAsr.foundryLanguageZh")}
                                        </option>
                                        <option value="en">
                                            {t("localAsr.foundryLanguageEn")}
                                        </option>
                                    </select>
                                </label>
                            </div>
                        </div>

                        <div
                            style={{
                                fontSize: 12.5,
                                color: "var(--ol-ink-3)",
                                lineHeight: 1.6,
                            }}
                        >
                            <div>
                                <span style={{ color: "var(--ol-ink-4)" }}>
                                    {t("localAsr.foundrySelectedModel")}:{" "}
                                </span>
                                <strong>{selectedFoundryDisplayName}</strong>
                                <span>
                                    {" "}
                                    · {selectedFoundrySizeLabel} ·{" "}
                                    {selectedFoundryDownloadLabel}
                                </span>
                                <span>
                                    {" "}
                                    · {t(selectedFoundryModel.descKey)}
                                </span>
                            </div>
                            <div>
                                <span style={{ color: "var(--ol-ink-4)" }}>
                                    {t("localAsr.foundryRuntimeSourceLabel")}
                                    :{" "}
                                </span>
                                {t(
                                    `localAsr.foundryRuntimeSource${selectedFoundryRuntimeSource === "ort-nightly" ? "OrtNightly" : selectedFoundryRuntimeSource === "nuget" ? "Nuget" : "Auto"}`,
                                )}
                                <span>
                                    {" "}
                                    · {t("localAsr.foundryRuntimeSourceDesc")}
                                </span>
                            </div>
                            <div>
                                <span style={{ color: "var(--ol-ink-4)" }}>
                                    {t("localAsr.foundryLanguageLabel")}:{" "}
                                </span>
                                {selectedFoundryLanguageHint
                                    ? t(
                                          `localAsr.foundryLanguage${selectedFoundryLanguageHint === "zh" ? "Zh" : "En"}`,
                                      )
                                    : t("localAsr.foundryLanguageAuto")}
                                <span>
                                    {" "}
                                    · {t("localAsr.foundryLanguageDesc")}
                                </span>
                            </div>
                            <div>
                                <span style={{ color: "var(--ol-ink-4)" }}>
                                    {t("localAsr.foundryActiveModel")}:{" "}
                                </span>
                                {foundryStatus?.activeModel ?? "whisper-small"}
                            </div>
                            <div>
                                <span style={{ color: "var(--ol-ink-4)" }}>
                                    {t("localAsr.modelDir")}:{" "}
                                </span>
                                <code>
                                    {foundryModelDir?.alias ===
                                    selectedFoundryAlias
                                        ? foundryModelDir.dir
                                        : "—"}
                                </code>
                            </div>
                            <div>
                                <span style={{ color: "var(--ol-ink-4)" }}>
                                    {t("localAsr.foundryLoadedModel")}:{" "}
                                </span>
                                {foundryStatus?.loadedModelId ??
                                    t("localAsr.foundryNotLoaded")}
                            </div>
                            {foundryStatus?.error && (
                                <div style={{ color: "#9b2c2c" }}>
                                    <span>{t("localAsr.foundryError")}: </span>
                                    {foundryStatus.error}
                                </div>
                            )}
                        </div>

                        {(foundryBusy === "prepare" || foundryProgress) && (
                            <FoundryPrepareProgressBlock
                                progress={foundryProgress}
                                modelCached={
                                    selectedFoundryCatalog?.cached === true
                                }
                                cancelRequested={foundryCancelRequested}
                            />
                        )}

                        <div
                            style={{
                                display: "flex",
                                gap: 8,
                                flexWrap: "wrap",
                            }}
                        >
                            <Btn
                                variant="blue"
                                size="sm"
                                disabled={
                                    foundryBusy !== null || !foundryAvailable
                                }
                                onClick={() => void handleEnableFoundry()}
                            >
                                {foundryBusy === "enable"
                                    ? t("localAsr.foundryEnabling")
                                    : t("localAsr.foundrySetDefault")}
                            </Btn>
                            <Btn
                                variant="primary"
                                size="sm"
                                disabled={
                                    foundryBusy !== null || !foundryAvailable
                                }
                                onClick={() => void handlePrepareFoundry()}
                            >
                                {foundryPrepareLabel}
                            </Btn>
                            {foundryBusy === "prepare" && (
                                <Btn
                                    variant="ghost"
                                    size="sm"
                                    disabled={foundryCancelRequested}
                                    onClick={() =>
                                        void handleCancelFoundryPrepare()
                                    }
                                >
                                    {foundryCancelRequested
                                        ? t("localAsr.foundryCancelRequested")
                                        : t("localAsr.foundryCancelPrepare")}
                                </Btn>
                            )}
                            <Btn
                                variant="ghost"
                                size="sm"
                                disabled={
                                    foundryBusy !== null ||
                                    !foundryStatus?.loadedModelId
                                }
                                onClick={() => void handleReleaseFoundry()}
                            >
                                {foundryBusy === "release"
                                    ? t("localAsr.foundryReleasing")
                                    : t("localAsr.releaseNow")}
                            </Btn>
                            <Btn
                                variant="ghost"
                                size="sm"
                                disabled={foundryBusy !== null}
                                onClick={() => void handleRevealFoundryDir()}
                            >
                                {foundryBusy === "reveal"
                                    ? t("common.loading")
                                    : t("localAsr.revealDir")}
                            </Btn>
                            <Btn
                                variant="ghost"
                                size="sm"
                                disabled={foundryBusy !== null}
                                onClick={() => void handleDeleteFoundry()}
                            >
                                {foundryBusy === "delete"
                                    ? t("common.loading")
                                    : t("localAsr.delete")}
                            </Btn>
                        </div>
                    </div>
                </Card>
            )}

            {IS_WINDOWS && (
                <Card style={{ marginBottom: 16 }}>
                    <div
                        ref={sherpaAnchorRef}
                        onMouseDownCapture={activateScrollGuard}
                        onKeyDownCapture={(event) => {
                            if (event.key === "Enter" || event.key === " ") {
                                activateScrollGuard()
                            }
                        }}
                        style={{
                            display: "flex",
                            flexDirection: "column",
                            gap: 14,
                        }}
                    >
                        <div
                            style={{
                                display: "flex",
                                justifyContent: "space-between",
                                gap: 16,
                                flexWrap: "wrap",
                            }}
                        >
                            <div style={{ minWidth: 0, flex: "1 1 360px" }}>
                                <div
                                    style={{
                                        display: "flex",
                                        alignItems: "center",
                                        gap: 8,
                                        marginBottom: 6,
                                        flexWrap: "wrap",
                                    }}
                                >
                                    <div
                                        style={{
                                            fontSize: 14,
                                            fontWeight: 700,
                                            color: "var(--ol-ink)",
                                        }}
                                    >
                                        {t("localAsr.sherpaTitle")}
                                    </div>
                                    {sherpaDefault && (
                                        <Pill tone="blue" size="sm">
                                            {t("localAsr.activeBadge")}
                                        </Pill>
                                    )}
                                    <Pill
                                        tone={
                                            sherpaStatus?.available
                                                ? "ok"
                                                : "outline"
                                        }
                                        size="sm"
                                    >
                                        {sherpaStatus?.available
                                            ? t("localAsr.foundryAvailable")
                                            : t("localAsr.foundryUnavailable")}
                                    </Pill>
                                    <Pill
                                        tone={
                                            sherpaStatus?.runtimeReady
                                                ? "ok"
                                                : "outline"
                                        }
                                        size="sm"
                                    >
                                        {sherpaStatus?.runtimeReady
                                            ? t("localAsr.sherpaRuntimeReady")
                                            : t(
                                                  "localAsr.sherpaRuntimeMissing",
                                              )}
                                    </Pill>
                                </div>
                                <div
                                    style={{
                                        fontSize: 13,
                                        color: "var(--ol-ink-3)",
                                        lineHeight: 1.55,
                                    }}
                                >
                                    {t("localAsr.sherpaDesc")}
                                </div>
                            </div>
                            <div
                                style={{
                                    display: "flex",
                                    gap: 10,
                                    flexWrap: "wrap",
                                    justifyContent: "flex-end",
                                }}
                            >
                                <label
                                    style={{
                                        display: "flex",
                                        flexDirection: "column",
                                        gap: 4,
                                        fontSize: 11,
                                        color: "var(--ol-ink-4)",
                                    }}
                                >
                                    {t("localAsr.foundrySelectedModel")}
                                    <SelectLite
                                        value={selectedSherpaAlias}
                                        onChange={(value) => {
                                            void handleSherpaModelChange(
                                                value as SherpaOnnxModelAlias,
                                            )
                                        }}
                                        disabled={sherpaBusy !== null}
                                        options={sherpaModelOptions}
                                        ariaLabel={t(
                                            "localAsr.foundrySelectedModel",
                                        )}
                                        style={{
                                            fontSize: 13,
                                            height: 31,
                                            padding: "0 10px",
                                            borderRadius: 8,
                                            border: "0.5px solid rgba(0,0,0,0.12)",
                                            background: "var(--ol-surface)",
                                            color: "var(--ol-ink)",
                                            minWidth: 260,
                                        }}
                                    />
                                </label>
                                <label
                                    style={{
                                        display: "flex",
                                        flexDirection: "column",
                                        gap: 4,
                                        fontSize: 11,
                                        color: "var(--ol-ink-4)",
                                    }}
                                >
                                    {t("localAsr.foundryLanguageLabel")}
                                    <SelectLite
                                        value={selectedSherpaLanguageHint}
                                        onChange={(value) => {
                                            activateScrollGuard()
                                            void handleSherpaLanguageChange(
                                                value as SherpaOnnxLanguageHint,
                                            )
                                        }}
                                        disabled={sherpaBusy !== null}
                                        options={sherpaLanguageOptions}
                                        ariaLabel={t(
                                            "localAsr.foundryLanguageLabel",
                                        )}
                                        style={{
                                            fontSize: 13,
                                            height: 31,
                                            padding: "0 10px",
                                            borderRadius: 8,
                                            border: "0.5px solid rgba(0,0,0,0.12)",
                                            background: "var(--ol-surface)",
                                            color: "var(--ol-ink)",
                                            minWidth: 132,
                                        }}
                                    />
                                </label>
                                <label
                                    style={{
                                        display: "flex",
                                        flexDirection: "column",
                                        gap: 4,
                                        fontSize: 11,
                                        color: "var(--ol-ink-4)",
                                    }}
                                >
                                    {t("localAsr.mirrorLabel")}
                                    <select
                                        value={selectedSherpaMirrorValue}
                                        onChange={(e) =>
                                            void handleMirrorChange(
                                                e.target.value,
                                            )
                                        }
                                        disabled={
                                            sherpaBusy !== null ||
                                            selectedSherpaUsesReleaseArchive
                                        }
                                        style={{
                                            fontSize: 13,
                                            height: 31,
                                            padding: "0 10px",
                                            borderRadius: 8,
                                            border: "0.5px solid rgba(0,0,0,0.12)",
                                            background: "var(--ol-surface)",
                                            color: "var(--ol-ink)",
                                            minWidth: 200,
                                        }}
                                    >
                                        {selectedSherpaUsesReleaseArchive ? (
                                            <option value="github-release">
                                                {t(
                                                    "localAsr.mirrorGithubRelease",
                                                )}
                                            </option>
                                        ) : (
                                            <>
                                                <option value="huggingface">
                                                    {t(
                                                        "localAsr.mirrorHuggingface",
                                                    )}
                                                </option>
                                                <option value="hf-mirror">
                                                    {t(
                                                        "localAsr.mirrorHfMirror",
                                                    )}
                                                </option>
                                            </>
                                        )}
                                    </select>
                                </label>
                            </div>
                        </div>

                        <div
                            style={{
                                fontSize: 12.5,
                                color: "var(--ol-ink-3)",
                                lineHeight: 1.6,
                            }}
                        >
                            <div>
                                <span style={{ color: "var(--ol-ink-4)" }}>
                                    {t("localAsr.foundrySelectedModel")}:{" "}
                                </span>
                                <strong>{selectedSherpaDisplayName}</strong>
                                <span>
                                    {" "}
                                    · {selectedSherpaSizeLabel} ·{" "}
                                    {selectedSherpaDownloadLabel}
                                </span>
                                <span> · {t(selectedSherpaModel.descKey)}</span>
                            </div>
                            <div>
                                <span style={{ color: "var(--ol-ink-4)" }}>
                                    {t("localAsr.sherpaModelDir")}:{" "}
                                </span>
                                <code>{sherpaModelDir || "—"}</code>
                            </div>
                            <div>
                                <span style={{ color: "var(--ol-ink-4)" }}>
                                    {t("localAsr.foundryLoadedModel")}:{" "}
                                </span>
                                {sherpaStatus?.loadedModelId ??
                                    t("localAsr.foundryNotLoaded")}
                            </div>
                            {sherpaStatus?.error && (
                                <div style={{ color: "#9b2c2c" }}>
                                    <span>{t("localAsr.sherpaError")}: </span>
                                    {sherpaStatus.error}
                                </div>
                            )}
                        </div>

                        {(sherpaBusy === "prepare" || sherpaProgress) && (
                            <FoundryPrepareProgressBlock
                                progress={sherpaProgress}
                                modelCached={
                                    selectedSherpaCatalog?.cached === true
                                }
                                cancelRequested={sherpaCancelRequested}
                            />
                        )}

                        {showSherpaDownloadProgress && (
                            <DownloadProgressBlock
                                progress={selectedSherpaDownloadProgressForDisplay}
                                remoteSize={selectedSherpaRemoteSize}
                                cancelRequested={sherpaDownloadCancelRequested}
                            />
                        )}

                        <div
                            style={{
                                display: "flex",
                                gap: 8,
                                flexWrap: "wrap",
                            }}
                        >
                            <Btn
                                variant="blue"
                                size="sm"
                                disabled={
                                    sherpaBusy !== null || !sherpaAvailable
                                }
                                onClick={() => void handleEnableSherpa()}
                            >
                                {sherpaBusy === "enable"
                                    ? t("localAsr.foundryEnabling")
                                    : t("localAsr.sherpaSetDefault")}
                            </Btn>
                            <Btn
                                variant="primary"
                                size="sm"
                                disabled={
                                    sherpaBusy !== null ||
                                    !sherpaAvailable
                                }
                                onClick={() => void handlePrepareSherpa()}
                            >
                                {sherpaPrepareLabel}
                            </Btn>
                            {selectedSherpaCatalog?.cached !== true &&
                                !isSherpaDownloading && (
                                    <Btn
                                        variant="primary"
                                        size="sm"
                                        disabled={
                                            sherpaBusy !== null ||
                                            !sherpaAvailable
                                        }
                                        onClick={() =>
                                            void handleDownloadSherpa()
                                        }
                                    >
                                        {hasSherpaPartial
                                            ? t("localAsr.resume")
                                            : t("localAsr.download")}
                                    </Btn>
                                )}
                            {isSherpaDownloading && (
                                <Btn
                                    variant="ghost"
                                    size="sm"
                                    disabled={sherpaDownloadCancelRequested}
                                    onClick={() =>
                                        void handleCancelSherpaDownload()
                                    }
                                >
                                    {sherpaDownloadCancelRequested
                                        ? t("localAsr.foundryCancelRequested")
                                        : t("localAsr.cancel")}
                                </Btn>
                            )}
                            {sherpaBusy === "prepare" && (
                                <Btn
                                    variant="ghost"
                                    size="sm"
                                    disabled={sherpaCancelRequested}
                                    onClick={() =>
                                        void handleCancelSherpaPrepare()
                                    }
                                >
                                    {sherpaCancelRequested
                                        ? t("localAsr.foundryCancelRequested")
                                        : t("localAsr.foundryCancelPrepare")}
                                </Btn>
                            )}
                            <Btn
                                variant="ghost"
                                size="sm"
                                disabled={
                                    sherpaBusy !== null ||
                                    !sherpaStatus?.loadedModelId
                                }
                                onClick={() => void handleReleaseSherpa()}
                            >
                                {sherpaBusy === "release"
                                    ? t("localAsr.foundryReleasing")
                                    : t("localAsr.releaseNow")}
                            </Btn>
                            <Btn
                                variant="ghost"
                                size="sm"
                                disabled={sherpaBusy !== null}
                                onClick={() => void handleRevealSherpaDir()}
                            >
                                {sherpaBusy === "reveal"
                                    ? t("common.loading")
                                    : t("localAsr.sherpaRevealDir")}
                            </Btn>
                            <Btn
                                variant="ghost"
                                size="sm"
                                disabled={
                                    sherpaBusy !== null ||
                                    !canDeleteSelectedSherpa
                                }
                                onClick={() => void handleDeleteSherpa()}
                            >
                                {sherpaBusy === "delete"
                                    ? t("common.loading")
                                    : t("localAsr.delete")}
                            </Btn>
                        </div>
                    </div>
                </Card>
            )}

            {/* Qwen3 模型管理区——只在 macOS 渲染（后端 #[cfg(target_os = "macos")] 独占）。
          Windows / Linux 看见镜像源 / 下载 / 模型列表都是 dead UI。Foundry 块自身已经
          被上方 IS_WINDOWS 守卫，错误 Card（共享 setError，被 Foundry handler 也写）
          保持无条件露出。 */}
            {IS_MAC && !engineAvailable && (
                <Card
                    style={{
                        marginBottom: 16,
                        background: "rgba(255, 235, 200, 0.4)",
                    }}
                >
                    <div
                        style={{
                            fontSize: 13,
                            color: "var(--ol-ink-2)",
                        }}
                    >
                        {t("localAsr.engineUnavailable")}
                    </div>
                </Card>
            )}

            {error && (
                <Card
                    style={{
                        marginBottom: 16,
                        background: "rgba(255, 220, 220, 0.5)",
                    }}
                >
                    <div style={{ fontSize: 13, color: "#9b2c2c" }}>
                        {error}
                    </div>
                </Card>
            )}
        </Wrapper>
    )
}

// Presentational sub-components (FoundryPrepareProgressBlock, DownloadProgressBlock,
// ModelRow, TestResultBlock) live in ./components — imported at the top of this file.

// Pure UI helpers (alias/language-hint guards, platform detection, size
// formatting) live in ./helpers — imported at the top of this file.
