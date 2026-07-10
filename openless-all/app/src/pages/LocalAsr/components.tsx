// Presentational sub-components for the Local ASR page, extracted from
// LocalAsr/index.tsx (behavior-preserving move). All are props-driven and
// stateless beyond local render memoization.

import { useMemo } from "react"
import { useTranslation } from "react-i18next"
import {
    type FoundryPrepareProgress,
    type LocalAsrDownloadProgress,
    type LocalAsrModelStatus,
    type LocalAsrTestResult,
} from "../../lib/localAsr"
import { Btn, Card, Pill } from "../_atoms"
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
