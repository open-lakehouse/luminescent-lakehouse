// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
export interface PartialJsonResult {
  /** Best-effort value, `undefined` when nothing could be parsed yet. */
  value: unknown
  /** True when the text held one finished JSON value (trailing commas tolerated). */
  complete: boolean
}

const NONE = Symbol("none")
type Parsed = unknown | typeof NONE

const LITERALS: Record<string, [string, unknown]> = {
  t: ["true", true],
  f: ["false", false],
  n: ["null", null],
}

const ESCAPES: Record<string, string> = {
  '"': '"',
  "\\": "\\",
  "/": "/",
  b: "\b",
  f: "\f",
  n: "\n",
  r: "\r",
  t: "\t",
}

const NUMBER_CHAR = /[-+.eE0-9]/

class Parser {
  i = 0
  /** Set when input ran out or a syntax error was hit; unwinds every open container. */
  stopped = false

  readonly s: string

  constructor(s: string) {
    this.s = s
  }

  ws(): void {
    const s = this.s
    while (this.i < s.length) {
      const c = s.charCodeAt(this.i)
      if (c !== 32 && c !== 10 && c !== 13 && c !== 9) return
      this.i++
    }
  }

  value(): Parsed {
    this.ws()
    const c = this.s[this.i]
    if (c === undefined) return this.stop()
    if (c === "{") return this.object()
    if (c === "[") return this.array()
    if (c === '"') return this.string()
    if (c === "-" || (c >= "0" && c <= "9")) return this.number()
    if (c in LITERALS) return this.literal(c)
    return this.stop()
  }

  stop(): typeof NONE {
    this.stopped = true
    return NONE
  }

  object(): Record<string, unknown> {
    const obj: Record<string, unknown> = {}
    this.i++
    while (true) {
      this.ws()
      const c = this.s[this.i]
      if (c === "}") {
        this.i++
        return obj
      }
      if (c === ",") {
        this.i++
        continue
      }
      if (c !== '"') {
        this.stop()
        return obj
      }
      const key = this.string()
      if (this.stopped) return obj
      this.ws()
      if (this.s[this.i] !== ":") {
        this.stop()
        return obj
      }
      this.i++
      const v = this.value()
      if (v !== NONE) {
        // defineProperty keeps a "__proto__" key from replacing the prototype.
        Object.defineProperty(obj, key, {
          value: v,
          enumerable: true,
          writable: true,
          configurable: true,
        })
      }
      if (this.stopped) return obj
      this.ws()
      const next = this.s[this.i]
      if (next === ",") this.i++
      else if (next !== "}") {
        this.stop()
        return obj
      }
    }
  }

  array(): unknown[] {
    const arr: unknown[] = []
    this.i++
    while (true) {
      this.ws()
      const c = this.s[this.i]
      if (c === "]") {
        this.i++
        return arr
      }
      if (c === ",") {
        this.i++
        continue
      }
      const v = this.value()
      if (v !== NONE) arr.push(v)
      if (this.stopped) return arr
      this.ws()
      const next = this.s[this.i]
      if (next === ",") this.i++
      else if (next !== "]") {
        this.stop()
        return arr
      }
    }
  }

  string(): string {
    const s = this.s
    let out = ""
    let start = ++this.i
    while (this.i < s.length) {
      const c = s[this.i]
      if (c === '"') {
        out += s.slice(start, this.i++)
        return out
      }
      if (c !== "\\") {
        this.i++
        continue
      }
      out += s.slice(start, this.i)
      start = this.i
      const e = s[this.i + 1]
      if (e === undefined) break
      if (e === "u") {
        const hex = s.slice(this.i + 2, this.i + 6)
        if (hex.length < 4) break
        const code = Number.parseInt(hex, 16)
        out += Number.isNaN(code) ? hex : String.fromCharCode(code)
        this.i += 6
      } else {
        out += ESCAPES[e] ?? e
        this.i += 2
      }
      start = this.i
    }
    // Ran out of input inside the string. A partial escape (start sits on it) is dropped.
    if (s[start] !== "\\") out += s.slice(start)
    this.stopped = true
    const last = out.charCodeAt(out.length - 1)
    return last >= 0xd800 && last <= 0xdbff ? out.slice(0, -1) : out
  }

  number(): Parsed {
    const s = this.s
    const start = this.i
    while (this.i < s.length && NUMBER_CHAR.test(s[this.i] as string)) this.i++
    const token = s.slice(start, this.i)
    // Trim a dangling sign, dot or exponent left by truncation (`12.`, `3e`, `-`).
    const trimmed = token.replace(/[-+.eE]+$/, "")
    if (trimmed !== token) this.stopped = true
    const n = Number(trimmed)
    return trimmed === "" || Number.isNaN(n) ? this.stop() : n
  }

  literal(first: string): Parsed {
    const [word, value] = LITERALS[first] as [string, unknown]
    const rest = this.s.slice(this.i, this.i + word.length)
    if (rest === word) {
      this.i += word.length
      return value
    }
    if (this.i + rest.length === this.s.length && word.startsWith(rest)) {
      this.i += rest.length
      this.stopped = true
      return value
    }
    return this.stop()
  }
}

/** Parse possibly truncated JSON, returning the best-effort value and whether it was complete. */
export function parsePartialJson(text: string): PartialJsonResult {
  const p = new Parser(text)
  const v = p.value()
  if (!p.stopped) p.ws()
  return { value: v === NONE ? undefined : v, complete: !p.stopped && p.i === text.length }
}
