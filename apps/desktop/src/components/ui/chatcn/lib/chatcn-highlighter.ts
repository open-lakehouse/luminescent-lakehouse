// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import type { GrammarState, HighlighterCore, ThemedToken, ThemeRegistration } from "shiki/core"

/* -------------------------------------------------------------------------------------------------
 * Roles and theme
 * -----------------------------------------------------------------------------------------------*/

export type ChatcnTokenRole =
  | "plain"
  | "keyword"
  | "comment"
  | "string"
  | "constant"
  | "function"
  | "type"
  | "tag"
  | "attribute"
  | "punctuation"

export interface ChatcnToken {
  content: string
  role: ChatcnTokenRole
}

export type ChatcnTokenLine = ChatcnToken[]

/** Each role reads one syntax variable. Keywords lead in red, comments and punctuation recede. */
export const chatcnTokenClass: Record<ChatcnTokenRole, string> = {
  plain: "text-chatcn-syntax-plain",
  keyword: "text-chatcn-syntax-keyword",
  comment: "text-chatcn-syntax-comment",
  string: "text-chatcn-syntax-string",
  constant: "text-chatcn-syntax-constant",
  function: "text-chatcn-syntax-function",
  type: "text-chatcn-syntax-type",
  tag: "text-chatcn-syntax-tag",
  attribute: "text-chatcn-syntax-attribute",
  punctuation: "text-chatcn-syntax-punctuation",
}

// Shiki needs a color per scope. The "color" is the role name, read back in `toLine`.
const role = (name: ChatcnTokenRole) => `var(--chatcn-code-${name})`
const ROLE = /--chatcn-code-([a-z]+)/

const chatcnTheme: ThemeRegistration = {
  name: "chatcn",
  type: "light",
  colors: { "editor.foreground": role("plain"), "editor.background": "transparent" },
  tokenColors: [
    {
      scope: ["comment", "punctuation.definition.comment", "string.quoted.docstring"],
      settings: { foreground: role("comment") },
    },
    {
      scope: [
        "string",
        "string.template",
        "string.regexp",
        "markup.inline.raw",
        "markup.fenced_code",
      ],
      settings: { foreground: role("string") },
    },
    {
      scope: [
        "constant.numeric",
        "constant.language",
        "constant.character",
        "constant.other",
        "support.constant",
        "variable.other.enummember",
      ],
      settings: { foreground: role("constant") },
    },
    {
      scope: [
        "keyword",
        "storage",
        "storage.type",
        "storage.modifier",
        "keyword.control",
        "variable.language",
        "markup.heading",
        "markup.bold",
      ],
      settings: { foreground: role("keyword") },
    },
    {
      scope: [
        "keyword.operator",
        "keyword.operator.assignment",
        "keyword.operator.arithmetic",
        "storage.type.function.arrow",
      ],
      settings: { foreground: role("punctuation") },
    },
    {
      scope: [
        "keyword.operator.new",
        "keyword.operator.expression",
        "keyword.operator.logical.python",
        "keyword.operator.typeof",
        "keyword.operator.instanceof",
      ],
      settings: { foreground: role("keyword") },
    },
    {
      scope: [
        "entity.name.function",
        "support.function",
        "meta.function-call entity.name.function",
        "entity.name.method",
      ],
      settings: { foreground: role("function") },
    },
    {
      scope: [
        "entity.name.type",
        "entity.name.class",
        "entity.other.inherited-class",
        "support.type",
        "support.class",
        "meta.type.annotation",
        "entity.name.namespace",
      ],
      settings: { foreground: role("type") },
    },
    {
      scope: ["entity.name.tag", "support.class.component", "meta.tag.sgml"],
      settings: { foreground: role("tag") },
    },
    {
      scope: [
        "entity.other.attribute-name",
        "meta.object-literal.key",
        "variable.other.property",
        "variable.parameter",
      ],
      settings: { foreground: role("attribute") },
    },
    {
      scope: [
        "punctuation",
        "meta.brace",
        "punctuation.definition.tag",
        "punctuation.separator",
        "punctuation.terminator",
        "punctuation.accessor",
        "meta.delimiter",
      ],
      settings: { foreground: role("punctuation") },
    },
    {
      scope: ["support.type.property-name", "meta.mapping.key", "string.unquoted.argument"],
      settings: { foreground: role("plain") },
    },
    {
      scope: ["punctuation.definition.string", "punctuation.definition.template-expression"],
      settings: { foreground: role("string") },
    },
    {
      scope: ["markup.deleted", "markup.inserted", "markup.changed"],
      settings: { foreground: role("plain") },
    },
  ],
}

/* -------------------------------------------------------------------------------------------------
 * Languages
 * -----------------------------------------------------------------------------------------------*/

const ALIASES: Record<string, string> = {
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  sh: "bash",
  shell: "bash",
  zsh: "bash",
  console: "bash",
  shellsession: "bash",
  py: "python",
  rb: "ruby",
  rs: "rust",
  golang: "go",
  yml: "yaml",
  md: "markdown",
  mdx: "mdx",
  "c++": "cpp",
  cs: "csharp",
  "c#": "csharp",
  kt: "kotlin",
  ps1: "powershell",
  dockerfile: "docker",
  tf: "hcl",
  jsonc: "jsonc",
  plaintext: "text",
  txt: "text",
  plain: "text",
}

const LABELS: Record<string, string> = {
  typescript: "TypeScript",
  tsx: "TSX",
  javascript: "JavaScript",
  jsx: "JSX",
  json: "JSON",
  jsonc: "JSON",
  bash: "Bash",
  python: "Python",
  rust: "Rust",
  go: "Go",
  ruby: "Ruby",
  html: "HTML",
  css: "CSS",
  scss: "SCSS",
  sql: "SQL",
  yaml: "YAML",
  toml: "TOML",
  markdown: "Markdown",
  mdx: "MDX",
  diff: "Diff",
  docker: "Dockerfile",
  graphql: "GraphQL",
  swift: "Swift",
  kotlin: "Kotlin",
  java: "Java",
  cpp: "C++",
  c: "C",
  csharp: "C#",
  php: "PHP",
  powershell: "PowerShell",
  hcl: "HCL",
  prisma: "Prisma",
  vue: "Vue",
  svelte: "Svelte",
  text: "Plain text",
}

/** Lower-cases and resolves common aliases: `ts` to `typescript`, `sh` to `bash`. */
export function resolveLanguage(lang?: string | null): string {
  const id = (lang ?? "").trim().toLowerCase()
  if (!id) return "text"
  return ALIASES[id] ?? id
}

/** A readable label for a header: `TypeScript`, `Bash`, or the id as written. */
export function languageLabel(lang?: string | null): string {
  const id = resolveLanguage(lang)
  return LABELS[id] ?? id
}

const PLAIN = new Set(["text", "txt", "plain", "plaintext", "ansi", "log", "output"])

/* -------------------------------------------------------------------------------------------------
 * Loading
 * -----------------------------------------------------------------------------------------------*/

let core: Promise<HighlighterCore> | undefined
let ready: HighlighterCore | undefined
const loaded = new Set<string>()
const pending = new Map<string, Promise<boolean>>()
const missing = new Set<string>()

function getCore() {
  core ??= (async () => {
    const [{ createHighlighterCore }, { createJavaScriptRegexEngine }] = await Promise.all([
      import("shiki/core"),
      import("shiki/engine/javascript"),
    ])
    const highlighter = await createHighlighterCore({
      themes: [chatcnTheme],
      langs: [],
      engine: createJavaScriptRegexEngine({ forgiving: true }),
    })
    ready = highlighter
    return highlighter
  })()
  return core
}

/** True once `lang` can be tokenized synchronously. */
export function isLanguageReady(lang?: string | null) {
  const id = resolveLanguage(lang)
  return PLAIN.has(id) || (ready !== undefined && loaded.has(id))
}

/** True when the language has no grammar (loading failed or it is plain text). */
export function isPlainLanguage(lang?: string | null) {
  const id = resolveLanguage(lang)
  return PLAIN.has(id) || missing.has(id)
}

/**
 * Loads Shiki and one grammar. Resolves `true` when the language can be highlighted, `false`
 * when it is unknown (the block then stays plain, which is the right fallback).
 */
export function loadLanguage(lang?: string | null): Promise<boolean> {
  const id = resolveLanguage(lang)
  if (PLAIN.has(id) || missing.has(id)) return Promise.resolve(false)
  if (loaded.has(id) && ready) return Promise.resolve(true)
  const existing = pending.get(id)
  if (existing) return existing
  const task = (async () => {
    try {
      const [highlighter, { bundledLanguages, bundledLanguagesAlias }] = await Promise.all([
        getCore(),
        import("shiki/langs"),
      ])
      const input =
        (bundledLanguages as Record<string, unknown>)[id] ??
        (bundledLanguagesAlias as Record<string, unknown>)[id]
      if (!input) {
        missing.add(id)
        return false
      }
      await highlighter.loadLanguage(input as Parameters<HighlighterCore["loadLanguage"]>[0])
      loaded.add(id)
      return true
    } catch {
      missing.add(id)
      return false
    } finally {
      pending.delete(id)
    }
  })()
  pending.set(id, task)
  return task
}

/* -------------------------------------------------------------------------------------------------
 * Tokenizing
 * -----------------------------------------------------------------------------------------------*/

function toLine(tokens: ThemedToken[] | undefined): ChatcnTokenLine {
  const line: ChatcnTokenLine = []
  for (const token of tokens ?? []) {
    const match = token.color ? ROLE.exec(token.color) : null
    const tokenRole = (match?.[1] as ChatcnTokenRole | undefined) ?? "plain"
    const last = line.at(-1)
    // Merge neighbours with the same role: fewer spans, same look.
    if (last && last.role === tokenRole) last.content += token.content
    else line.push({ content: token.content, role: tokenRole })
  }
  return line
}

/** Plain lines, used before the grammar loads and for unknown languages. */
export function plainLines(code: string): ChatcnTokenLine[] {
  return code.split("\n").map((content) => (content ? [{ content, role: "plain" as const }] : []))
}

/** Tokenizes a whole snippet. Returns `null` until the language is ready. */
export function tokenize(code: string, lang?: string | null): ChatcnTokenLine[] | null {
  const id = resolveLanguage(lang)
  if (PLAIN.has(id) || missing.has(id)) return plainLines(code)
  if (!ready || !loaded.has(id)) return null
  try {
    return ready.codeToTokens(code, { lang: id, theme: "chatcn" }).tokens.map(toLine)
  } catch {
    return plainLines(code)
  }
}

/**
 * Incremental tokenizer for streaming code. Finished lines are tokenized once and kept (same
 * array identity, so memoized line components never re-render); only the line being written is
 * re-tokenized on each update, continuing from the saved grammar state.
 */
export interface ChatcnLineTokenizer {
  update(code: string): ChatcnTokenLine[] | null
  reset(): void
}

export function createLineTokenizer(lang?: string | null): ChatcnLineTokenizer {
  const id = resolveLanguage(lang)
  let stableCode = ""
  let stable: ChatcnTokenLine[] = []
  let state: GrammarState | undefined

  const reset = () => {
    stableCode = ""
    stable = []
    state = undefined
  }

  const run = (code: string) => {
    if (!ready) return { tokens: [] as ThemedToken[][], grammarState: undefined }
    return ready.codeToTokens(code, {
      lang: id,
      theme: "chatcn",
      ...(state ? { grammarState: state } : {}),
    })
  }

  return {
    reset,
    update(code) {
      if (PLAIN.has(id) || missing.has(id)) return plainLines(code)
      if (!ready || !loaded.has(id)) return null
      try {
        if (!code.startsWith(stableCode)) reset()
        const rest = code.slice(stableCode.length)
        const lastBreak = rest.lastIndexOf("\n")
        if (lastBreak >= 0) {
          const done = rest.slice(0, lastBreak)
          const result = run(done)
          stable = [...stable, ...result.tokens.map(toLine)]
          state = result.grammarState
          stableCode += rest.slice(0, lastBreak + 1)
        }
        const tail = code.slice(stableCode.length)
        const tailLine = tail ? toLine(run(tail).tokens[0]) : []
        return [...stable, tailLine]
      } catch {
        return plainLines(code)
      }
    },
  }
}
