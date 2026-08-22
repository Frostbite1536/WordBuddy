import { useState, useEffect, useCallback } from "react";
import {
  ArrowLeft,
  MessageSquare,
  Trash2,
  User,
  Bot,
  ChevronDown,
  ChevronRight,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { open } from "@tauri-apps/plugin-shell";
import { useApp } from "../contexts/app.context";
import {
  loadConversations,
  loadMessages,
  deleteConversation,
  type ConversationRow,
  type MessageRow,
} from "../lib/db";
import { confirmExternalLink } from "../lib/safeOpen";

export default function History() {
  const { messages, clearMessages, setCurrentPage } = useApp();
  const [conversations, setConversations] = useState<ConversationRow[]>([]);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [expandedMessages, setExpandedMessages] = useState<MessageRow[]>([]);
  const [loading, setLoading] = useState(true);

  // Load conversations from SQLite on mount
  useEffect(() => {
    loadConversations()
      .then(setConversations)
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  const handleExpand = useCallback(
    async (convId: string) => {
      if (expandedId === convId) {
        setExpandedId(null);
        setExpandedMessages([]);
        return;
      }
      try {
        const msgs = await loadMessages(convId);
        setExpandedId(convId);
        setExpandedMessages(msgs);
      } catch {
        // Load failed
      }
    },
    [expandedId],
  );

  const handleDelete = useCallback(
    async (convId: string) => {
      try {
        await deleteConversation(convId);
        setConversations((prev) => prev.filter((c) => c.id !== convId));
        if (expandedId === convId) {
          setExpandedId(null);
          setExpandedMessages([]);
        }
      } catch {
        // Delete failed
      }
    },
    [expandedId],
  );

  // Also show current session messages (not yet persisted)
  const sessionMessages = messages.filter((m) => m.id !== "streaming");

  return (
    <div className="h-full bg-background-primary text-zinc-100 overflow-y-auto">
      <div className="max-w-lg mx-auto p-6 space-y-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <button
              onClick={() => setCurrentPage("chat")}
              aria-label="Back to chat"
              className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-400"
            >
              <ArrowLeft size={18} />
            </button>
            <h1 className="text-lg font-heading font-semibold">History</h1>
          </div>
          {(conversations.length > 0 || sessionMessages.length > 0) && (
            <button
              onClick={clearMessages}
              className="text-xs text-zinc-500 hover:text-red-400 flex items-center gap-1"
            >
              <Trash2 size={12} /> Clear session
            </button>
          )}
        </div>

        {/* Current session */}
        {sessionMessages.length > 0 && (
          <div className="space-y-2">
            <h2 className="text-xs text-zinc-500 uppercase tracking-wider">
              Current Session
            </h2>
            {sessionMessages.map((msg, i) => (
              <div
                key={msg.id}
                className={`p-3 rounded-lg border transition-colors ${
                  msg.role === "user"
                    ? "bg-accent/5 border-accent/20"
                    : "bg-zinc-900 border-zinc-800"
                }`}
              >
                <div className="flex items-center gap-2 mb-1">
                  {msg.role === "user" ? (
                    <User size={12} className="text-accent" />
                  ) : (
                    <Bot size={12} className="text-zinc-400" />
                  )}
                  <span className="text-[10px] text-zinc-600 uppercase">
                    {msg.role === "user" ? "You" : "WordBuddy"}
                  </span>
                  <span className="text-[10px] text-zinc-700 ml-auto">
                    {new Date(msg.timestamp).toLocaleTimeString()}
                  </span>
                </div>
                {msg.role === "user" ? (
                  <p className="text-sm text-zinc-300 line-clamp-3">
                    {msg.content}
                  </p>
                ) : (
                  <div className="prose text-zinc-300 text-sm max-w-full line-clamp-6">
                    <ReactMarkdown
                      remarkPlugins={[remarkGfm]}
                      components={{
                        a({ href, children, ...props }) {
                          return (
                            <a
                              {...props}
                              href="#"
                              title={href}
                              onClick={(e) => {
                                e.preventDefault();
                                if (!href) return;
                                if (confirmExternalLink(href)) {
                                  open(href).catch(() => {});
                                }
                              }}
                              className="text-accent hover:underline cursor-pointer"
                            >
                              {children}
                            </a>
                          );
                        },
                      }}
                    >
                      {msg.content}
                    </ReactMarkdown>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}

        {/* Saved conversations from SQLite */}
        {loading ? (
          <div className="text-center py-12 text-zinc-600">
            <p className="text-sm">Loading history...</p>
          </div>
        ) : conversations.length === 0 && sessionMessages.length === 0 ? (
          <div className="text-center py-12 text-zinc-600">
            <MessageSquare size={32} className="mx-auto mb-3 opacity-50" />
            <p className="text-sm">No conversations yet</p>
            <p className="text-xs mt-1">Ask a question to get started</p>
          </div>
        ) : conversations.length > 0 ? (
          <div className="space-y-2">
            <h2 className="text-xs text-zinc-500 uppercase tracking-wider">
              Saved Conversations
            </h2>
            {conversations.map((conv) => (
              <div
                key={conv.id}
                className="rounded-lg border border-zinc-800 bg-zinc-900 overflow-hidden"
              >
                <div className="flex items-center justify-between p-3">
                  <button
                    onClick={() => handleExpand(conv.id)}
                    className="flex items-center gap-2 text-sm text-zinc-300 hover:text-white"
                  >
                    {expandedId === conv.id ? (
                      <ChevronDown size={14} />
                    ) : (
                      <ChevronRight size={14} />
                    )}
                    <span>
                      {new Date(conv.created_at).toLocaleDateString()}{" "}
                      {new Date(conv.created_at).toLocaleTimeString()}
                    </span>
                    {conv.program && (
                      <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-accent/15 text-accent capitalize">
                        {conv.program.replaceAll("_", " ")}
                      </span>
                    )}
                  </button>
                  <button
                    onClick={() => handleDelete(conv.id)}
                    className="p-1 text-zinc-600 hover:text-red-400"
                    title="Delete conversation"
                    aria-label="Delete conversation"
                  >
                    <Trash2 size={12} />
                  </button>
                </div>

                {expandedId === conv.id && expandedMessages.length > 0 && (
                  <div className="border-t border-zinc-800 p-3 space-y-2">
                    {expandedMessages.map((msg) => (
                      <div
                        key={msg.id}
                        className={`p-2 rounded text-sm ${
                          msg.role === "user"
                            ? "bg-accent/5 text-accent"
                            : "text-zinc-300"
                        }`}
                      >
                        <div className="flex items-center gap-1.5 mb-1">
                          {msg.role === "user" ? (
                            <User size={10} className="text-accent" />
                          ) : (
                            <Bot size={10} className="text-zinc-400" />
                          )}
                          <span className="text-[9px] text-zinc-600 uppercase">
                            {msg.role === "user" ? "You" : "WordBuddy"}
                          </span>
                        </div>
                        {msg.role === "user" ? (
                          <p className="line-clamp-3">{msg.content}</p>
                        ) : (
                          <div className="prose text-zinc-300 text-sm max-w-full line-clamp-10">
                            <ReactMarkdown
                              remarkPlugins={[remarkGfm]}
                              components={{
                                a({ href, children, ...props }) {
                                  return (
                                    <a
                                      {...props}
                                      href="#"
                                      onClick={(e) => {
                                        e.preventDefault();
                                        if (href) open(href).catch(() => {});
                                      }}
                                      className="text-accent hover:underline cursor-pointer"
                                    >
                                      {children}
                                    </a>
                                  );
                                },
                              }}
                            >
                              {msg.content}
                            </ReactMarkdown>
                          </div>
                        )}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}
