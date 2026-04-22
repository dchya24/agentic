# opencode Tools Reference

The `packages/opencode/src/tool/` directory contains **19 registered tools** (plus supporting utilities). Each tool follows the `Tool.define(id, Effect<Init>)` pattern with Zod schemas for parameters.

## Architecture

| File                    | Purpose                                                                                                        |
| ----------------------- | -------------------------------------------------------------------------------------------------------------- |
| `tool.ts`               | Core `Tool.define()` / `Tool.init()` functions, `Def`/`Context`/`ExecuteResult` types                          |
| `registry.ts`           | `ToolRegistry` service that registers all builtin tools, discovers plugin tools, and filters by model/provider |
| `schema.ts`             | `ToolID` branded schema                                                                                        |
| `truncate.ts`           | Output truncation service (max lines/bytes)                                                                    |
| `external-directory.ts` | External directory guard for file tools                                                                        |
| `mcp-exa.ts`            | Exa AI MCP client for web/code search                                                                          |

---

## Tool List

### bash

Executes a given bash command in a persistent shell session with optional timeout, ensuring proper handling and security measures. Supports git, npm, docker, etc. OS/Shell-aware (Bash, PowerShell).

| Parameter   | Type   | Required | Description                                                        |
| ----------- | ------ | -------- | ------------------------------------------------------------------ |
| command     | string | yes      | The command to execute                                             |
| timeout     | number | no       | Optional timeout in milliseconds                                   |
| workdir     | string | no       | The working directory to run the command in                        |
| description | string | yes      | Clear, concise description of what this command does in 5-10 words |

---

### read

Read a file or directory from the local filesystem. Supports text files, images, and PDFs. Returns up to 2000 lines by default with line numbers.

| Parameter | Type   | Required | Description                                            |
| --------- | ------ | -------- | ------------------------------------------------------ |
| filePath  | string | yes      | The absolute path to the file or directory to read     |
| offset    | number | no       | The line number to start reading from (1-indexed)      |
| limit     | number | no       | The maximum number of lines to read (defaults to 2000) |

---

### edit

Performs exact string replacements in files. Uses fuzzy matching strategies (trimmed lines, block anchors, whitespace normalization, escape normalization, etc.) for robust matching.

| Parameter  | Type    | Required | Description                                                    |
| ---------- | ------- | -------- | -------------------------------------------------------------- |
| filePath   | string  | yes      | The absolute path to the file to modify                        |
| oldString  | string  | yes      | The text to replace                                            |
| newString  | string  | yes      | The text to replace it with (must be different from oldString) |
| replaceAll | boolean | no       | Replace all occurrences of oldString (default false)           |

---

### write

Writes a file to the local filesystem. Overwrites existing files. Triggers LSP diagnostics after write.

| Parameter | Type   | Required | Description                                               |
| --------- | ------ | -------- | --------------------------------------------------------- |
| content   | string | yes      | The content to write to the file                          |
| filePath  | string | yes      | The absolute path to the file to write (must be absolute) |

---

### glob

Fast file pattern matching tool. Supports glob patterns like `**/*.js`. Returns matching file paths sorted by modification time. Limited to 100 results.

| Parameter | Type   | Required | Description                                  |
| --------- | ------ | -------- | -------------------------------------------- |
| pattern   | string | yes      | The glob pattern to match files against      |
| path      | string | no       | The directory to search in (defaults to cwd) |

---

### grep

Fast content search tool using regular expressions. Filter files by pattern with the include parameter. Returns file paths and line numbers with matches sorted by modification time.

| Parameter | Type   | Required | Description                                                       |
| --------- | ------ | -------- | ----------------------------------------------------------------- |
| pattern   | string | yes      | The regex pattern to search for in file contents                  |
| path      | string | no       | The directory to search in (defaults to cwd)                      |
| include   | string | no       | File pattern to include in the search (e.g. `*.js`, `*.{ts,tsx}`) |

---

### task

Launch a new agent to handle complex, multistep tasks autonomously. Creates a sub-session with the specified agent type. Dynamic description includes available agent types.

| Parameter     | Type   | Required | Description                                        |
| ------------- | ------ | -------- | -------------------------------------------------- |
| description   | string | yes      | A short (3-5 words) description of the task        |
| prompt        | string | yes      | The task for the agent to perform                  |
| subagent_type | string | yes      | The type of specialized agent to use for this task |
| task_id       | string | no       | Set to resume a previous task's subagent session   |
| command       | string | no       | The command that triggered this task               |

---

### question

Ask the user questions during execution. Gather preferences, clarify instructions, get decisions on implementation choices. Enabled only for app/cli/desktop clients.

| Parameter | Type  | Required | Description                                                                                                 |
| --------- | ----- | -------- | ----------------------------------------------------------------------------------------------------------- |
| questions | array | yes      | Questions to ask (array of Question.Prompt objects with question, header, options, custom, multiple fields) |

---

### webfetch

Fetches content from a specified URL. Converts HTML to markdown by default. Supports text, markdown, and html output formats. Handles images as attachments.

| Parameter | Type   | Required | Description                                                                              |
| --------- | ------ | -------- | ---------------------------------------------------------------------------------------- |
| url       | string | yes      | The URL to fetch content from                                                            |
| format    | enum   | no       | The format to return the content in (`text`, `markdown`, `html`). Defaults to `markdown` |
| timeout   | number | no       | Optional timeout in seconds (max 120)                                                    |

---

### websearch

Search the web using Exa AI. Performs real-time web searches with configurable result counts and live crawling modes.

| Parameter            | Type   | Required | Description                                            |
| -------------------- | ------ | -------- | ------------------------------------------------------ |
| query                | string | yes      | Websearch query                                        |
| numResults           | number | no       | Number of search results to return (default: 8)        |
| livecrawl            | enum   | no       | Live crawl mode: `fallback` or `preferred`             |
| type                 | enum   | no       | Search type: `auto`, `fast`, or `deep`                 |
| contextMaxCharacters | number | no       | Maximum characters for context string (default: 10000) |

---

### todowrite

Create and manage a structured task list for the current coding session. Track progress, organize complex tasks. Each todo has content, status, and priority.

| Parameter | Type  | Required | Description                                                            |
| --------- | ----- | -------- | ---------------------------------------------------------------------- |
| todos     | array | yes      | The updated todo list (array of `{content, status, priority}` objects) |

---

### skill

Load a specialized skill that provides domain-specific instructions and workflows. Injects the skill's instructions, resources, and file listing into the conversation.

| Parameter | Type   | Required | Description                                 |
| --------- | ------ | -------- | ------------------------------------------- |
| name      | string | yes      | The name of the skill from available_skills |

---

### apply_patch

Apply a patch to one or more files using a stripped-down, file-oriented diff format. Supports Add File, Delete File, and Update File (with optional rename) operations. Only available for GPT models.

| Parameter | Type   | Required | Description                                               |
| --------- | ------ | -------- | --------------------------------------------------------- |
| patchText | string | yes      | The full patch text that describes all changes to be made |

---

### codesearch

Search and get relevant context for any programming task using Exa Code API. Returns code examples, documentation, and API references.

| Parameter | Type   | Required | Description                                            |
| --------- | ------ | -------- | ------------------------------------------------------ |
| query     | string | yes      | Search query for APIs, Libraries, and SDKs             |
| tokensNum | number | no       | Number of tokens to return (1000-50000, default: 5000) |

---

### lsp

Interact with Language Server Protocol servers for code intelligence: goToDefinition, findReferences, hover, documentSymbol, workspaceSymbol, goToImplementation, prepareCallHierarchy, incomingCalls, outgoingCalls. Experimental (requires `OPENCODE_EXPERIMENTAL_LSP_TOOL` flag).

| Parameter | Type   | Required | Description                                                                                                                                                                          |
| --------- | ------ | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| operation | enum   | yes      | The LSP operation (`goToDefinition`, `findReferences`, `hover`, `documentSymbol`, `workspaceSymbol`, `goToImplementation`, `prepareCallHierarchy`, `incomingCalls`, `outgoingCalls`) |
| filePath  | string | yes      | The absolute or relative path to the file                                                                                                                                            |
| line      | number | yes      | The line number (1-based)                                                                                                                                                            |
| character | number | yes      | The character offset (1-based)                                                                                                                                                       |

---

### plan_exit

Exit plan agent mode. Asks the user if they want to switch to the build agent to start implementing the plan. Only available in CLI with experimental plan mode enabled.

| Parameter | Type | Required | Description |
| --------- | ---- | -------- | ----------- |
| _(none)_  |      |          |             |

---

### invalid

Internal tool for handling invalid tool call arguments. Not intended for direct use.

| Parameter | Type   | Required | Description                   |
| --------- | ------ | -------- | ----------------------------- |
| tool      | string | yes      | The tool name that was called |
| error     | string | yes      | The error message             |

---

## Additional (not registered in builtin list)

| Name      | File           | Notes                                                                          |
| --------- | -------------- | ------------------------------------------------------------------------------ |
| multiedit | `multiedit.ts` | Multiple sequential edits to a single file. Defined but not in builtin list    |
| mcp-exa   | `mcp-exa.ts`   | Helper module providing Exa AI MCP client used by `websearch` and `codesearch` |

---

## Conditional Availability

| Tool                     | Condition                                                              |
| ------------------------ | ---------------------------------------------------------------------- |
| `question`               | Only when `OPENCODE_ENABLE_QUESTION_TOOL` or client is app/cli/desktop |
| `apply_patch`            | Only for GPT models (excludes GPT-4 and OSS variants)                  |
| `edit` / `write`         | Hidden when `apply_patch` is active                                    |
| `lsp`                    | Requires `OPENCODE_EXPERIMENTAL_LSP_TOOL` flag                         |
| `plan_exit`              | Requires `OPENCODE_EXPERIMENTAL_PLAN_MODE` + CLI client                |
| `websearch`/`codesearch` | Requires opencode provider or `OPENCODE_ENABLE_EXA` flag               |
