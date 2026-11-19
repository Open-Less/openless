'use client'

import {
    AnimatePresence,
    animate,
    motion,
    useMotionValue,
    useMotionValueEvent,
    useReducedMotion,
    useSpring,
    useTransform,
    type AnimationPlaybackControls,
    type Transition,
    type Variants,
} from 'motion/react'
import React, { useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from 'react'

import { cn } from '@/lib/utils'

export type PromptEffort = {
    id: string
    label: string
    color?: string
}

export type PromptBoxStatus = 'idle' | 'streaming'

type RGB = [number, number, number]

const DEFAULT_EFFORTS: PromptEffort[] = [
    { id: 'quick', label: 'Quick' },
    { id: 'think', label: 'Think', color: '#F5A524' },
    { id: 'deep', label: 'Think deeply', color: '#F2552C' },
]

const NEUTRAL: RGB = [134, 133, 147]
// a press shorter than this is a tap, so it sends at the first effort
const HOLD_DELAY = 180
const LEVEL_MS = 650
const LINE = 24
const MAX_HEIGHT = 192
const PLACEHOLDER_MS = 3200
// past this many words the text fades as one block instead of flying word by word
const MAX_WORDS = 80

const SWAP: Transition = { type: 'spring', duration: 0.45, bounce: 0.15 }
const GROW: Transition = { type: 'spring', duration: 0.4, bounce: 0.1 }
const SETTLE: Transition = { type: 'spring', duration: 0.7, bounce: 0 }
const INSTANT: Transition = { duration: 0 }
const LIFT = [0.32, 0.72, 0, 1] as const
// the words rise toward the thread above the box, where the sent message lands
const RISE = 32

const swapVariants: Variants = {
    enter: { y: 6, opacity: 0, filter: 'blur(4px)' },
    center: { y: 0, opacity: 1, filter: 'blur(0px)' },
    exit: { y: -6, opacity: 0, filter: 'blur(4px)' },
}

const iconVariants: Variants = {
    enter: { scale: 0.4, opacity: 0, filter: 'blur(4px)' },
    center: { scale: 1, opacity: 1, filter: 'blur(0px)' },
    exit: { scale: 0.4, opacity: 0, filter: 'blur(4px)' },
}

const toRgb = (color: string | undefined): RGB => {
    const hex = color?.trim().replace(/^#/, '') ?? ''
    const full = hex.length === 3 ? [...hex].map((c) => c + c).join('') : hex
    if (!/^[0-9a-f]{6}$/i.test(full)) {
        return NEUTRAL
    }
    const n = Number.parseInt(full, 16)
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255]
}

const rgba = ([r, g, b]: RGB, alpha: number) => `rgba(${r}, ${g}, ${b}, ${alpha})`

export type PromptBoxProps = Omit<React.ComponentProps<'div'>, 'onSubmit' | 'children' | 'defaultValue'> & {
    value?: string
    defaultValue?: string
    onValueChange?: (value: string) => void
    onSubmit?: (prompt: string, effort: PromptEffort) => void
    status?: PromptBoxStatus
    onStop?: () => void
    efforts?: PromptEffort[]
    placeholder?: string | string[]
    hint?: React.ReactNode
    actions?: React.ReactNode
    disabled?: boolean
    inputRef?: React.Ref<HTMLTextAreaElement>
    inputProps?: Omit<React.ComponentProps<'textarea'>, 'ref' | 'value' | 'defaultValue' | 'onChange' | 'disabled' | 'rows'>
}

type Ghost = { id: number; text: string; scroll: number; width: number; height: number }

// a release this far outside the button still counts, since the button shrinks while charging
const SLOP = 8

function PromptBox({
    value: valueProp,
    defaultValue = '',
    onValueChange,
    onSubmit,
    status = 'idle',
    onStop,
    efforts: effortsProp = DEFAULT_EFFORTS,
    placeholder = 'Ask anything',
    hint = 'Hold to think harder',
    actions,
    disabled = false,
    inputRef,
    inputProps,
    className,
    ...props
}: PromptBoxProps) {
    const reduced = !!useReducedMotion()
    const efforts = effortsProp.length ? effortsProp : DEFAULT_EFFORTS
    const top = efforts.length - 1
    const stops = efforts.map((effort) => toRgb(effort.color))
    const placeholders = Array.isArray(placeholder) ? placeholder : [placeholder]

    const [inner, setInner] = useState(defaultValue)
    const value = valueProp ?? inner
    const streaming = status === 'streaming'
    const canSend = !disabled && !streaming && value.trim().length > 0

    const fieldRef = useRef<HTMLDivElement>(null)
    const textareaRef = useRef<HTMLTextAreaElement>(null)
    const ghostRef = useRef<HTMLDivElement>(null)
    const iconRef = useRef<HTMLSpanElement>(null)
    const press = useRef<'key' | 'pointer' | null>(null)
    const holdTimer = useRef<number | undefined>(undefined)
    const charge = useRef<AnimationPlaybackControls | null>(null)
    const levelRef = useRef(0)
    const ghostCount = useRef(0)
    const stopPress = useRef(false)
    const pointered = useRef(false)
    const unlisten = useRef<(() => void) | null>(null)

    const [charging, setCharging] = useState(false)
    const [level, setLevel] = useState(0)
    const [ghost, setGhost] = useState<Ghost | null>(null)
    const [sent, setSent] = useState<RGB>(NEUTRAL)
    const [placeholderIndex, setPlaceholderIndex] = useState(0)
    const [fieldWidth, setFieldWidth] = useState(0)

    const progress = useMotionValue(0)
    // steps per level rather than blending, so the button never shows a color the label hasn't reached
    const tint = useMotionValue(rgba(stops[0], 1))
    const squeeze = useSpring(useTransform(progress, [0, top || 1], [1, 0.92]), { stiffness: 300, damping: 30 })
    const surface = useTransform(progress, (p) => {
        const t = top ? Math.min(Math.max(p / top, 0), 1) : 0
        return `${(1 - t) * 100}%`
    })
    const liquid = useTransform(progress, (p) => (p > 0.01 ? 1 : 0))

    const setValue = (next: string) => {
        if (valueProp === undefined) {
            setInner(next)
        }
        onValueChange?.(next)
    }

    useMotionValueEvent(progress, 'change', (p) => {
        const next = Math.max(0, Math.min(Math.floor(p + 0.001), top))
        if (next === levelRef.current) {
            return
        }
        if (next > levelRef.current && press.current && !reduced) {
            if (iconRef.current) {
                animate(iconRef.current, { scale: [1, 1.12, 1] }, { duration: 0.45, ease: 'easeInOut' })
            }
            navigator.vibrate?.(8)
        }
        if (next > levelRef.current) {
            animate(tint, rgba(stops[next], 1), reduced ? INSTANT : { duration: 0.35, ease: 'easeOut' })
        }
        levelRef.current = next
        setLevel(next)
    })

    // the height stays put while words rise out, so the box doesn't shrink under them
    useLayoutEffect(() => {
        const el = textareaRef.current
        const field = fieldRef.current
        if (!el || !field || ghost) {
            return
        }
        el.style.height = '0px'
        const next = Math.min(Math.max(el.scrollHeight, LINE), MAX_HEIGHT)
        el.style.height = `${next}px`
        animate(field, { height: next }, reduced ? INSTANT : GROW)
    }, [value, ghost, fieldWidth, reduced])

    useEffect(() => {
        const field = fieldRef.current
        if (!field) {
            return
        }
        const observer = new ResizeObserver(() => setFieldWidth(field.offsetWidth))
        observer.observe(field)
        return () => observer.disconnect()
    }, [])

    const rotating = placeholders.length > 1 && !value
    useEffect(() => {
        if (!rotating) {
            return
        }
        const id = window.setInterval(() => setPlaceholderIndex((i) => i + 1), PLACEHOLDER_MS)
        return () => window.clearInterval(id)
    }, [rotating])

    useEffect(
        () => () => {
            window.clearTimeout(holdTimer.current)
            charge.current?.stop()
            unlisten.current?.()
        },
        [],
    )

    useLayoutEffect(() => {
        const layer = ghostRef.current
        if (!ghost || !layer) {
            return
        }
        let cancelled = false
        const view = layer.getBoundingClientRect()
        const words = Array.from(layer.querySelectorAll<HTMLElement>('[data-word]')).filter((word) => {
            const box = word.getBoundingClientRect()
            const seen = box.bottom > view.top + RISE && box.top < view.bottom
            if (!seen) {
                word.style.visibility = 'hidden'
            }
            return seen
        })
        // the whole flight stays under half a second however long the prompt is
        const step = Math.min(0.03, 0.3 / Math.max(words.length, 1))
        const flights = words.length
            ? words.map((word, i) =>
                  animate(
                      word,
                      { y: -RISE, opacity: 0, filter: 'blur(4px)' },
                      { duration: 0.4, delay: i * step, ease: LIFT },
                  ),
              )
            : [animate(layer.firstElementChild ?? layer, { y: -8, opacity: 0, filter: 'blur(4px)' }, { duration: 0.35, ease: LIFT })]

        Promise.all(flights).then(() => {
            if (cancelled) {
                return
            }
            setGhost(null)
            if (iconRef.current) {
                animate(iconRef.current, { scale: [1, 1.25, 1] }, { duration: 0.35, ease: 'easeOut' })
            }
        })
        return () => {
            cancelled = true
            flights.forEach((flight) => flight.stop())
        }
    }, [ghost])

    const submit = (effort: PromptEffort) => {
        const prompt = value.trim()
        if (!prompt || disabled || streaming) {
            return
        }
        if (!reduced) {
            ghostCount.current += 1
            const el = textareaRef.current
            setGhost({
                id: ghostCount.current,
                text: value,
                scroll: el?.scrollTop ?? 0,
                width: el?.clientWidth ?? 0,
                height: fieldRef.current?.offsetHeight ?? LINE,
            })
        }
        setSent(toRgb(effort.color))
        setValue('')
        onSubmit?.(prompt, effort)
    }

    const begin = (source: 'key' | 'pointer') => {
        if (!canSend || press.current) {
            return
        }
        press.current = source
        // window-wide, so escape cancels a mouse hold even when the textarea isn't focused
        const onKey = (event: KeyboardEvent) => {
            if (event.key === 'Escape') {
                event.preventDefault()
                cancel()
            }
        }
        window.addEventListener('keydown', onKey)
        unlisten.current = () => window.removeEventListener('keydown', onKey)
        // a press during the drain after escape would otherwise start from, and tap-send at, the leftover level
        progress.jump(0)
        levelRef.current = 0
        setLevel(0)
        tint.set(rgba(stops[0], 1))
        if (!top) {
            return
        }
        holdTimer.current = window.setTimeout(() => {
            setCharging(true)
            charge.current = animate(progress, top, { duration: (top * LEVEL_MS) / 1000, ease: 'linear' })
        }, HOLD_DELAY)
    }

    const settle = () => {
        press.current = null
        unlisten.current?.()
        unlisten.current = null
        window.clearTimeout(holdTimer.current)
        charge.current?.stop()
        setCharging(false)
        animate(progress, 0, reduced ? INSTANT : SETTLE)
    }

    const release = () => {
        if (!press.current) {
            return
        }
        const effort = efforts[Math.min(levelRef.current, top)]
        settle()
        submit(effort)
    }

    const cancel = () => {
        if (press.current) {
            settle()
        }
    }

    const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
        inputProps?.onKeyDown?.(event)
        if (event.defaultPrevented || event.key !== 'Enter' || event.shiftKey || event.nativeEvent.isComposing) {
            return
        }
        event.preventDefault()
        if (!event.repeat) {
            begin('key')
        }
    }

    useImperativeHandle(inputRef, () => textareaRef.current as HTMLTextAreaElement, [])

    const tokens = ghost?.text.split(/(\s+)/) ?? []
    const flyWords = tokens.filter((token) => token.trim()).length <= MAX_WORDS
    const effort = efforts[Math.min(level, top)]
    const ready = canSend || streaming

    return (
        <div
            data-slot="prompt-box"
            data-status={status}
            data-charging={charging || undefined}
            data-disabled={disabled || undefined}
            className={cn(
                'relative flex w-[560px] max-w-full flex-col rounded-[24px] border border-black/[0.08] bg-white font-[family-name:var(--font-open-runde,ui-rounded),ui-rounded,system-ui,sans-serif] text-black shadow-[0_1px_2px_rgba(0,0,0,0.04)] dark:border-white/10 dark:bg-[#0f0f0f] dark:text-white',
                disabled && 'opacity-60',
                className,
            )}
            {...props}
        >
            <div className="relative px-4 pt-4">
                <motion.div ref={fieldRef} className="relative overflow-hidden" style={{ height: LINE }}>
                    <AnimatePresence initial={false}>
                        {!value && (
                            <motion.span
                                key="placeholder"
                                aria-hidden
                                initial={{ opacity: 0 }}
                                animate={{ opacity: 1 }}
                                exit={{ opacity: 0, transition: INSTANT }}
                                transition={reduced ? INSTANT : { duration: 0.2, delay: ghost ? 0.25 : 0 }}
                                className="pointer-events-none absolute inset-x-0 top-0 grid text-[15px] leading-6 text-[#868593]"
                            >
                                <AnimatePresence initial={false} mode="popLayout">
                                    <motion.span
                                        key={placeholderIndex % placeholders.length}
                                        variants={swapVariants}
                                        initial="enter"
                                        animate="center"
                                        exit="exit"
                                        transition={reduced ? INSTANT : SWAP}
                                        className="truncate"
                                    >
                                        {placeholders[placeholderIndex % placeholders.length]}
                                    </motion.span>
                                </AnimatePresence>
                            </motion.span>
                        )}
                    </AnimatePresence>
                    <textarea
                        aria-label={placeholders[0]}
                        {...inputProps}
                        ref={textareaRef}
                        data-slot="prompt-box-input"
                        rows={1}
                        value={value}
                        disabled={disabled}
                        onChange={(event) => setValue(event.target.value)}
                        onKeyDown={handleKeyDown}
                        onKeyUp={(event) => {
                            inputProps?.onKeyUp?.(event)
                            if (event.key === 'Enter') {
                                release()
                            }
                        }}
                        onBlur={(event) => {
                            inputProps?.onBlur?.(event)
                            if (press.current === 'key') {
                                cancel()
                            }
                        }}
                        style={{ ...inputProps?.style, height: LINE }}
                        className={cn(
                            'relative block w-full resize-none break-words bg-transparent text-[15px] leading-6 outline-none [scrollbar-width:thin] disabled:cursor-not-allowed',
                            inputProps?.className,
                        )}
                    />
                </motion.div>

                {ghost && (
                    // clipped to what the textarea showed, plus headroom above for the rise
                    <div
                        key={ghost.id}
                        ref={ghostRef}
                        aria-hidden
                        className="pointer-events-none absolute inset-x-4 z-10 overflow-hidden"
                        style={{
                            top: 16 - (flyWords ? RISE : 0),
                            height: ghost.height + (flyWords ? RISE : 0),
                            paddingTop: flyWords ? RISE : 0,
                        }}
                    >
                        <div
                            className="whitespace-pre-wrap break-words text-[15px] leading-6"
                            style={{ marginTop: -ghost.scroll, width: ghost.width || undefined }}
                        >
                            {flyWords
                                ? tokens.map((token, i) =>
                                      token.trim() ? (
                                          <span key={i} data-word className="inline-block">
                                              {token}
                                          </span>
                                      ) : (
                                          token
                                      ),
                                  )
                                : ghost.text}
                        </div>
                    </div>
                )}
            </div>

            <div className="flex items-center gap-2 px-3 pt-3 pb-3">
                <div data-slot="prompt-box-actions" className="flex min-w-0 flex-1 items-center gap-1">
                    {actions}
                </div>

                {/* only the effort is announced; the hint repeats the button's label and would speak on every first keystroke */}
                <span aria-live="polite" className="sr-only">
                    {charging ? effort.label : ''}
                </span>
                <span aria-hidden className="relative grid text-[13px] font-medium whitespace-nowrap">
                    {/* the span collapses once its only child pops out, so a left anchor would fling the exit over the button */}
                    <AnimatePresence initial={false} mode="popLayout" anchorX="right">
                        {charging ? (
                            <motion.span
                                key="effort"
                                variants={swapVariants}
                                initial="enter"
                                animate="center"
                                exit="exit"
                                transition={reduced ? INSTANT : SWAP}
                                data-slot="prompt-box-effort"
                                className="flex items-center gap-2 text-black/75 dark:text-white/80"
                            >
                                <span
                                    aria-hidden
                                    className="flex h-3 items-end gap-[2px] transition-colors duration-300"
                                    style={{ color: effort.color ?? rgba(NEUTRAL, 1) }}
                                >
                                    {efforts.map((item, i) => (
                                        <motion.span
                                            key={item.id}
                                            className="w-[3px] origin-bottom rounded-full bg-current"
                                            style={{ height: 5 + (7 * i) / Math.max(top, 1) }}
                                            initial={false}
                                            animate={i <= level ? { opacity: 1, scaleY: 1 } : { opacity: 0.2, scaleY: 0.7 }}
                                            transition={reduced ? INSTANT : SWAP}
                                        />
                                    ))}
                                </span>
                                <span className="grid">
                                    <AnimatePresence initial={false} mode="popLayout">
                                        <motion.span
                                            key={effort.id}
                                            variants={swapVariants}
                                            initial="enter"
                                            animate="center"
                                            exit="exit"
                                            transition={reduced ? INSTANT : SWAP}
                                        >
                                            {effort.label}
                                        </motion.span>
                                    </AnimatePresence>
                                </span>
                            </motion.span>
                        ) : (
                            hint &&
                            canSend &&
                            top > 0 && (
                                <motion.span
                                    key="hint"
                                    variants={swapVariants}
                                    initial="enter"
                                    animate="center"
                                    exit="exit"
                                    transition={reduced ? INSTANT : SWAP}
                                    className="text-[#868593]/70"
                                >
                                    {hint}
                                </motion.span>
                            )
                        )}
                    </AnimatePresence>
                </span>

                <div className="relative flex size-9 shrink-0 items-center justify-center">
                    <AnimatePresence>
                        {streaming && (
                            <motion.span
                                key="spinner"
                                aria-hidden
                                className="pointer-events-none absolute -inset-[3px] rounded-full"
                                style={{
                                    background: `conic-gradient(from 0deg, ${rgba(sent, 0)} 0%, ${rgba(sent, 0)} 35%, ${rgba(sent, 1)} 100%)`,
                                    mask: 'radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1.5px))',
                                    WebkitMask: 'radial-gradient(farthest-side, transparent calc(100% - 2px), #000 calc(100% - 1.5px))',
                                }}
                                initial={{ opacity: 0, scale: 0.85 }}
                                animate={reduced ? { opacity: 1, scale: 1 } : { opacity: 1, scale: 1, rotate: 360 }}
                                exit={{ opacity: 0, scale: 0.85 }}
                                transition={{
                                    opacity: { duration: 0.3 },
                                    scale: SWAP,
                                    rotate: { duration: 1.1, ease: 'linear', repeat: Infinity },
                                }}
                            />
                        )}
                    </AnimatePresence>

                    <motion.button
                        type="button"
                        data-slot="prompt-box-button"
                        disabled={!ready}
                        aria-label={streaming ? 'Stop' : 'Send, hold to think harder'}
                        style={{ scale: squeeze }}
                        // keeps focus in the textarea, so a press doesn't blur it and drop the charge
                        onMouseDown={(event) => event.preventDefault()}
                        onContextMenu={(event) => event.preventDefault()}
                        onPointerDown={(event) => {
                            if (event.button !== 0) {
                                return
                            }
                            pointered.current = true
                            stopPress.current = streaming
                            if (streaming) {
                                return
                            }
                            event.currentTarget.setPointerCapture(event.pointerId)
                            begin('pointer')
                        }}
                        onPointerUp={(event) => {
                            if (press.current !== 'pointer') {
                                return
                            }
                            const box = event.currentTarget.getBoundingClientRect()
                            const inside =
                                event.clientX >= box.left - SLOP &&
                                event.clientX <= box.right + SLOP &&
                                event.clientY >= box.top - SLOP &&
                                event.clientY <= box.bottom + SLOP
                            if (inside) {
                                release()
                            } else {
                                cancel()
                            }
                        }}
                        onPointerCancel={cancel}
                        // a mouse press can end with the button disabled and no click to clear the flag
                        onKeyDown={() => {
                            pointered.current = false
                        }}
                        onClick={() => {
                            const fromPointer = pointered.current
                            pointered.current = false
                            // keyboard, screen readers and synthetic clicks arrive without a pointer press
                            if (!fromPointer) {
                                if (streaming) {
                                    onStop?.()
                                } else {
                                    submit(efforts[0])
                                }
                            } else if (stopPress.current && streaming) {
                                // the click after a send lands on the stop button, so only a press that began while streaming stops
                                stopPress.current = false
                                onStop?.()
                            }
                        }}
                        className={cn(
                            'relative flex size-9 touch-none items-center justify-center overflow-hidden rounded-full transition-colors outline-none select-none focus-visible:ring-2 focus-visible:ring-black/20 dark:focus-visible:ring-white/30',
                            ready
                                ? 'cursor-pointer bg-black text-white dark:bg-white dark:text-black'
                                : 'cursor-default bg-black/[0.06] text-[#868593] dark:bg-white/[0.08]',
                        )}
                    >
                        <motion.span
                            aria-hidden
                            className="pointer-events-none absolute inset-0"
                            style={{ y: surface, opacity: liquid, backgroundColor: tint, color: tint }}
                        >
                            <motion.svg
                                viewBox="0 0 72 6"
                                className="absolute -top-[5px] left-0 h-1.5 w-[72px]"
                                animate={charging && !reduced ? { x: [0, -36] } : { x: 0 }}
                                transition={charging && !reduced ? { duration: 1.6, ease: 'linear', repeat: Infinity } : INSTANT}
                            >
                                <path
                                    d="M0 3 Q4.5 0.5 9 3 T18 3 T27 3 T36 3 T45 3 T54 3 T63 3 T72 3 V6 H0 Z"
                                    fill="currentColor"
                                />
                            </motion.svg>
                        </motion.span>
                        <motion.span
                            ref={iconRef}
                            // stays dark until the fill reaches it, or it vanishes on the white dark-mode button
                            className={cn('relative grid size-[18px] place-items-center transition-colors duration-200', charging && level > 0 && 'text-white')}
                        >
                            {/* both icons share one cell; popLayout mismeasures svgs and drops the square low */}
                            <AnimatePresence initial={false}>
                                <motion.svg
                                    key={streaming ? 'stop' : 'send'}
                                    viewBox="0 0 24 24"
                                    width={18}
                                    height={18}
                                    className="col-start-1 row-start-1 block"
                                    variants={iconVariants}
                                    initial="enter"
                                    animate="center"
                                    exit="exit"
                                    transition={reduced ? INSTANT : SWAP}
                                >
                                    {streaming ? (
                                        <motion.rect
                                            x={7}
                                            y={7}
                                            width={10}
                                            height={10}
                                            rx={2.5}
                                            fill="currentColor"
                                            style={{ transformBox: 'fill-box', transformOrigin: 'center' }}
                                            animate={reduced ? undefined : { scale: [1, 0.8, 1] }}
                                            transition={{ duration: 1.8, ease: 'easeInOut', repeat: Infinity }}
                                        />
                                    ) : (
                                        <path
                                            d="M12 19V5M5.5 11.5L12 5l6.5 6.5"
                                            fill="none"
                                            stroke="currentColor"
                                            strokeWidth={2.25}
                                            strokeLinecap="round"
                                            strokeLinejoin="round"
                                        />
                                    )}
                                </motion.svg>
                            </AnimatePresence>
                        </motion.span>
                    </motion.button>
                </div>
            </div>
        </div>
    )
}

export { PromptBox }
export default PromptBox
