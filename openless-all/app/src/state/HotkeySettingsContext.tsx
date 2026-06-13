import {
    createContext,
    useCallback,
    useContext,
    useEffect,
    useMemo,
    useRef,
    useState,
    type ReactNode,
} from "react"
import {
    getHotkeyCapability,
    getSettings,
    isTauri,
    setSettings,
} from "../lib/ipc"
import type {
    HotkeyBinding,
    HotkeyCapability,
    UserPreferences,
} from "../lib/types"
import i18n, { outputPrefsForLocale, type SupportedLocale } from "../i18n"
import { applyThemeFromPreference } from "../lib/themeMode"

interface HotkeySettingsContextValue {
    prefs: UserPreferences | null
    hotkey: HotkeyBinding | null
    capability: HotkeyCapability | null
    loading: boolean
    error: string | null
    refresh: () => Promise<void>
    updatePrefs: (
        next: UserPreferences | ((current: UserPreferences) => UserPreferences),
    ) => Promise<void>
}

const HotkeySettingsContext = createContext<HotkeySettingsContextValue | null>(
    null,
)

const errorMessage = (error: unknown) =>
    String(error instanceof Error ? error.message : error)

export function HotkeySettingsProvider({ children }: { children: ReactNode }) {
    const [prefs, setPrefs] = useState<UserPreferences | null>(null)
    const [capability, setCapability] = useState<HotkeyCapability | null>(null)
    const [loading, setLoading] = useState(true)
    const [error, setError] = useState<string | null>(null)
    const persistQueueRef = useRef<Promise<void>>(Promise.resolve())
    const latestPrefsRef = useRef<UserPreferences | null>(null)

    const refresh = useCallback(async () => {
        setLoading(true)
        setError(null)
        try {
            const [prefsResult, capabilityResult] = await Promise.allSettled([
                getSettings(),
                getHotkeyCapability(),
            ])
            let nextError: string | null = null
            if (prefsResult.status === "fulfilled") {
                setPrefs(prefsResult.value)
                applyThemeFromPreference(prefsResult.value.themeMode ?? "system")
            } else {
                console.error(
                    "[hotkey-settings] failed to load preferences",
                    prefsResult.reason,
                )
                nextError = errorMessage(prefsResult.reason)
            }
            if (capabilityResult.status === "fulfilled") {
                setCapability(capabilityResult.value)
            } else {
                console.error(
                    "[hotkey-settings] failed to load hotkey capability",
                    capabilityResult.reason,
                )
                nextError = errorMessage(capabilityResult.reason)
            }
            setError(nextError)
        } catch (error) {
            console.error(
                "[hotkey-settings] failed to refresh hotkey settings",
                error,
            )
            setError(errorMessage(error))
        } finally {
            setLoading(false)
        }
    }, [])

    const queueSetSettings = useCallback(
        (resolved: UserPreferences) => {
            const task = persistQueueRef.current
                .catch(() => undefined)
                .then(async () => {
                    await setSettings(resolved)
                })
            persistQueueRef.current = task
            return task
        },
        [],
    )

    useEffect(() => {
        void refresh()
    }, [refresh])

    useEffect(() => {
        if (!isTauri) return
        let cancelled = false
        let unlisten: (() => void) | undefined
        void (async () => {
            try {
                const { listen } = await import("@tauri-apps/api/event")
                const handle = await listen<UserPreferences>(
                    "prefs:changed",
                    (event) => {
                        const nextPrefs = event.payload
                        if (!nextPrefs) return
                        latestPrefsRef.current = nextPrefs
                        setPrefs(nextPrefs)
                        applyThemeFromPreference(nextPrefs.themeMode ?? "system")
                    },
                )
                if (cancelled) {
                    handle()
                } else {
                    unlisten = handle
                }
            } catch (error) {
                console.warn(
                    "[settings] prefs:changed listener setup failed",
                    error,
                )
            }
        })()
        return () => {
            cancelled = true
            unlisten?.()
        }
    }, [])

    useEffect(() => {
        latestPrefsRef.current = prefs
    }, [prefs])

    useEffect(() => {
        const currentPrefs = latestPrefsRef.current
        if (!currentPrefs) return
        const lang = (
            i18n.resolvedLanguage ||
            i18n.language ||
            ""
        ).toLowerCase()
        const resolvedLocale: SupportedLocale =
            lang.startsWith("zh-tw") || lang.includes("hant")
                ? "zh-TW"
                : lang.startsWith("zh-cn") || lang.startsWith("zh")
                  ? "zh-CN"
                  : lang.startsWith("ja")
                    ? "ja"
                    : lang.startsWith("ko")
                      ? "ko"
                      : "en"
        const nextLocalePrefs = outputPrefsForLocale(resolvedLocale)
        if (
            currentPrefs.chineseScriptPreference ===
                nextLocalePrefs.chineseScriptPreference &&
            currentPrefs.outputLanguagePreference ===
                nextLocalePrefs.outputLanguagePreference
        ) {
            return
        }
        const merged = { ...currentPrefs, ...nextLocalePrefs }
        latestPrefsRef.current = merged
        setPrefs(merged)
        void queueSetSettings(merged).catch((error) => {
            console.warn(
                "[settings] sync locale output preferences failed",
                error,
            )
        })
    }, [prefs, queueSetSettings])

    const updatePrefs = useCallback(
        async (
            next:
                | UserPreferences
                | ((current: UserPreferences) => UserPreferences),
        ) => {
            const current = latestPrefsRef.current
            if (!current) return
            const resolved = typeof next === "function" ? next(current) : next
            if (resolved === current) return
            setPrefs(resolved)
            latestPrefsRef.current = resolved
            await queueSetSettings(resolved)
        },
        [queueSetSettings],
    )

    const value = useMemo<HotkeySettingsContextValue>(
        () => ({
            prefs,
            hotkey: prefs?.hotkey ?? null,
            capability,
            loading,
            error,
            refresh,
            updatePrefs,
        }),
        [capability, error, loading, prefs, refresh, updatePrefs],
    )

    return (
        <HotkeySettingsContext.Provider value={value}>
            {children}
        </HotkeySettingsContext.Provider>
    )
}

export function useHotkeySettings() {
    const value = useContext(HotkeySettingsContext)
    if (!value) {
        throw new Error(
            "useHotkeySettings must be used within HotkeySettingsProvider",
        )
    }
    return value
}
