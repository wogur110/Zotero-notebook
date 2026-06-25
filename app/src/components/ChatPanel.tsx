// Per-paper "Ask AI" chat. Context (metadata + extracted PDF text) is
// assembled backend-side; this panel keeps the conversation state for the
// lifetime of the modal and renders the streaming answer live.

import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import type { ChatMessage, Item, ProviderId } from "../types";
import { IconAlert, IconArrowRight, IconLoader, IconSparkles } from "./icons";

interface Props {
  item: Item;
  defaultProvider: ProviderId;
}

const SUGGESTIONS = [
  "What problem does this paper solve?",
  "Explain the method in simple terms.",
  "What are the main results and limitations?",
];

export default function ChatPanel({ item, defaultProvider }: Props) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [streamText, setStreamText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    // scrollIntoView is missing under jsdom in tests.
    bottomRef.current?.scrollIntoView?.({ block: "end" });
  }, [messages, streamText]);

  const send = async (raw: string) => {
    const question = raw.trim();
    if (!question || busy) return;
    const history: ChatMessage[] = [
      ...messages,
      { role: "user", content: question },
    ];
    setMessages(history);
    setInput("");
    setBusy(true);
    setError(null);
    setStreamText("");

    const un = await api.onChatDelta((d) => {
      if (d.itemKey === item.key) {
        setStreamText((prev) => prev + d.delta);
      }
    });
    try {
      const answer = await api.chatWithItem(item.key, history, defaultProvider);
      setMessages([...history, { role: "assistant", content: answer }]);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      un();
      setStreamText("");
      setBusy(false);
      inputRef.current?.focus();
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-6 py-4">
        {messages.length === 0 && !busy ? (
          <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
            <span className="flex h-11 w-11 items-center justify-center rounded-full bg-accent-soft text-accent">
              <IconSparkles size={20} />
            </span>
            <p className="text-sm font-medium">Ask anything about this paper</p>
            <p className="max-w-sm text-xs text-muted">
              Answers are grounded in the paper's extracted text (when
              available); the output language is set in Settings.
            </p>
            <div className="flex flex-col gap-1.5">
              {SUGGESTIONS.map((s) => (
                <button
                  key={s}
                  className="btn-secondary py-1! text-xs"
                  onClick={() => void send(s)}
                >
                  {s}
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
            placeholder={busy ? "Answering…" : "Ask about this paper…"}
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
          Each question sends the paper's text to the AI provider · output
          language is set in Settings
        </p>
      </div>
    </div>
  );
}
