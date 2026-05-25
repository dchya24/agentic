# How an AI Coding Agent Works: Conceptual Architecture

## The Core: One `while(true)` Loop

The entire agent is built on a single concept — a loop that keeps calling the LLM until the model decides it's done:

```
1. Send conversation history to LLM API
2. Receive response
3. Response has tool_use blocks?
   ├── YES → Execute tools → Push results back → Go to step 1
   └── NO  → Return text response to user
```

The model's **output becomes its own input** on the next turn. The loop doesn't make decisions — the **model** decides what to do next.

---

## Architecture Layers

### Layer 1: The Foundation

#### Agentic Loop

The `while(true)` core that drives everything. Call the LLM, check for tool calls, execute tools, push results, repeat. The loop has exit conditions:

| Condition | What happens |
|-----------|-------------|
| No tool calls | Model responded with just text. Normal exit. |
| Max turns reached | Safety limit to prevent infinite loops. |
| Context too long | Conversation history exceeds the model's context window. |
| API error | Rate limit, server error, etc. |
| User cancelled | Abort signal triggered. |

**Key insight:** Check the actual content blocks for tool calls, not the `stop_reason` metadata — it's unreliable.

#### Tools

Tools are how the model interacts with the real world. Each tool is a function the model can call. The model doesn't run tools itself — it can only *ask* the agent to run them. The agent executes the function and sends the result back.

Essential tools:

| Tool | Purpose |
|------|---------|
| **read_file** | Read file contents (with line numbers for reference) |
| **write_file** | Create or overwrite a file |
| **edit_file** | Replace specific text in a file |
| **list_files** | Find files in a directory |
| **search_files** | Search file contents with pattern matching |
| **run_command** | Execute a shell command |

Each tool has:
- **A definition** (name, description, input schema) — what the model sees
- **An implementation** — the actual function that does the work
- **Input validation** — the model can send bad arguments, validate before executing
- **Error handling** — never crash, always return something (even errors)

#### The Edit Tool

The edit tool uses **string replacement** — not AST parsing, not line numbers, not diff algorithms.

**Why string replacement:**
- Line numbers drift after edits
- AST parsing is language-specific (need different parsers for every language)
- LLMs are naturally good at outputting exact text matches

**The uniqueness rule:** The old string must appear exactly once in the file. Zero matches = not found. Multiple matches = ambiguous. Both are errors that tell the model to adjust.

**Staleness detection:** Track when the model last read each file. If the file was modified externally since then, reject the edit and tell the model to re-read it.

**Quote normalization:** Sometimes the model outputs curly quotes instead of straight quotes. Normalize both sides before matching.

---

### Layer 2: Intelligence

#### System Prompt

A set of instructions sent with every API call that defines how the model should behave. The model sees it fresh every turn.

**The three rules that matter most:**

1. **Read before edit** — Never guess what's in a file. Always read it first.
2. **Search before assuming** — Use list and search tools instead of guessing file paths.
3. **Understand before modifying** — Explore the codebase to understand patterns before making changes.

**The search funnel:** The system prompt creates a pattern where the model starts broad and narrows down:

```
list_files (broad)     →  12 files found
search_files (narrow)  →  3 files match
read_file (confirm)    →  1 file is the right one
edit_file (act)        →  done
```

Tool descriptions can also include behavioral hints that reinforce system prompt rules.

#### Context

LLMs have no memory. Every API call sends the **entire conversation history** — all user messages, assistant responses, tool calls, and tool results.

This is why follow-up questions work without re-reading files: the file contents are already in the conversation history from a previous turn.

**The cost problem:** Every turn sends all previous turns. File reads dump thousands of tokens that persist forever. A 50-turn conversation can easily reach 200,000 tokens. Each token costs money and fills the context window.

---

### Layer 3: Robustness

#### Context Compression

Three layers, ordered from cheapest to most expensive:

```
Messages → Layer 1: Truncate large results
         → Layer 2: Clear old results
         → Layer 3: Summarize conversation
         → Send to API
```

| Layer | What it does | Cost | When it fires |
|-------|-------------|------|---------------|
| **Truncate** | Cap tool results at a character limit. Keep the first chunk, add a truncation note. | Free (string slicing) | Every tool result |
| **Clear old** | Replace old tool results with `"[Cleared]"`. Keep only the N most recent results intact. | Free (string replacement) | Every turn |
| **Autocompact** | Ask the LLM to summarize the conversation. Replace old messages with the summary. | One extra API call | Only at ~80% of context window |

**Compact boundary:** After summarization, mark where the summary ends. Next time you compact, only summarize messages after the boundary — don't re-summarize the summary (information degrades with each cycle).

#### Permissions

A gate between "the model wants to do something" and "the thing actually happens."

Three decisions for every tool call:

| Decision | What happens |
|----------|-------------|
| **allow** | Tool runs immediately. No user prompt. |
| **ask** | User sees the tool call and approves or denies. |
| **deny** | Tool is rejected with an error. No user prompt. |

**Permission modes:**

| Mode | Behavior |
|------|----------|
| **default** | Ask for writes and commands, allow reads |
| **plan** | Read-only mode. Deny all writes and commands. |
| **yolo** | Allow everything (dangerous, for trusted tasks) |

**Permission rules:** Specific patterns that override default behavior. "Always allow `npm test`." "Never allow `rm`." Rules are checked before the tool's own decision. If a rule matches, it takes priority.

**Session persistence of rules:** When a user says "always allow this tool," the rule lasts for the session. Production agents persist rules at three levels: session (memory), project settings (shared with team), user settings (global).

When denied, the model gets an error back. It doesn't crash — it can try a different approach.

---

### Layer 4: Advanced Features

#### Subagents

A subagent is the **same agentic loop** called with a **fresh, isolated conversation history**. The parent agent only sees the final text summary the subagent returns.

**Why:** Complex tasks have subtasks that don't need each other's context. Reading 20 files for API exploration shouldn't pollute the context for writing tests.

```
Main agent:  "Update the API, then the tests, then the docs"
  ├── Subagent 1: Explores API files → returns summary
  ├── Subagent 2: Explores test files → returns summary
  └── Subagent 3: Updates docs based on summaries
```

**Shared vs Isolated:**

| Component | Shared or Isolated | Why |
|-----------|-------------------|-----|
| Messages | Isolated | Keep contexts separate |
| Tools | Shared | Both use the same tools |
| Abort signal | Linked | Parent cancel kills child too |
| Permission rules | Shared | User-approved rules apply everywhere |

Subagents have tighter turn limits (10-15 turns) than the main agent (20-30) to prevent runaway token usage.

#### Streaming

Instead of waiting for the complete response, process events as they arrive:

- **Text deltas** → Print immediately to the terminal
- **Tool input deltas** → Buffer the pieces, parse the complete JSON when the block finishes
- **Thinking blocks** → Model reasoning before responding (optional to display)

The key change: iterate over stream events instead of waiting for a complete response. Text appears token by token. The total time is the same, but the perceived speed is much better.

#### Concurrent Tool Execution

When the model calls multiple tools in one response, partition them into batches:

- **Read-only tools** (read, list, search) → Run in parallel
- **State-changing tools** (edit, write, command) → Run alone, sequentially

```
Tool calls:  read(A), read(B), search(X), edit(C), read(D)

Batch 1 (parallel):  read(A), read(B), search(X)
Batch 2 (serial):    edit(C)
Batch 3 (parallel):  read(D)
```

Results must be returned in the **original order** regardless of which finishes first.

#### Web Access

Two tools:

1. **Fetch** — Give a URL, get back page content as clean text
2. **Search** — Give a query, get back titles and URLs

**HTML → Clean text:** Web pages contain navigation, ads, scripts. Convert HTML to structured text (preserving headings, code blocks, links) rather than stripping tags.

**Secondary model extraction:** Instead of sending 50,000 characters of page content to the main model, use a cheap/fast model to extract the relevant section. The main model gets a focused 200-500 token summary.

**Caching:** Cache fetched URLs in memory for the session to avoid re-fetching.

**Domain-based permissions:** Pre-approved domains (documentation sites) auto-allow. Unknown domains ask the user.

#### Persistence

Three forms of persistence:

**1. Conversation history (session resume)**
- Save messages as they happen (append-only, one message per line)
- Resume by loading all messages back into the conversation history
- Commands like `/resume` and `/new` for session management
- Each session gets a unique ID with metadata for listing previous sessions

**2. Project instructions**
- A markdown file in the project root (like `AGENT.md`)
- Read automatically on every startup
- Contains project-specific rules: coding conventions, test commands, directory structure
- Shared with the team via version control

**3. Memory across sessions**
- A file the agent can read and write to persist notes
- User preferences, project decisions, important context
- Loaded into the system prompt every session
- Grows over time with accumulated knowledge

---

## The Complete Data Flow

```
User types message
       │
       ▼
┌─── REPL ─────────────────────────────────────┐
│  Push to conversation history                 │
│  Call agentLoop(conversationHistory)          │
│       │                                      │
│       ▼                                      │
│  ┌── Compress context ──────────────────┐    │
│  │  1. Truncate large results           │    │
│  │  2. Clear old tool results           │    │
│  │  3. Autocompact if near limit        │    │
│  └──────────────────────────────────────┘    │
│       │                                      │
│       ▼                                      │
│  ┌── Build system prompt ───────────────┐    │
│  │  Base instructions                   │    │
│  │  + Project instructions (AGENT.md)   │    │
│  │  + Memory from previous sessions     │    │
│  └──────────────────────────────────────┘    │
│       │                                      │
│       ▼                                      │
│  ┌── Call LLM API ──────────────────────┐    │
│  │  system: system prompt               │    │
│  │  tools: [read, edit, search, ...]    │    │
│  │  messages: conversation history      │    │
│  └──────────────────────────────────────┘    │
│       │                                      │
│       ▼                                      │
│  Response has tool_use? ──YES──► Permission? │
│       │                        │   │         │
│       │                    allow  ask/deny    │
│       │                        │   │         │
│       NO                  Execute tool        │
│       │                   Push result         │
│       ▼                        │              │
│  Return text to user ◄─────────┘  (loop)     │
└──────────────────────────────────────────────┘
```

---

## How a Real Task Flows

User types: **"Change the button color to red"**

```
Turn 1: [tool] list_files("src/")         → discovers project structure
Turn 2: [tool] search_files("button")     → narrows to Button component
Turn 3: [tool] read_file("Button.tsx")    → sees: className="bg-blue-500..."
Turn 4: [tool] edit_file({                → string.replace() in action
          old: "bg-blue-500...hover:bg-blue-600",
          new: "bg-red-500...hover:bg-red-600"
        })
Turn 5: [text] "Done. Changed button to red"  → loop exits (no more tools)
```

The model made **all decisions autonomously** — which files to read, what to edit, when it's done.

---

## Production Extensions (Beyond This Tutorial)

| Feature | Concept |
|---------|---------|
| **MCP (Model Context Protocol)** | Standard for dynamically connecting external tools and data sources at runtime |
| **LSP Integration** | Language server provides type errors, lint warnings, go-to-definition — instant feedback after edits |
| **Git Integration** | Track changes, show diffs, create commits, auto-revert on failure |
| **Multi-model Strategy** | Powerful model for decisions, cheap/fast model for summarization and extraction |
| **Prompt Caching** | Cache the processed system prompt and conversation prefix on the server side |
| **File Watching** | Detect external file changes in real-time, update staleness cache |
| **TUI (Terminal UI)** | Colors, spinners, syntax highlighting, interactive permission dialogs |

---

## The One-Sentence Summary

> **The loop is dumb. The model is smart. Your job is to give the model tools, keep the loop running, and add guardrails so it doesn't do anything dangerous.**

Everything — tools, system prompts, compression, permissions, subagents, streaming, concurrency, persistence — is just infrastructure around that single `while(true)` loop.
