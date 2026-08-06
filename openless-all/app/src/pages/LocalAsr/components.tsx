// Presentational sub-components for the Local ASR page, extracted from
// LocalAsr/index.tsx (behavior-preserving move). All are props-driven and
// stateless beyond local render memoization.

import { useMemo } from "react"
import { createPortal } from "react-dom"
import { useTranslation } from "react-i18next"
import {
    type FoundryPrepareProgress,
    type HfModelCard,
    type LocalAsrDownloadProgress,
    type LocalAsrModelStatus,
    type LocalAsrTestResult,
} from "../../lib/localAsr"
import { Btn, Card, Pill } from "../_atoms"
import { Icon } from "../../components/Icon"
import { formatBytes } from "./helpers"
import type { RemoteSize } from "./types"

export function FoundryPrepareProgressBlock({
    progress,
    modelCached,
    cancelRequested,
}: {
    progress: FoundryPrepareProgress | null
    modelCached: boolean
    cancelRequested: boolean
}) {
    const { t } = useTranslation()
    const stages = [
        { phase: "runtime", label: t("localAsr.foundryPrepareRuntime") },
        { phase: "model", label: t("localAsr.foundryPrepareModel") },
        { phase: "load", label: t("localAsr.foundryPrepareLoad") },
    ] as const
    const currentIndex = progress
        ? stages.findIndex((stage) => stage.phase === progress.phase)
        : -1

    return (
        <div
            style={{
                padding: "10px 12px",
                borderRadius: 8,
                background: "rgba(0,0,0,0.035)",
                display: "flex",
                flexDirection: "column",
                gap: 9,
            }}
        >
            {stages.map((stage, index) => {
                const finished =
                    progress?.phase === "finished" || currentIndex > index
                const skippedCachedModel =
                    stage.phase === "model" &&
                    modelCached &&
                    (progress?.phase === "load" ||
                        progress?.phase === "finished")
                const active = progress?.phase === stage.phase
                const failed = progress?.phase === "failed"
                const percent =
                    finished || skippedCachedModel
                        ? 100
                        : active
                          ? Math.max(0, Math.min(100, progress?.percent ?? 0))
                          : 0
                const detail = skippedCachedModel
                    ? t("localAsr.foundryPrepareModelSkipped")
                    : active
                      ? progress?.label
                      : finished
                        ? t("localAsr.foundryPrepareDone")
                        : t("localAsr.foundryPrepareWaiting")
                return (
                    <div key={stage.phase}>
                        <div
                            style={{
                                display: "flex",
                                justifyContent: "space-between",
                                gap: 12,
                                marginBottom: 5,
                            }}
                        >
                            <span
                                style={{
                                    fontSize: 12,
                                    color: "var(--ol-ink-2)",
                                    fontWeight: 600,
                                }}
                            >
                                {stage.label}
                            </span>
                            <span
                                style={{
                                    fontSize: 11,
                                    color: "var(--ol-ink-4)",
                                }}
                            >
                                {failed
                                    ? t("localAsr.failed")
                                    : `${Math.round(percent)}%`}
                            </span>
                        </div>
                        <div
                            style={{
                                height: 6,
                                borderRadius: 3,
                                overflow: "hidden",
                                background: "rgba(0,0,0,0.08)",
                            }}
                        >
                            <div
                                style={{
                                    height: "100%",
                                    width: `${percent}%`,
                                    background: failed
                                        ? "#d04545"
                                        : "var(--ol-accent-blue, #2c5cff)",
                                    transition: "width 120ms linear",
                                }}
                            />
                        </div>
                        <div
                            style={{
                                fontSize: 11,
                                color: "var(--ol-ink-4)",
                                marginTop: 4,
                            }}
                        >
                            {detail}
                        </div>
                    </div>
                )
            })}
            {cancelRequested && (
                <div
                    style={{
                        fontSize: 11.5,
                        color: "#8a5a00",
                        lineHeight: 1.5,
                    }}
                >
                    {t("localAsr.foundryCancelBestEffort")}
                </div>
            )}
            {progress?.phase === "failed" && progress.error && (
                <div
                    style={{
                        fontSize: 11.5,
                        color: "#9b2c2c",
                        lineHeight: 1.5,
                    }}
                >
                    {progress.error}
                </div>
            )}
        </div>
    )
}

export function DownloadProgressBlock({
    progress,
    remoteSize,
    cancelRequested,
}: {
    progress?: LocalAsrDownloadProgress
    remoteSize?: RemoteSize
    cancelRequested: boolean
}) {
    const { t } = useTranslation()
    const downloadedBytes = progress?.bytesDownloaded ?? 0
    const totalBytes = progress?.bytesTotal ?? remoteSize?.totalBytes ?? 0
    const ratio = totalBytes > 0 ? Math.min(1, downloadedBytes / totalBytes) : 0
    const failed = progress?.phase === "failed"
    return (
        <div
            style={{
                padding: "10px 12px",
                borderRadius: 8,
                background: "rgba(0,0,0,0.035)",
                display: "flex",
                flexDirection: "column",
                gap: 8,
            }}
        >
            <div
                style={{
                    display: "flex",
                    justifyContent: "space-between",
                    gap: 12,
                }}
            >
                <span
                    style={{
                        fontSize: 12,
                        color: "var(--ol-ink-2)",
                        fontWeight: 600,
                    }}
                >
                    {t("localAsr.foundryPrepareModel")}
                </span>
                <span style={{ fontSize: 11, color: "var(--ol-ink-4)" }}>
                    {failed
                        ? t("localAsr.failed")
                        : `${Math.round(ratio * 100)}%`}
                </span>
            </div>
            <div
                style={{
                    height: 6,
                    borderRadius: 3,
                    overflow: "hidden",
                    background: "rgba(0,0,0,0.08)",
                }}
            >
                <div
                    style={{
                        height: "100%",
                        width: `${ratio * 100}%`,
                        background: failed
                            ? "#d04545"
                            : "var(--ol-accent-blue, #2c5cff)",
                        transition: "width 120ms linear",
                    }}
                />
            </div>
            <div style={{ fontSize: 11, color: "var(--ol-ink-4)" }}>
                {failed
                    ? `${t("localAsr.failed")}: ${progress?.error ?? ""}`
                    : `${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)}` +
                      (progress?.file ? ` · ${progress.file}` : "")}
            </div>
            {cancelRequested && (
                <div
                    style={{
                        fontSize: 11.5,
                        color: "#8a5a00",
                        lineHeight: 1.5,
                    }}
                >
                    {t("localAsr.foundryCancelRequested")}
                </div>
            )}
        </div>
    )
}

export interface ModelRowProps {
    model: LocalAsrModelStatus
    modelDir: string
    remoteSize?: RemoteSize
    progress?: LocalAsrDownloadProgress
    isActive: boolean
    engineAvailable: boolean
    disabled: boolean
    testing: boolean
    testResult?: LocalAsrTestResult | { error: string }
    onDownload: () => void
    onCancel: () => void
    onDelete: () => void
    onReveal: () => void
    onSetActive: () => void
    onTest: () => void
}

export function ModelRow({
    model,
    modelDir,
    remoteSize,
    progress,
    isActive,
    engineAvailable,
    disabled,
    testing,
    testResult,
    onDownload,
    onCancel,
    onDelete,
    onReveal,
    onSetActive,
    onTest,
}: ModelRowProps) {
    const { t } = useTranslation()
    const isDownloading = useMemo(
        () => progress?.phase === "started" || progress?.phase === "progress",
        [progress?.phase],
    )
    const downloadedBytes = progress?.bytesDownloaded ?? model.downloadedBytes
    const totalBytes = progress?.bytesTotal ?? remoteSize?.totalBytes ?? 0
    const ratio = totalBytes > 0 ? Math.min(1, downloadedBytes / totalBytes) : 0
    // 进度条要保留：有 partial 残留（downloadedBytes>0 但未完整）就一直显示，
    // 让用户看到上次下到哪里了，再点下载会从那里续。
    const hasPartial = !model.isDownloaded && model.downloadedBytes > 0
    const showProgress =
        isDownloading || progress?.phase === "failed" || hasPartial

    const sizeLabel = remoteSize?.loading
        ? t("localAsr.sizeLoading")
        : remoteSize?.error
          ? t("localAsr.sizeUnknown")
          : remoteSize && remoteSize.totalBytes > 0
            ? `${formatBytes(remoteSize.totalBytes)} · ${remoteSize.fileCount} ${t("localAsr.files")}`
            : t("localAsr.sizeUnknown")

    return (
        <Card>
            <div
                style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 16,
                }}
            >
                <div style={{ minWidth: 0 }}>
                    <div
                        style={{
                            display: "flex",
                            alignItems: "center",
                            gap: 8,
                            marginBottom: 4,
                        }}
                    >
                        <div
                            style={{
                                fontSize: 14,
                                fontWeight: 600,
                                color: "var(--ol-ink)",
                            }}
                        >
                            {model.id}
                        </div>
                        {isActive && (
                            <Pill tone="blue" size="sm">
                                {t("localAsr.activeBadge")}
                            </Pill>
                        )}
                        {model.isDownloaded && (
                            <Pill tone="ok" size="sm">
                                {t("localAsr.downloadedBadge")}
                            </Pill>
                        )}
                    </div>
                    <div style={{ fontSize: 12, color: "var(--ol-ink-3)" }}>
                        {model.hfRepo} · {sizeLabel}
                    </div>
                    <div
                        style={{
                            fontSize: 11,
                            color: "var(--ol-ink-4)",
                            marginTop: 4,
                            wordBreak: "break-all",
                        }}
                    >
                        {t("localAsr.modelDir")}:{" "}
                        <code>{modelDir || "—"}</code>
                    </div>
                    {showProgress && (
                        <div style={{ marginTop: 10, maxWidth: 420 }}>
                            <div
                                style={{
                                    height: 6,
                                    borderRadius: 3,
                                    background: "rgba(0,0,0,0.06)",
                                    overflow: "hidden",
                                }}
                            >
                                <div
                                    style={{
                                        width: `${ratio * 100}%`,
                                        height: "100%",
                                        background:
                                            progress?.phase === "failed"
                                                ? "#d04545"
                                                : "var(--ol-accent-blue, #2c5cff)",
                                        transition: "width 120ms linear",
                                    }}
                                />
                            </div>
                            <div
                                style={{
                                    fontSize: 11,
                                    color: "var(--ol-ink-4)",
                                    marginTop: 6,
                                }}
                            >
                                {progress?.phase === "failed"
                                    ? `${t("localAsr.failed")}: ${progress.error ?? ""}`
                                    : `${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)}` +
                                      (progress?.file
                                          ? ` · ${progress.file}`
                                          : "")}
                            </div>
                        </div>
                    )}
                </div>
                <div
                    style={{
                        display: "flex",
                        gap: 8,
                        flexShrink: 0,
                        flexWrap: "wrap",
                        justifyContent: "flex-end",
                        maxWidth: 360,
                    }}
                >
                    {model.isDownloaded ? (
                        <>
                            {!isActive && (
                                <Btn
                                    variant="blue"
                                    size="sm"
                                    disabled={disabled || !engineAvailable}
                                    onClick={onSetActive}
                                >
                                    {t("localAsr.setActive")}
                                </Btn>
                            )}
                            <Btn
                                variant="primary"
                                size="sm"
                                disabled={
                                    disabled || testing || !engineAvailable
                                }
                                onClick={onTest}
                            >
                                {testing
                                    ? t("localAsr.testRunning")
                                    : t("localAsr.test")}
                            </Btn>
                            <Btn
                                variant="ghost"
                                size="sm"
                                disabled={disabled || testing}
                                onClick={onDelete}
                            >
                                {t("localAsr.delete")}
                            </Btn>
                            <Btn
                                variant="ghost"
                                size="sm"
                                disabled={disabled}
                                onClick={onReveal}
                            >
                                {t("localAsr.revealDir")}
                            </Btn>
                        </>
                    ) : isDownloading ? (
                        <Btn variant="ghost" size="sm" onClick={onCancel}>
                            {t("localAsr.cancel")}
                        </Btn>
                    ) : (
                        <>
                            <Btn
                                variant="primary"
                                size="sm"
                                disabled={disabled || !engineAvailable}
                                onClick={onDownload}
                            >
                                {hasPartial
                                    ? t("localAsr.resume")
                                    : t("localAsr.download")}
                            </Btn>
                            {hasPartial && (
                                <Btn
                                    variant="ghost"
                                    size="sm"
                                    disabled={disabled}
                                    onClick={onDelete}
                                >
                                    {t("localAsr.delete")}
                                </Btn>
                            )}
                            <Btn
                                variant="ghost"
                                size="sm"
                                disabled={disabled}
                                onClick={onReveal}
                            >
                                {t("localAsr.revealDir")}
                            </Btn>
                        </>
                    )}
                </div>
            </div>
            {testResult && <TestResultBlock result={testResult} />}
        </Card>
    )
}

export function TestResultBlock({
    result,
}: {
    result: LocalAsrTestResult | { error: string }
}) {
    const { t } = useTranslation()
    const hasError = "error" in result
    return (
        <div
            style={{
                marginTop: 12,
                padding: "10px 12px",
                background: hasError
                    ? "rgba(255, 220, 220, 0.5)"
                    : "rgba(0, 0, 0, 0.04)",
                borderRadius: 8,
                fontSize: 12.5,
                color: hasError ? "#9b2c2c" : "var(--ol-ink-2)",
                lineHeight: 1.6,
            }}
        >
            {hasError ? (
                <div>
                    <strong>{t("localAsr.testFailed")}: </strong>
                    {result.error}
                </div>
            ) : (
                <div
                    style={{ display: "flex", flexDirection: "column", gap: 4 }}
                >
                    <div
                        style={{
                            fontSize: 11,
                            color: "var(--ol-ink-4)",
                            letterSpacing: ".04em",
                            textTransform: "uppercase",
                        }}
                    >
                        {t("localAsr.testHeading")}
                    </div>
                    <div>
                        <span style={{ color: "var(--ol-ink-4)" }}>
                            {t("localAsr.testExpected")}:{" "}
                        </span>
                        {result.expectedText}
                    </div>
                    <div>
                        <span style={{ color: "var(--ol-ink-4)" }}>
                            {t("localAsr.testActual")}:{" "}
                        </span>
                        <strong>{result.transcribedText || "(空)"}</strong>
                    </div>
                    <div style={{ fontSize: 11, color: "var(--ol-ink-4)" }}>
                        {t("localAsr.testStats", {
                            audio: (result.audioMs / 1000).toFixed(1),
                            load: (result.loadMs / 1000).toFixed(1),
                            transcribe: (result.transcribeMs / 1000).toFixed(1),
                            backend: result.backend,
                        })}
                    </div>
                </div>
            )}
        </div>
    )
}

// ─────────────────────────────────────────────────────────────────────
// 本地 ASR 模型管理重构（两栏看板 + 下载弹框 + 右上角下载进度浮层）。
// 纯展示组件；数据与动作由 LocalAsr/index.tsx 组装后传入。
// ─────────────────────────────────────────────────────────────────────

/** 侧栏统一条目：三套本地引擎（Qwen3 / sherpa-onnx / foundry）归一化。 */
export interface SidebarModelEntry {
    id: string
    /** 展示名（如 qwen3-asr-0.6b / whisper-small）。 */
    name: string
    /** HF 仓库标识（Qwen3 有；sherpa/foundry 可能为空）。 */
    repo?: string
    /** 已下载字节数（HF 拉取的真实尺寸）。 */
    remoteBytes?: number
    /** 已下载（有绿勾）。 */
    isDownloaded: boolean
    /** 下载中（有进度条/取消入口）。 */
    isDownloading: boolean
    /** 下载中实时百分比（0-100；仅 isDownloading 时有值）。 */
    percent?: number | null
    /** 当前激活（设为默认的本地模型）。 */
    isActive: boolean
    /** 引擎标识，决定右侧动作按钮分派。 */
    engine: "qwen3" | "sherpa" | "foundry"
}

/** 左侧模型选择栏：竖排条目，选中高亮；底部预留「下载新模型」按钮位。 */
export function ModelSidebar({
    entries,
    selectedId,
    onSelect,
    onOpenDownload,
    downloadDisabled,
}: {
    entries: SidebarModelEntry[]
    selectedId: string | null
    onSelect: (id: string) => void
    onOpenDownload: () => void
    downloadDisabled: boolean
}) {
    const { t } = useTranslation()
    return (
        <div
            style={{
                display: "flex",
                flexDirection: "column",
                gap: 6,
                minWidth: 0,
            }}
        >
            <div
                style={{
                    fontSize: 12,
                    fontWeight: 600,
                    color: "var(--ol-ink-4)",
                    marginBottom: 2,
                }}
            >
                {t("localAsr.sidebarTitle")}
            </div>
            {entries.map((entry) => {
                const selected = entry.id === selectedId
                return (
                    <button
                        key={entry.id}
                        type="button"
                        onClick={() => onSelect(entry.id)}
                        style={{
                            display: "flex",
                            alignItems: "center",
                            gap: 8,
                            // 行距加大：列表可容纳约 4 个模型，竖排更长、横向不变。
                            padding: "11px 14px",
                            borderRadius: 8,
                            border: "0.5px solid var(--ol-line-soft)",
                            background: selected
                                ? "var(--ol-segmented-active-bg)"
                                : "transparent",
                            boxShadow: selected
                                ? "var(--ol-segmented-active-shadow)"
                                : "none",
                            color: "var(--ol-ink)",
                            fontFamily: "inherit",
                            fontSize: 13,
                            textAlign: "left",
                            cursor: "pointer",
                            transition:
                                "background 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft)",
                            width: "100%",
                        }}
                    >
                        {/* 状态徽标：已下载绿勾 / 下载中 spinner / 未下载空心点 */}
                        <span
                            style={{
                                display: "inline-flex",
                                width: 16,
                                height: 16,
                                alignItems: "center",
                                justifyContent: "center",
                                flexShrink: 0,
                                color: entry.isDownloaded
                                    ? "var(--ol-ok)"
                                    : "var(--ol-ink-4)",
                            }}
                        >
                            {entry.isDownloaded ? (
                                <IconCheck />
                            ) : entry.isDownloading ? (
                                <span
                                    style={{
                                        width: 10,
                                        height: 10,
                                        borderRadius: 999,
                                        border: "1.5px solid var(--ol-ink-4)",
                                        borderTopColor: "transparent",
                                        animation: "ol-spin 0.8s linear infinite",
                                        display: "inline-block",
                                    }}
                                />
                            ) : (
                                <span
                                    style={{
                                        width: 6,
                                        height: 6,
                                        borderRadius: 999,
                                        background: "var(--ol-ink-4)",
                                        opacity: 0.5,
                                        display: "inline-block",
                                    }}
                                />
                            )}
                        </span>
                        <span
                            style={{
                                flex: 1,
                                minWidth: 0,
                                overflow: "hidden",
                                textOverflow: "ellipsis",
                                whiteSpace: "nowrap",
                            }}
                        >
                            {entry.name}
                        </span>
                        {entry.isActive && (
                            <span
                                style={{
                                    fontSize: 10,
                                    color: "var(--ol-blue)",
                                    flexShrink: 0,
                                }}
                            >
                                {t("localAsr.activePill")}
                            </span>
                        )}
                        {entry.percent != null && entry.percent >= 0 ? (
                            <span
                                style={{
                                    fontSize: 10.5,
                                    color: "var(--ol-ink-4)",
                                    flexShrink: 0,
                                }}
                            >
                                {Math.round(entry.percent)}%
                            </span>
                        ) : entry.remoteBytes != null && entry.remoteBytes > 0 ? (
                            <span
                                style={{
                                    fontSize: 10.5,
                                    color: "var(--ol-ink-4)",
                                    flexShrink: 0,
                                }}
                            >
                                {formatBytes(entry.remoteBytes)}
                            </span>
                        ) : null}
                    </button>
                )
            })}
            {entries.length === 0 && (
                <div style={{ fontSize: 12, color: "var(--ol-ink-4)", padding: "4px 2px" }}>
                    {t("localAsr.modelSelectEmpty")}
                </div>
            )}
            <button
                type="button"
                onClick={onOpenDownload}
                disabled={downloadDisabled}
                style={{
                    marginTop: 4,
                    padding: "11px 10px",
                    borderRadius: 8,
                    border: "1px dashed var(--ol-line-strong)",
                    background: "transparent",
                    color: "var(--ol-blue)",
                    fontFamily: "inherit",
                    fontSize: 12.5,
                    fontWeight: 600,
                    cursor: downloadDisabled ? "not-allowed" : "pointer",
                    opacity: downloadDisabled ? 0.5 : 1,
                    transition:
                        "background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)",
                }}
            >
                ＋ {t("localAsr.downloadNewModel")}
            </button>
        </div>
    )
}

function IconCheck() {
    return (
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <path
                d="M3 8.5L6.2 11.7L13 4.5"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
            />
        </svg>
    )
}

/** 下载量/收藏数展示：千分位分隔（12345 → "12,345"）。 */
function formatCount(n: number): string {
    return n.toLocaleString("en-US")
}

/** 右侧详情看板：选中模型的信息（HF 抓取的尺寸/文件数）+ 操作按钮。 */
export function ModelDetailPanel({
    entry,
    fileCount,
    mirrorLabel,
    downloading,
    progressPercent,
    busy,
    onDownload,
    onCancel,
    onDelete,
    onReveal,
    onTest,
    showTest,
    testResult,
    testing,
}: {
    entry: SidebarModelEntry | null
    fileCount: number | null
    mirrorLabel?: string
    downloading: boolean
    progressPercent: number | null
    busy: boolean
    onDownload: () => void
    onCancel: () => void
    onDelete: () => void
    onReveal: () => void
    onTest: () => void
    showTest: boolean
    testResult: LocalAsrTestResult | { error: string } | null
    testing: boolean
}) {
    const { t } = useTranslation()
    if (!entry) {
        return (
            <div
                style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    minHeight: 120,
                    fontSize: 12.5,
                    color: "var(--ol-ink-4)",
                }}
            >
                {t("localAsr.detailEmpty")}
            </div>
        )
    }
    return (
        <div
            style={{
                display: "flex",
                flexDirection: "column",
                minWidth: 0,
                height: "100%",
            }}
        >
            {/* 顶部行：模型名在左上，Hugging Face 仓库 / 镜像源在右上。 */}
            <div
                style={{
                    display: "flex",
                    alignItems: "flex-start",
                    justifyContent: "space-between",
                    gap: 12,
                }}
            >
                <div style={{ minWidth: 0 }}>
                    <div style={{ fontSize: 14, fontWeight: 650, color: "var(--ol-ink)" }}>
                        {entry.name}
                    </div>
                    <div
                        style={{
                            display: "flex",
                            flexWrap: "wrap",
                            gap: 6,
                            marginTop: 8,
                            fontSize: 11,
                        }}
                    >
                        {entry.remoteBytes != null && entry.remoteBytes > 0 && (
                            <span
                                style={{
                                    padding: "2px 8px",
                                    borderRadius: 999,
                                    background: "rgba(0,0,0,0.05)",
                                    color: "var(--ol-ink-2)",
                                }}
                            >
                                {formatBytes(entry.remoteBytes)}
                            </span>
                        )}
                        {fileCount != null && fileCount > 0 && (
                            <span
                                style={{
                                    padding: "2px 8px",
                                    borderRadius: 999,
                                    background: "rgba(0,0,0,0.05)",
                                    color: "var(--ol-ink-2)",
                                }}
                            >
                                {fileCount} {t("localAsr.detailFiles")}
                            </span>
                        )}
                        {entry.isDownloaded && (
                            <span
                                style={{
                                    padding: "2px 8px",
                                    borderRadius: 999,
                                    background: "rgba(40,160,90,0.12)",
                                    color: "var(--ol-ok)",
                                }}
                            >
                                ✓ {t("localAsr.detailDownloaded")}
                            </span>
                        )}
                    </div>
                </div>
                <div
                    style={{
                        display: "flex",
                        flexDirection: "column",
                        alignItems: "flex-end",
                        gap: 6,
                        flexShrink: 0,
                        minWidth: 0,
                    }}
                >
                    {entry.repo && (
                        <span
                            title={entry.repo}
                            style={{
                                maxWidth: 220,
                                overflow: "hidden",
                                textOverflow: "ellipsis",
                                whiteSpace: "nowrap",
                                padding: "2px 8px",
                                borderRadius: 999,
                                background: "rgba(0,0,0,0.05)",
                                color: "var(--ol-ink-4)",
                                fontSize: 10.5,
                            }}
                        >
                            {entry.repo}
                        </span>
                    )}
                    {mirrorLabel && (
                        <span
                            style={{
                                padding: "2px 8px",
                                borderRadius: 999,
                                background: "rgba(0,0,0,0.05)",
                                color: "var(--ol-ink-4)",
                                fontSize: 10.5,
                            }}
                        >
                            {mirrorLabel}
                        </span>
                    )}
                </div>
            </div>

            {downloading && progressPercent != null && (
                <div style={{ marginTop: 12 }}>
                    <div style={{ height: 6, borderRadius: 999, background: "var(--ol-surface-2)", overflow: "hidden" }}>
                        <div
                            style={{
                                height: "100%",
                                width: `${progressPercent}%`,
                                background: "var(--ol-blue)",
                                transition: "width 0.18s var(--ol-motion-soft)",
                            }}
                        />
                    </div>
                    <div style={{ fontSize: 11, color: "var(--ol-ink-4)", marginTop: 4 }}>
                        {Math.round(progressPercent)}%
                    </div>
                </div>
            )}

            {testResult && <TestResultBlock result={testResult} />}

            {/* 底部操作行：下载 / 加载并测试 / 打开目录 / 删除，全部并排。
                「加载并测试」加载即作为当前模型使用（激活 = 在 ASR 语音转写里
                选本地模型供应商，不再单独设「设为默认」）。 */}
            <div
                style={{
                    marginTop: "auto",
                    display: "flex",
                    flexWrap: "wrap",
                    gap: 6,
                    borderTop: "0.5px solid var(--ol-line)",
                    paddingTop: 12,
                }}
            >
                {!entry.isDownloaded && (
                    <Btn variant="primary" size="sm" disabled={busy} onClick={onDownload}>
                        {downloading ? t("localAsr.downloading") : t("localAsr.download")}
                    </Btn>
                )}
                {downloading && (
                    <Btn variant="ghost" size="sm" onClick={onCancel}>
                        {t("common.cancel")}
                    </Btn>
                )}
                {entry.isDownloaded && showTest && (
                    <Btn variant="primary" size="sm" disabled={busy || testing} onClick={onTest}>
                        {testing ? t("localAsr.testRunning") : t("localAsr.test")}
                    </Btn>
                )}
                {entry.isDownloaded && (
                    <>
                        <Btn variant="ghost" size="sm" onClick={onReveal}>
                            {t("localAsr.revealDir")}
                        </Btn>
                        <Btn variant="ghost" size="sm" onClick={onDelete}>
                            {t("localAsr.delete")}
                        </Btn>
                    </>
                )}
            </div>
        </div>
    )
}

/** 下载弹框：全页式（与设置弹窗同尺寸、顶部锚定），左侧模型选择（竖排，
 *  已下载标勾）+ 右侧详情，右上角 ✕ 关闭，底部开始下载。
 *
 *  必须 createPortal 到 document.body：WindowChrome 根节点带常驻 transform /
 *  will-change（ol-window-enter 动画 fill-mode: both 保留终帧 transform），
 *  会创建 containing block —— 直接渲染的话 `position: fixed` 会相对设置弹窗
 *  而不是视口定位，遮罩只盖住设置面板（内容发灰）、弹框被裁掉一半且点不到
 *  （与 Modal.tsx 的 GitHub 登录弹窗同一 bug 根因）。portal 出去后 fixed
 *  相对视口，铺满整窗、始终置顶。 */
export function DownloadDialog({
    entries,
    selectedId,
    onSelect,
    sizeOf,
    fileCountOf,
    hfCardOf,
    busy,
    onStart,
    onClose,
}: {
    entries: SidebarModelEntry[]
    selectedId: string | null
    onSelect: (id: string) => void
    sizeOf: (id: string) => number | null
    fileCountOf: (id: string) => number | null
    hfCardOf: (id: string) =>
        | { status: "loading" }
        | { status: "error"; message: string }
        | { status: "ok"; card: HfModelCard }
        | null
    busy: boolean
    onStart: () => void
    onClose: () => void
}) {
    const { t } = useTranslation()
    // 默认选中第一项：看板可能什么都没选中（零下载用户），弹窗不能停在
    // 「未选择」空态——高亮、右侧详情与「开始下载」都跟随该解析值。
    const resolvedId = entries.some((e) => e.id === selectedId)
        ? selectedId
        : (entries[0]?.id ?? null)
    const selected = entries.find((e) => e.id === resolvedId) ?? null
    const hfCard = selected ? hfCardOf(selected.id) : null
    return createPortal(
        <div
            role="dialog"
            aria-modal="true"
            style={{
                position: "fixed",
                inset: 0,
                background: "var(--ol-overlay-bg)",
                // 与设置弹窗完全同尺寸同位置（880×600 垂直居中）：弹层盖在设置
                // 窗口正上方，不会错位、不会比设置窗更高。
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                zIndex: 1000,
                padding: 28,
                // 无入场动画：WKWebView 上遮罩/卡片的合成层动画（opacity/
                // transform）叠加在弹窗打开瞬间的 setState 重渲染上，会被
                // 反复重栅格化——用户感知为「弹窗闪一下」。淡入只有 0.2s，
                // 收益为零，去掉最稳（#928 实测后回退）。
            }}
            onClick={(e) => {
                // busy = 真实下载中（index 传 anyDownloadInFlight）：下载中点击
                // 遮罩不关闭——否则用户点遮罩下的设置项时弹窗会「像按了叉一样
                // 消失」，误以为是设置页闪退。只能走右上角 ✕ 关闭。
                if (e.target === e.currentTarget && !busy) onClose()
            }}
        >
            <div
                style={{
                    width: "min(880px, 100%)",
                    height: "min(600px, calc(100vh - 56px))",
                    display: "flex",
                    flexDirection: "column",
                    borderRadius: 14,
                    background: "var(--ol-surface)",
                    border: "0.5px solid var(--ol-line-strong)",
                    boxShadow: "var(--ol-shadow-xl)",
                    overflow: "hidden",
                    // 无入场动画，见上方遮罩注释：动画重放是「弹窗闪一下 /
                    // 上下动」的 WKWebView 合成层根源，去掉后纯静态出现。
                }}
            >
                {/* 标题行：左标题 + 右 ✕ 关闭 */}
                <div
                    style={{
                        padding: "13px 16px",
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 12,
                        borderBottom: "0.5px solid var(--ol-line)",
                    }}
                >
                    <div
                        style={{
                            fontSize: 14,
                            fontWeight: 650,
                            color: "var(--ol-ink)",
                            letterSpacing: "-0.01em",
                        }}
                    >
                        {t("localAsr.downloadDialogTitle")}
                    </div>
                    <button
                        type="button"
                        onClick={onClose}
                        disabled={busy}
                        aria-label={t("common.close")}
                        style={{
                            display: "inline-flex",
                            alignItems: "center",
                            justifyContent: "center",
                            width: 26,
                            height: 26,
                            borderRadius: 7,
                            border: 0,
                            padding: 0,
                            background: "transparent",
                            color: "var(--ol-ink-3)",
                            cursor: busy ? "not-allowed" : "pointer",
                            opacity: busy ? 0.5 : 1,
                            transition:
                                "background 0.12s var(--ol-motion-quick), color 0.12s var(--ol-motion-quick)",
                        }}
                        onMouseEnter={(e) => {
                            e.currentTarget.style.background = "var(--ol-surface-2)"
                            e.currentTarget.style.color = "var(--ol-ink)"
                        }}
                        onMouseLeave={(e) => {
                            e.currentTarget.style.background = "transparent"
                            e.currentTarget.style.color = "var(--ol-ink-3)"
                        }}
                    >
                        <Icon name="close" size={13} />
                    </button>
                </div>
                <div style={{ display: "flex", minHeight: 0, flex: 1, overflow: "hidden" }}>
                    {/* 左侧：模型选择（竖排）——与设置页左栏 rail 同风格，
                        宽度对齐设置页（200px），让弹窗看起来和设置页是一体的 */}
                    <div
                        style={{
                            width: 200,
                            flexShrink: 0,
                            background: "var(--ol-settings-rail-bg)",
                            borderRight: "0.5px solid var(--ol-line-soft)",
                            padding: "14px 12px",
                            overflowY: "auto",
                            display: "flex",
                            flexDirection: "column",
                            gap: 4,
                        }}
                    >
                        <div
                            style={{
                                fontSize: 12,
                                fontWeight: 600,
                                color: "var(--ol-ink-4)",
                                marginBottom: 6,
                            }}
                        >
                            {t("localAsr.sidebarTitle")}
                        </div>
                        {entries.map((entry) => (
                            <button
                                key={entry.id}
                                type="button"
                                onClick={() => onSelect(entry.id)}
                                style={{
                                    display: "flex",
                                    alignItems: "center",
                                    gap: 8,
                                    padding: "7px 10px",
                                    borderRadius: 8,
                                    border: 0,
                                    background:
                                        entry.id === resolvedId
                                            ? "var(--ol-segmented-active-bg)"
                                            : "transparent",
                                    boxShadow:
                                        entry.id === resolvedId
                                            ? "var(--ol-segmented-active-shadow)"
                                            : "none",
                                    color: "var(--ol-ink)",
                                    fontFamily: "inherit",
                                    fontSize: 12.5,
                                    textAlign: "left",
                                    cursor: "pointer",
                                    transition:
                                        "background 0.12s var(--ol-motion-quick), box-shadow 0.12s var(--ol-motion-quick)",
                                }}
                            >
                                <span
                                    style={{
                                        width: 14,
                                        color: entry.isDownloaded
                                            ? "var(--ol-ok)"
                                            : "var(--ol-ink-4)",
                                        flexShrink: 0,
                                        display: "inline-flex",
                                    }}
                                >
                                    {entry.isDownloaded ? <IconCheck /> : "•"}
                                </span>
                                <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                                    {entry.name}
                                </span>
                            </button>
                        ))}
                        {entries.length === 0 && (
                            <div style={{ fontSize: 12, color: "var(--ol-ink-4)", padding: "4px 2px" }}>
                                {t("localAsr.modelSelectEmpty")}
                            </div>
                        )}
                    </div>
                    {/* 右侧：模型信息 + HF 模型卡片（下载量/收藏/简介） */}
                    <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column" }}>
                        <div style={{ flex: 1, minHeight: 0, overflowY: "auto", padding: 16 }}>
                            {selected ? (
                                <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                                    <div style={{ fontSize: 13.5, fontWeight: 600, color: "var(--ol-ink)" }}>
                                        {selected.name}
                                    </div>
                                    {selected.repo && (
                                        <div style={{ fontSize: 11.5, color: "var(--ol-ink-4)" }}>
                                            {t("localAsr.detailRepo")}: {selected.repo}
                                        </div>
                                    )}
                                    <div style={{ display: "flex", flexWrap: "wrap", gap: 6, fontSize: 11 }}>
                                        {(() => {
                                            const bytes = sizeOf(selected.id)
                                            const files = fileCountOf(selected.id)
                                            return (
                                                <>
                                                    {bytes != null && bytes > 0 && (
                                                        <span style={{ padding: "2px 8px", borderRadius: 999, background: "rgba(0,0,0,0.05)", color: "var(--ol-ink-2)" }}>
                                                            {formatBytes(bytes)}
                                                        </span>
                                                    )}
                                                    {files != null && files > 0 && (
                                                        <span style={{ padding: "2px 8px", borderRadius: 999, background: "rgba(0,0,0,0.05)", color: "var(--ol-ink-2)" }}>
                                                            {files} {t("localAsr.detailFiles")}
                                                        </span>
                                                    )}
                                                </>
                                            )
                                        })()}
                                        {selected.isDownloaded && (
                                            <span style={{ padding: "2px 8px", borderRadius: 999, background: "rgba(40,160,90,0.12)", color: "var(--ol-ok)" }}>
                                                ✓ {t("localAsr.detailDownloaded")}
                                            </span>
                                        )}
                                    </div>
                                    {hfCard?.status === "loading" && (
                                        <div style={{ fontSize: 11.5, color: "var(--ol-ink-4)" }}>
                                            {t("common.loading")}
                                        </div>
                                    )}
                                    {hfCard?.status === "error" && (
                                        <div style={{ fontSize: 11.5, color: "#9b2c2c", lineHeight: 1.5 }}>
                                            {t("localAsr.hfCardFailed")}: {hfCard.message}
                                        </div>
                                    )}
                                    {hfCard?.status === "ok" && (
                                        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                                            <div style={{ display: "flex", flexWrap: "wrap", gap: 6, fontSize: 11 }}>
                                                <span style={{ padding: "2px 8px", borderRadius: 999, background: "rgba(0,0,0,0.05)", color: "var(--ol-ink-2)" }}>
                                                    {t("localAsr.hfDownloads")}: {formatCount(hfCard.card.downloads)}
                                                </span>
                                                <span style={{ padding: "2px 8px", borderRadius: 999, background: "rgba(0,0,0,0.05)", color: "var(--ol-ink-2)" }}>
                                                    {t("localAsr.hfLikes")}: {formatCount(hfCard.card.likes)}
                                                </span>
                                            </div>
                                            {hfCard.card.description ? (
                                                <>
                                                    <div style={{ fontSize: 11, color: "var(--ol-ink-4)", letterSpacing: ".02em" }}>
                                                        {t("localAsr.hfDescription")}
                                                    </div>
                                                    <div style={{
                                                        fontSize: 12,
                                                        color: "var(--ol-ink-3)",
                                                        lineHeight: 1.65,
                                                        display: "-webkit-box",
                                                        WebkitLineClamp: 3,
                                                        WebkitBoxOrient: "vertical",
                                                        overflow: "hidden",
                                                    }}>
                                                        {hfCard.card.description}
                                                    </div>
                                                </>
                                            ) : (
                                                <div style={{ fontSize: 11.5, color: "var(--ol-ink-4)" }}>
                                                    {t("localAsr.hfNoDescription")}
                                                </div>
                                            )}
                                        </div>
                                    )}
                                    {selected.isDownloaded && (
                                        <div style={{ fontSize: 11.5, color: "var(--ol-ink-4)" }}>
                                            {t("localAsr.downloadDialogAlreadyHave")}
                                        </div>
                                    )}
                                </div>
                            ) : (
                                <div style={{ fontSize: 12, color: "var(--ol-ink-4)" }}>
                                    {t("localAsr.detailEmpty")}
                                </div>
                            )}
                        </div>
                        <div
                            style={{
                                padding: "12px 18px",
                                borderTop: "0.5px solid var(--ol-line)",
                                display: "flex",
                                justifyContent: "flex-end",
                                gap: 8,
                            }}
                        >
                            <Btn variant="ghost" size="sm" disabled={busy} onClick={onClose}>
                                {t("common.cancel")}
                            </Btn>
                            <Btn
                                variant="primary"
                                size="sm"
                                disabled={busy || !selected || selected.isDownloaded}
                                onClick={onStart}
                            >
                                {t("localAsr.startDownload")}
                            </Btn>
                        </div>
                    </div>
                </div>
            </div>
        </div>,
        document.body,
    )
}
