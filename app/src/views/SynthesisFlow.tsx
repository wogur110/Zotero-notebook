// Multi-paper synthesis & Q&A over a set of papers (a whole collection or an
// ad-hoc selection). Context is metadata + abstracts only, assembled
// backend-side and capped; answers stream live and are always in English.

import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import type { ChatMessage, Item, ProviderId } from "../types";
import {
  IconAlert,
  IconArrowRight,
  IconChevronRight,
  IconLibrary,
  IconLoader,
  IconSparkles,
} from "../components/icons";

interface Props {
  /** The papers in scope (current view, or the selected subset). */
  items: Item[];
  scopeLabel: string;
  defaultProvider: ProviderId;
  onClose: () => void;
}

/** Keep in sync with MAX_SYNTHESIS_PAPERS in core/src/llm/provider.rs. */
const MAX_PAPERS = 50;

const PRESETS: { label: string; prompt: string }[] = [
  {
    label: "Overview of these papers",
    prompt:
      "Give me a structured overview of these papers: the main themes, the methods used, and how they relate to one another.",
  },
  {
    label: "Compare the methods",
    prompt:
      "Compare the methods and approaches across these papers. Where do they agree, differ, or build on each other?",
  },
  {
    label: "Themes & open gaps",
    prompt:
      "What are the common themes across these papers, and what open problems or gaps do they collectively point to?",
  },
];

export default function SynthesisFlow({
  items,
  scopeLabel,
  defaultProvider,
  onClose,
}: Props) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [streamText, setStreamText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const capped = items.length > MAX_PAPERS;
  const usedCount = Math.min(items.length, MAX_PAPERS);

  useEffect(() => {
    // scrollIntoView is missing under jsdom in tests.
    bottomRef.current?.scrollIntoView?.({ block: "end" });
  }, [messages, streamText]);

  const send = async (raw: string) => {
    const question = raw.trim();
    if (!question || busy || items.length === 0) return;
    const history: ChatMessage[] = [
      ...messages,
      { role: "user", content: question },
    ];
    setMessages(history);
    setInput("");
    setBusy(true);
    setError(null);
    setStreamText("");

    const un = await api.onSynthesisDelta((d) => {
      setStreamText((prev) => prev + d.delta);
    });
    try {
      const answer = await api.chatWithItems(
        items.map((i) => i.key),
        history,
        defaultProvider,
      );
      setMessages([...history, { role: "assistant", content: answer }]);
    } catch (e) {
      setError(api.errorMessage(e));
      // Drop the unanswered question so a retry doesn't double it up.
      setMessages(messages);
    } finally {
      un();
      setStreamText("");
      setBusy(false);
      inputRef.current?.focus();
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center gap-2 border-b border-edge px-6 py-3">
        <button
          className="btn-ghost h-8 px-2! text-xs"
          onClick={onClose}
          aria-label="Back to library"
        >
          <span className="rotate-180">
            <IconChevronRight size={14} />
          </span>
          Back
        </button>
        <div className="min-w-0 flex-1">
          <h1 className="flex items-center gap-1.5 truncate text-sm font-semibold">
            <span className="text-accent">
              <IconLibrary size={15} />
            </span>
            Synthesize
            <span className="font-normal text-muted">· {scopeLabel}</span>
          </h1>
          <p className="truncate text-xs text-faint">
            {usedCount} {usedCount === 1 ? "paper" : "papers"} · metadata +
            abstracts
            {capped && (
              <span className="text-warn">
                {" "}
                · using the first {MAX_PAPERS} of {items.length}
              </span>
            )}
          </p>
        </div>
      </div>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-6 py-4">
        {messages.length === 0 && !busy ? (
          <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
            <span className="flex h-11 w-11 items-center justify-center rounded-full bg-accent-soft text-accent">
              <IconSparkles size={20} />
            </span>
            <p className="text-sm font-medium">
              Ask across {usedCount} {usedCount === 1 ? "paper" : "papers"}
            </p>
            <p className="max-w-sm text-xs text-muted">
              Grounded in each paper's metadata and abstract. Pick a starting
              point or ask your own question (e.g. "which of these use
              diffusion?").
            </p>
            <div className="flex flex-col gap-1.5">
              {PRESETS.map((p) => (
                <button
                  key={p.label}
                  className="btn-secondary py-1! text-xs"
                  onClick={() => void send(p.prompt)}
                >
                  {p.label}
                </button>
              ))}
            </div>
          </div>
        ) : (
          <>
            {messages.map((m, i) =>
              m.role === "user" ? (
                <div key={i} className="flex justify-end">
                  <p className="max-w-[85%] rounded-lg rounded-br-sm bg-accent-soft px-3 py-2 text-sm leading-relaxed">
                    {m.content}
                  </p>
                </div>
              ) : (
                <p
                  key={i}
                  className="max-w-[92%] whitespace-pre-wrap text-sm leading-relaxed"
                >
                  {m.content}
                </p>
              ),
            )}
            {busy && (
              <p className="max-w-[92%] whitespace-pre-wrap text-sm leading-relaxed">
                {streamText}
                <span className="ml-1 inline-flex align-middle text-faint">
                  <IconLoader size={12} />
                </span>
              </p>
            )}
            {error && (
              <p className="flex items-start gap-1.5 rounded-md bg-danger-soft px-3 py-2 text-xs text-danger">
                <IconAlert size={13} className="mt-0.5 shrink-0" />
                <span>{error} — API keys can be added in Settings.</span>
              </p>
            )}
            <div ref={bottomRef} />
          </>
        )}
      </div>

      <div className="border-t border-edge px-6 py-3">
        <form
          className="flex items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void send(input);
          }}
        >
          <input
            ref={inputRef}
            className="input"
            placeholder={
              busy ? "Thinking…" : "Ask across these papers…"
            }
            value={input}
            disabled={busy}
            onChange={(e) => setInput(e.target.value)}
            autoFocus
          />
          <button
            type="submit"
            className="btn-primary h-[34px] w-[38px] shrink-0 px-0!"
            disabled={busy || !input.trim()}
            aria-label="Send"
          >
            {busy ? <IconLoader size={15} /> : <IconArrowRight size={15} />}
          </button>
        </form>
        <p className="mt-1.5 text-[11px] text-faint">
          Each question sends the papers' metadata and abstracts to the AI
          provider · output language is set in Settings
        </p>
      </div>
    </div>
  );
}
