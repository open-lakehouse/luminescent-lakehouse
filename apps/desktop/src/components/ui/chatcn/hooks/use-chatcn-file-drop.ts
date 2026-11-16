// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import * as React from "react"

type DropState = "idle" | "accept" | "reject"

interface FileRejection {
  file: File
  reason: "type" | "size" | "count"
}

interface UseChatcnFileDropOptions {
  /** Same syntax as `<input accept>`: `image/*,.pdf,application/json`. Empty accepts anything. */
  accept?: string
  /** Largest file in bytes. */
  maxSize?: number
  /** Most files in one drop. */
  maxFiles?: number
  disabled?: boolean
  onFiles?: (files: File[]) => void
  onReject?: (rejections: FileRejection[]) => void
  /**
   * `element` (default): spread `rootProps` on the drop target.
   * `window`: listen on the whole page, for a full-page overlay.
   */
  target?: "element" | "window"
}

/** Extensions mapped to MIME types, so `.pdf` can be judged while dragging, before names exist. */
const EXT_MIME: Record<string, string> = {
  ".pdf": "application/pdf",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".webp": "image/webp",
  ".svg": "image/svg+xml",
  ".csv": "text/csv",
  ".txt": "text/plain",
  ".md": "text/markdown",
  ".json": "application/json",
  ".zip": "application/zip",
}

/**
 * Whether a file (or a dragged item, which has a type but no name yet) matches an accept string.
 * Unknown types pass while dragging and are judged again by name on drop.
 */
function matchesAccept(file: { type: string; name?: string }, accept?: string) {
  if (!accept) return true
  const rules = accept
    .split(",")
    .map((r) => r.trim().toLowerCase())
    .filter(Boolean)
  if (rules.length === 0) return true
  const type = file.type.toLowerCase()
  const name = file.name?.toLowerCase()
  if (!type && !name) return true
  return rules.some((rule) => {
    if (rule.startsWith(".")) {
      if (name) return name.endsWith(rule)
      return EXT_MIME[rule] === type
    }
    if (rule.endsWith("/*")) return type.startsWith(rule.slice(0, -1))
    return type === rule
  })
}

function hasFiles(event: DragEvent | React.DragEvent) {
  return Array.from(event.dataTransfer?.types ?? []).includes("Files")
}

/** Judges the drag from what the browser exposes before the drop: item kinds and MIME types. */
function judgeDrag(event: DragEvent | React.DragEvent, accept?: string, maxFiles?: number) {
  const items = Array.from(event.dataTransfer?.items ?? []).filter((i) => i.kind === "file")
  if (maxFiles !== undefined && items.length > maxFiles) return "reject" as const
  return items.every((i) => matchesAccept({ type: i.type }, accept)) ? "accept" : "reject"
}

/** Splits dropped or pasted files into accepted and rejected, with a reason for each rejection. */
function sortFiles(
  files: File[],
  { accept, maxSize, maxFiles }: Pick<UseChatcnFileDropOptions, "accept" | "maxSize" | "maxFiles">,
) {
  const accepted: File[] = []
  const rejected: FileRejection[] = []
  for (const file of files) {
    if (!matchesAccept(file, accept)) rejected.push({ file, reason: "type" })
    else if (maxSize !== undefined && file.size > maxSize) rejected.push({ file, reason: "size" })
    else if (maxFiles !== undefined && accepted.length >= maxFiles)
      rejected.push({ file, reason: "count" })
    else accepted.push(file)
  }
  return { accepted, rejected }
}

/**
 * Drag and drop for files. Tracks whether files are over the target and whether they would be
 * accepted, so the overlay can say so before the drop. Text drags pass through untouched, so
 * dragging a snippet into a textarea still works.
 */
function useChatcnFileDrop(options: UseChatcnFileDropOptions = {}) {
  const optionsRef = React.useRef(options)
  optionsRef.current = options
  const [state, setState] = React.useState<DropState>("idle")
  const depth = React.useRef(0)

  const handlers = React.useMemo(() => {
    const enter = (event: DragEvent | React.DragEvent) => {
      const { disabled, accept, maxFiles } = optionsRef.current
      if (disabled || !hasFiles(event)) return
      event.preventDefault()
      depth.current += 1
      setState(judgeDrag(event, accept, maxFiles))
    }
    const over = (event: DragEvent | React.DragEvent) => {
      if (optionsRef.current.disabled || !hasFiles(event)) return
      event.preventDefault()
      if (event.dataTransfer) event.dataTransfer.dropEffect = "copy"
    }
    const leave = (event: DragEvent | React.DragEvent) => {
      if (optionsRef.current.disabled || !hasFiles(event)) return
      depth.current = Math.max(0, depth.current - 1)
      if (depth.current === 0) setState("idle")
    }
    const drop = (event: DragEvent | React.DragEvent) => {
      const { disabled, onFiles, onReject } = optionsRef.current
      if (disabled || !hasFiles(event)) return
      event.preventDefault()
      depth.current = 0
      setState("idle")
      const files = Array.from(event.dataTransfer?.files ?? [])
      const { accepted, rejected } = sortFiles(files, optionsRef.current)
      if (accepted.length) onFiles?.(accepted)
      if (rejected.length) onReject?.(rejected)
    }
    return { enter, over, leave, drop }
  }, [])

  const target = options.target ?? "element"
  React.useEffect(() => {
    if (target !== "window") return
    const { enter, over, leave, drop } = handlers
    window.addEventListener("dragenter", enter)
    window.addEventListener("dragover", over)
    window.addEventListener("dragleave", leave)
    window.addEventListener("drop", drop)
    return () => {
      window.removeEventListener("dragenter", enter)
      window.removeEventListener("dragover", over)
      window.removeEventListener("dragleave", leave)
      window.removeEventListener("drop", drop)
    }
  }, [target, handlers])

  React.useEffect(() => {
    if (options.disabled) {
      depth.current = 0
      setState("idle")
    }
  }, [options.disabled])

  const rootProps = React.useMemo(
    () => ({
      onDragEnter: handlers.enter as (event: React.DragEvent) => void,
      onDragOver: handlers.over as (event: React.DragEvent) => void,
      onDragLeave: handlers.leave as (event: React.DragEvent) => void,
      onDrop: handlers.drop as (event: React.DragEvent) => void,
    }),
    [handlers],
  )

  return { state, rootProps }
}

export type { DropState, FileRejection, UseChatcnFileDropOptions }
export { matchesAccept, sortFiles, useChatcnFileDrop }
