"use client";

import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Globe, Paperclip } from "lucide-react";
import { AiStatus } from "@/components/ui/ai-status";
import {
  PromptBox,
  type PromptBoxStatus,
  type PromptEffort,
} from "@/components/ui/prompt-box";

const PLACEHOLDERS = [
  "Ask anything",
  "Plan a weekend in Kyoto",
  "Explain springs like I'm five",
  "Draft a launch tweet",
];

const THINK_MS: Record<string, number> = {
  quick: 500,
  think: 1800,
  deep: 3400,
};

const ANSWERS: Record<string, string[]> = {
  quick: [
    "Quick answer, straight off the top. Hold send a little longer next time and I'll think it through first.",
    "Here's the short version. For anything tricky, hold the button until it turns amber.",
  ],
  think: [
    "I gave this a moment of thought before answering. Holding send told me it was worth more than a quick reply, so I weighed a couple of options first.",
    "Thought about it for a bit. Keep holding past amber to orange and I'll go even deeper next time.",
  ],
  deep: [
    "You held all the way to orange, so I took my time. I broke the question into parts, checked each one, and only then wrote this. Long holds are for the questions that deserve them.",
    "Deep mode. I worked through this step by step before replying, which is slower but more careful. Tap for speed, hold for depth.",
  ],
};

// the message lands as the words rise out of the box
const LAND_MS = 420;
// on the first send the box glides to the bottom before the message lands
const FIRST_LAND_MS = 560;
const WORD_MS = 45;

type Phase = "thinking" | "writing" | "done" | "stopped";

type Message = {
  id: number;
  prompt: string;
  effort: PromptEffort;
  answer: string;
  phase: Phase;
};

// eased stops, so the top fades out gradually instead of cutting off at a hard line
const FADE =
  "linear-gradient(to bottom, transparent 0, rgba(0,0,0,0.04) 20px, rgba(0,0,0,0.15) 48px, rgba(0,0,0,0.35) 80px, rgba(0,0,0,0.6) 112px, rgba(0,0,0,0.85) 144px, black 176px, black calc(100% - 28px), transparent 100%)";

const SPRING = { type: "spring", duration: 0.5, bounce: 0.15 } as const;

function ActionButton({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      className="flex size-8 cursor-pointer items-center justify-center rounded-full text-[#868593] transition-colors hover:bg-black/[0.05] hover:text-foreground dark:hover:bg-white/[0.08]"
    >
      {children}
    </button>
  );
}

function statusFor({ phase, effort }: Message) {
  if (phase === "thinking") {
    return {
      state: "thinking",
      label: effort.id === "deep" ? "Thinking deeply" : undefined,
    };
  }
  if (phase === "writing") return { state: "writing", label: undefined };
  if (phase === "stopped") return { state: "error", label: "Stopped" };
  return { state: "done", label: undefined };
}

export default function PromptBoxDemo() {
  const [messages, setMessages] = useState<Message[]>([]);
  const [status, setStatus] = useState<PromptBoxStatus>("idle");
  const [started, setStarted] = useState(false);
  const timers = useRef<number[]>([]);
  const count = useRef(0);
  const threadRef = useRef<HTMLDivElement>(null);

  useEffect(() => () => timers.current.forEach(window.clearTimeout), []);

  useEffect(() => {
    const thread = threadRef.current;
    thread?.scrollTo({ top: thread.scrollHeight, behavior: "smooth" });
  }, [messages]);

  const update = (id: number, patch: Partial<Message>) =>
    setMessages((list) =>
      list.map((m) => (m.id === id ? { ...m, ...patch } : m)),
    );

  const handleSubmit = (prompt: string, effort: PromptEffort) => {
    timers.current.forEach(window.clearTimeout);
    count.current += 1;
    const id = count.current;
    const pool = ANSWERS[effort.id] ?? ANSWERS.quick;
    const words = pool[id % pool.length].split(" ");
    const landAt = started ? LAND_MS : FIRST_LAND_MS;
    const writeAt = landAt + (THINK_MS[effort.id] ?? 1000);

    setStarted(true);
    setStatus("streaming");
    timers.current = [
      window.setTimeout(
        () =>
          setMessages((list) => [
            ...list,
            { id, prompt, effort, answer: "", phase: "thinking" },
          ]),
        landAt,
      ),
      ...words.map((_, i) =>
        window.setTimeout(
          () =>
            update(id, {
              phase: "writing",
              answer: words.slice(0, i + 1).join(" "),
            }),
          writeAt + i * WORD_MS,
        ),
      ),
      window.setTimeout(
        () => {
          update(id, { phase: "done" });
          setStatus("idle");
        },
        writeAt + words.length * WORD_MS + 200,
      ),
    ];
  };

  const handleStop = () => {
    timers.current.forEach(window.clearTimeout);
    setMessages((list) =>
      list.map((m) =>
        m.phase === "thinking" || m.phase === "writing"
          ? { ...m, phase: "stopped" }
          : m,
      ),
    );
    setStatus("idle");
  };

  const empty = !started;

  return (
    <div className="relative flex h-full flex-col items-center px-6 pt-6 pb-8">
      <div className="flex min-h-0 w-[560px] max-w-full flex-1 flex-col">
        {empty ? (
          <div className="flex flex-1 flex-col items-center justify-end pb-6">
            <h2 className="font-runde text-2xl font-semibold tracking-tight">
              What should we think about?
            </h2>
          </div>
        ) : (
          <div
            ref={threadRef}
            className="-mx-2 min-h-0 flex-1 overflow-y-auto px-2 pb-8 [scrollbar-width:none]"
            style={{ maskImage: FADE, WebkitMaskImage: FADE }}
          >
            <div className="flex min-h-full flex-col justify-end gap-8 pt-40">
              {messages.map((message) => {
                const { state, label } = statusFor(message);
                return (
                  <motion.div
                    key={message.id}
                    initial={{ opacity: 0, y: 12, filter: "blur(4px)" }}
                    animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
                    transition={SPRING}
                    className="flex flex-col gap-3"
                  >
                    <div className="flex flex-col items-end gap-1.5">
                      <p className="max-w-[80%] rounded-[20px] bg-black/[0.05] px-4 py-2.5 font-runde text-[15px] leading-6 whitespace-pre-wrap break-words dark:bg-white/[0.08]">
                        {message.prompt}
                      </p>
                      <span className="flex items-center gap-1.5 pr-2 font-runde text-xs font-medium text-[#868593]">
                        <span
                          className="size-1.5 rounded-full bg-current"
                          style={{ color: message.effort.color }}
                        />
                        {message.effort.label}
                      </span>
                    </div>
                    <div className="flex flex-col gap-1">
                      <AiStatus state={state} label={label} />
                      <AnimatePresence initial={false}>
                        {message.answer && (
                          <motion.p
                            initial={{ opacity: 0, height: 0 }}
                            animate={{ opacity: 1, height: "auto" }}
                            transition={SPRING}
                            className="font-runde text-[15px] leading-6 text-foreground/90"
                          >
                            {message.answer}
                          </motion.p>
                        )}
                      </AnimatePresence>
                    </div>
                  </motion.div>
                );
              })}
            </div>
          </div>
        )}

        <motion.div layout="position" transition={SPRING}>
          <PromptBox
            className="w-full"
            placeholder={PLACEHOLDERS}
            status={status}
            onSubmit={handleSubmit}
            onStop={handleStop}
            actions={
              <>
                <ActionButton label="Attach a file">
                  <Paperclip className="size-[18px]" />
                </ActionButton>
                <ActionButton label="Search the web">
                  <Globe className="size-[18px]" />
                </ActionButton>
              </>
            }
          />
        </motion.div>

        {empty && <div className="flex-1" />}
      </div>
    </div>
  );
}
