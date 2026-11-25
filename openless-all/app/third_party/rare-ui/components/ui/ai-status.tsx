'use client'

import {
    AnimatePresence,
    motion,
    useReducedMotion,
    type Transition,
    type Variants,
} from 'motion/react'
import React, { useEffect, useState } from 'react'
import useMeasure from 'react-use-measure'

import { cn } from '@/lib/utils'

export type AiStatusState = 'thinking' | 'researching' | 'analyzing' | 'writing' | 'done' | 'error'

export type AiStatusStateConfig = {
    label?: string | ((elapsed: string | undefined) => string)
    icon?: React.ReactNode
    active?: boolean
}

const DEFAULT_STATES: Record<AiStatusState, AiStatusStateConfig> = {
    thinking: { label: 'Thinking', active: true },
    researching: { label: 'Researching', active: true },
    analyzing: { label: 'Analyzing', active: true },
    writing: { label: 'Writing', active: true },
    done: { label: (elapsed) => (elapsed ? `Worked for ${elapsed}` : 'Done'), active: false },
    error: { label: (elapsed) => (elapsed ? `Failed after ${elapsed}` : 'Failed'), active: false },
}

const PEN_PATH = 'M3.78181 16.3092L3 21L7.69086 20.2182C8.50544 20.0825 9.25725 19.6956 9.84119 19.1116L20.4198 8.53288C21.1934 7.75922 21.1934 6.5049 20.4197 5.73126L18.2687 3.58024C17.495 2.80658 16.2406 2.80659 15.4669 3.58027L4.88841 14.159C4.30447 14.7429 3.91757 15.4947 3.78181 16.3092Z'
const SPARKLE_PATH = 'M11 5Q12 12 19 13Q12 14 11 21Q10 14 3 13Q10 12 11 5Z'
const WAVE_PATH = 'M3 19C4.11 16.3 5.06 16.3 6.17 19C7.27 21.7 8.22 21.7 9.33 19C10.44 16.3 11.39 16.3 12.5 19'
// points along the wave at eased arc length, so the pen tip stays on the ink as it draws
const WAVE_X = [3, 3.05, 3.18, 3.42, 3.78, 4.42, 5.26, 5.83, 6.29, 6.84, 7.76, 8.67, 9.22, 9.68, 10.25, 11.09, 11.72, 12.09, 12.33, 12.46, 12.5]
const WAVE_Y = [19, 18.88, 18.58, 18.1, 17.51, 17, 17.36, 18.25, 19.29, 20.33, 21.02, 20.32, 19.27, 18.24, 17.35, 17, 17.52, 18.11, 18.6, 18.9, 19]
const WAVE_PROGRESS = [0, 0.007, 0.028, 0.061, 0.104, 0.156, 0.216, 0.282, 0.352, 0.425, 0.5, 0.575, 0.648, 0.718, 0.784, 0.844, 0.896, 0.939, 0.972, 0.993, 1]
const DRAW_END = 0.7
const HOLD_END = 0.85
const SCRIBBLE: Transition = {
    duration: 2.4,
    ease: 'linear',
    repeat: Infinity,
    times: [...WAVE_PROGRESS.map((_, i) => (i / (WAVE_PROGRESS.length - 1)) * DRAW_END), HOLD_END, 1],
}
const TWINKLE_PATH = 'M18 2Q18.4 4.6 21 5Q18.4 5.4 18 8Q17.6 5.4 15 5Q17.6 4.6 18 2Z'

const SWAP: Transition = { type: 'spring', duration: 0.5, bounce: 0.15 }
const ICON: Transition = { type: 'spring', duration: 0.55, bounce: 0.35 }
const INSTANT: Transition = { duration: 0 }

const swapVariants: Variants = {
    enter: { y: 8, opacity: 0, filter: 'blur(4px)' },
    center: { y: 0, opacity: 1, filter: 'blur(0px)' },
    exit: { y: -8, opacity: 0, filter: 'blur(4px)' },
}

const iconVariants: Variants = {
    enter: { scale: 0.5, opacity: 0, rotate: -30, filter: 'blur(4px)' },
    center: { scale: 1, opacity: 1, rotate: 0, filter: 'blur(0px)' },
    exit: { scale: 0.5, opacity: 0, rotate: 30, filter: 'blur(4px)' },
}

// the highlight sits mid gradient and the size leaves room either side, so each sweep starts off screen
const SHIMMER = 'linear-gradient(90deg, var(--ai-status-ink) 40%, var(--ai-status-shine) 50%, var(--ai-status-ink) 60%)'

const STROKE = {
    fill: 'none',
    stroke: 'currentColor',
    strokeLinecap: 'round',
    strokeLinejoin: 'round',
} as const

const LOOP = { repeat: Infinity, ease: 'easeInOut' } as const

const isBuiltIn = (state: string): state is AiStatusState => Object.hasOwn(DEFAULT_STATES, state)

const titleCase = (state: string) => state.charAt(0).toUpperCase() + state.slice(1)

const formatElapsed = (seconds: number) =>
    seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, '0')}s`

export type AiStatusProps = Omit<React.ComponentProps<'div'>, 'children'> & {
    state?: AiStatusState | (string & {})
    states?: Record<string, AiStatusStateConfig>
    label?: string
    icon?: React.ReactNode
    detail?: string
    favicon?: string | React.ReactNode
    timer?: boolean
    elapsed?: number
    shimmer?: boolean
}

function AiStatus({
    state = 'thinking',
    states,
    label,
    icon,
    detail,
    favicon,
    timer = true,
    elapsed,
    shimmer,
    className,
    ...props
}: AiStatusProps) {
    const reduced = !!useReducedMotion()
    const config: AiStatusStateConfig = { ...(isBuiltIn(state) ? DEFAULT_STATES[state] : undefined), ...states?.[state] }
    const active = config.active ?? true
    const [counted, setCounted] = useState(0)
    const [wasActive, setWasActive] = useState(active)
    // a status that mounts already finished, like a message from history, has no run to report a time for
    const [hasRun, setHasRun] = useState(active)
    const counting = timer && elapsed === undefined

    // a new run starts from zero; switching between working states keeps counting
    if (active !== wasActive) {
        setWasActive(active)
        if (active) {
            setCounted(0)
            setHasRun(true)
        }
    }

    useEffect(() => {
        if (!active || !counting) {
            return
        }
        const start = Date.now()
        const id = window.setInterval(() => setCounted(Math.floor((Date.now() - start) / 1000)), 1000)
        return () => window.clearInterval(id)
    }, [active, counting])

    const [measureRef, bounds] = useMeasure()
    const swap = reduced ? INSTANT : SWAP
    const seconds = elapsed ?? (hasRun ? counted : undefined)
    const time = timer && seconds !== undefined ? formatElapsed(Math.max(0, Math.floor(seconds))) : undefined
    const text = label ?? (typeof config.label === 'function' ? config.label(time) : (config.label ?? titleCase(state)))
    const shimmering = shimmer ?? active

    return (
        <div
            data-slot="ai-status"
            data-state={state}
            data-active={active || undefined}
            role="status"
            className={cn('flex h-8 w-fit items-center gap-2 font-runde text-[14px] text-black dark:text-white', className)}
            {...props}
        >
            <span data-slot="ai-status-icon" className="relative flex size-5 shrink-0 items-center justify-center">
                <AnimatePresence initial={false} mode="popLayout">
                    <motion.span
                        key={state}
                        aria-hidden
                        variants={iconVariants}
                        initial="enter"
                        animate="center"
                        exit="exit"
                        transition={reduced ? INSTANT : ICON}
                        className="flex items-center justify-center"
                    >
                        {icon ?? config.icon ?? <AiStatusIcon state={state} />}
                    </motion.span>
                </AnimatePresence>
            </span>

            {/* the row springs between widths, so the detail and timer glide instead of jumping */}
            <motion.div
                className="relative overflow-x-clip"
                initial={false}
                animate={{ width: bounds.width || 'auto' }}
                transition={swap}
            >
                <div ref={measureRef} className="flex w-max items-center gap-1.5 whitespace-nowrap">
                    <span className="relative grid">
                        <AnimatePresence initial={false} mode="popLayout">
                            <motion.span
                                key={text}
                                variants={swapVariants}
                                initial="enter"
                                animate="center"
                                exit="exit"
                                transition={swap}
                                data-slot="ai-status-label"
                                className={cn('font-semibold', !shimmering && 'text-[#868593]')}
                            >
                                {shimmering ? <Shimmer reduced={reduced}>{text}</Shimmer> : text}
                            </motion.span>
                        </AnimatePresence>
                    </span>
                    <AnimatePresence initial={false} mode="popLayout">
                        {detail && (
                            <motion.span
                                key={detail}
                                data-slot="ai-status-detail"
                                variants={swapVariants}
                                initial="enter"
                                animate="center"
                                exit="exit"
                                transition={swap}
                                className="flex min-w-0 items-center gap-1.5 font-medium text-[#868593]/70"
                            >
                                {typeof favicon === 'string' ? (
                                    // eslint-disable-next-line @next/next/no-img-element
                                    <img
                                        src={favicon}
                                        alt=""
                                        draggable={false}
                                        onError={(event) => {
                                            event.currentTarget.style.display = 'none'
                                        }}
                                        className="size-3.5 shrink-0 rounded-[3px] object-contain"
                                    />
                                ) : (
                                    favicon && <span className="flex size-3.5 shrink-0 items-center justify-center">{favicon}</span>
                                )}
                                <span className="max-w-56 truncate">{detail}</span>
                            </motion.span>
                        )}
                    </AnimatePresence>
                    {time && active && (
                        <span data-slot="ai-status-timer" aria-hidden className="font-medium tabular-nums text-[#868593]/70">
                            {time}
                        </span>
                    )}
                </div>
            </motion.div>
        </div>
    )
}

export type AiStatusIconProps = {
    state: AiStatusState | (string & {})
}

function AiStatusIcon({ state }: AiStatusIconProps) {
    const reduced = !!useReducedMotion()
    const loop = (transition: Transition) => (reduced ? INSTANT : transition)

    switch (state) {
        case 'thinking':
            return (
                <svg viewBox="0 0 24 24" width={18} height={18} className="text-[#868593]">
                    <motion.path
                        d={SPARKLE_PATH}
                        fill="currentColor"
                        animate={reduced ? undefined : { rotate: [0, 90], scale: [1, 0.8, 1] }}
                        transition={loop({ duration: 2.4, ...LOOP })}
                    />
                    <motion.path
                        d={TWINKLE_PATH}
                        fill="currentColor"
                        animate={reduced ? undefined : { scale: [0.4, 1, 0.4], opacity: [0.3, 1, 0.3] }}
                        transition={loop({ duration: 1.2, ...LOOP })}
                    />
                </svg>
            )
        case 'researching':
            return (
                <svg viewBox="0 0 24 24" width={18} height={18} className="text-[#868593]">
                    <motion.g
                        animate={reduced ? undefined : { x: [0, 2, 0, -2, 0], y: [-2, 0, 2, 0, -2] }}
                        transition={loop({ duration: 2, repeat: Infinity, ease: 'linear' })}
                    >
                        <circle {...STROKE} cx={10.5} cy={10.5} r={5.5} strokeWidth={2.25} />
                        <path {...STROKE} d="M14.75 14.75L19 19" strokeWidth={2.5} />
                    </motion.g>
                </svg>
            )
        case 'analyzing':
            return (
                <svg viewBox="0 0 24 24" width={18} height={18} className="text-[#868593]">
                    {[6, 12, 18].map((x, i) => (
                        <motion.line
                            key={x}
                            {...STROKE}
                            x1={x}
                            x2={x}
                            y1={10}
                            y2={19}
                            strokeWidth={3}
                            animate={reduced ? undefined : { y1: [15, 5, 15] }}
                            transition={loop({ duration: 0.9, delay: i * 0.15, ...LOOP })}
                        />
                    ))}
                </svg>
            )
        case 'writing':
            return (
                <svg viewBox="0 0 24 24" width={18} height={18} className="text-[#868593]">
                    <g transform="translate(-1.2 -2)">
                        <motion.path
                            {...STROKE}
                            d={WAVE_PATH}
                            strokeWidth={2.25}
                            // spacing past the path's end stops the dash pattern repeating as a dot at the far end
                            style={{ pathSpacing: 2 }}
                            initial={false}
                            animate={reduced ? { pathLength: 1 } : { pathLength: [...WAVE_PROGRESS, 1, 1], opacity: [...WAVE_PROGRESS.map(() => 1), 1, 0] }}
                            transition={loop(SCRIBBLE)}
                        />
                        <motion.g
                            initial={false}
                            animate={
                                reduced
                                    ? { x: WAVE_X.at(-1), y: WAVE_Y.at(-1) }
                                    : { x: [...WAVE_X, WAVE_X.at(-1)!, WAVE_X[0]], y: [...WAVE_Y, WAVE_Y.at(-1)!, WAVE_Y[0]] }
                            }
                            transition={loop(SCRIBBLE)}
                        >
                            {/* puts the pen tip, not its corner, on the point being drawn */}
                            <path d={PEN_PATH} fill="currentColor" transform="scale(0.66) translate(-3 -21)" />
                        </motion.g>
                    </g>
                </svg>
            )
        case 'done':
            return (
                <svg viewBox="0 0 24 24" width={18} height={18} className="text-[#868593]">
                    <motion.path
                        {...STROKE}
                        d="M5 12.5L10 17.5L19 7"
                        strokeWidth={2.5}
                        initial={{ pathLength: reduced ? 1 : 0 }}
                        animate={{ pathLength: 1 }}
                        transition={reduced ? INSTANT : { type: 'spring', duration: 0.6, bounce: 0, delay: 0.1 }}
                    />
                </svg>
            )
        case 'error':
            return (
                <svg viewBox="0 0 24 24" width={18} height={18} className="text-[#FF3B30]">
                    <motion.path
                        {...STROKE}
                        d="M7 7L17 17M17 7L7 17"
                        strokeWidth={2.5}
                        animate={reduced ? undefined : { x: [0, -3, 3, -2, 2, 0] }}
                        transition={{ duration: 0.45, delay: 0.1 }}
                    />
                </svg>
            )
        default:
            return (
                <motion.span
                    className="size-2.5 rounded-full bg-[#868593]"
                    animate={reduced ? undefined : { scale: [1, 0.7, 1], opacity: [1, 0.5, 1] }}
                    transition={loop({ duration: 1.2, ...LOOP })}
                />
            )
    }
}

function Shimmer({ reduced, children }: { reduced: boolean; children: React.ReactNode }) {
    if (reduced) {
        return <span className="text-[#868593]">{children}</span>
    }
    return (
        <motion.span
            className="bg-clip-text text-transparent [--ai-status-ink:#868593] [--ai-status-shine:#000] dark:[--ai-status-shine:#fff]"
            style={{ backgroundImage: SHIMMER, backgroundSize: '250% 100%' }}
            initial={{ backgroundPosition: '100% 0' }}
            animate={{ backgroundPosition: '0% 0' }}
            transition={{ duration: 1.6, ease: 'linear', repeat: Infinity }}
        >
            {children}
        </motion.span>
    )
}

export { AiStatus, AiStatusIcon }
export default AiStatus
