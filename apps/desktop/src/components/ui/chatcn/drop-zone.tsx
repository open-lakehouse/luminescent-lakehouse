// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import * as React from "react"
import { createPortal } from "react-dom"
import {
  type DropState,
  type FileRejection,
  type UseChatcnFileDropOptions,
  useChatcnFileDrop,
} from "@/components/ui/chatcn/hooks/use-chatcn-file-drop"
import { BanIcon, UploadIcon } from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * Helpers
 * -----------------------------------------------------------------------------------------------*/

const KINDS: [RegExp, string][] = [
  [/^image\/\*$/, "images"],
  [/^(\.pdf|application\/pdf)$/, "PDFs"],
  [/^video\/\*$/, "videos"],
  [/^audio\/\*$/, "audio"],
  [/^text\/\*$/, "text files"],
  [/^(\.csv|text\/csv)$/, "CSV files"],
  [/^(\.md|text\/markdown)$/, "Markdown files"],
  [/^(\.txt|text\/plain)$/, "text files"],
  [/^(\.json|application\/json)$/, "JSON files"],
]

function joinWords(words: string[]) {
  if (words.length <= 1) return words[0] ?? ""
  return `${words.slice(0, -1).join(", ")} and ${words[words.length - 1]}`
}

/** Readable list from an accept string: `image/*,.pdf` becomes "images and PDFs". */
function describeAccept(accept?: string) {
  if (!accept) return ""
  const words: string[] = []
  for (const raw of accept.split(",")) {
    const rule = raw.trim().toLowerCase()
    if (!rule) continue
    const kind =
      KINDS.find(([re]) => re.test(rule))?.[1] ??
      (rule.startsWith(".") ? `${rule.slice(1).toUpperCase()} files` : rule)
    if (!words.includes(kind)) words.push(kind)
  }
  return joinWords(words)
}

function capitalize(text: string) {
  return text.charAt(0).toUpperCase() + text.slice(1)
}

/** How many files a drag carries. Browsers expose the items, not the files, until the drop. */
function countDragFiles(event: DragEvent | React.DragEvent) {
  return Array.from(event.dataTransfer?.items ?? []).filter((i) => i.kind === "file").length
}

/* -------------------------------------------------------------------------------------------------
 * Overlay
 * -----------------------------------------------------------------------------------------------*/

interface DropZoneOverlayProps extends React.ComponentProps<"div"> {
  state: DropState
  /** Accept title. Default "Drop files to attach". */
  label?: React.ReactNode
  /** Second line under the title, for example the accepted types and size limit. */
  hint?: React.ReactNode
  /** Reject title. Default says which types are accepted, built from `accept`. */
  rejectLabel?: React.ReactNode
  accept?: string
  /** Files in the drag, so the default copy can count them: "Drop 3 files to attach". */
  count?: number
  /** The most files one drop takes. With `count` above it, the reject copy says so. */
  maxFiles?: number
  /** `contained` fills its positioned parent; `page` covers the viewport. */
  variant?: "contained" | "page"
}

/**
 * What the user sees while dragging files. Fades in over 150 ms with the content settling from
 * scale 0.98; says before the drop what will happen, counted ("Drop 3 files to attach"), or that
 * the files will be refused, and why: the wrong type, or more than one drop takes.
 */
function DropZoneOverlay({
  state,
  label,
  hint,
  rejectLabel,
  accept,
  count,
  maxFiles,
  variant = "contained",
  className,
  ...props
}: DropZoneOverlayProps) {
  const visible = state !== "idle"
  const reject = state === "reject"
  const types = describeAccept(accept)
  const page = variant === "page"
  const tooMany = count !== undefined && maxFiles !== undefined && count > maxFiles
  const what = !count ? "files" : count === 1 ? "the file" : `${count.toLocaleString()} files`
  const title = reject
    ? (rejectLabel ??
      (tooMany
        ? `Attach up to ${maxFiles} ${maxFiles === 1 ? "file" : "files"} at a time`
        : types
          ? `Only ${types} can be attached`
          : "These files can’t be attached"))
    : (label ?? `Drop ${what} ${page ? "anywhere " : ""}to attach`)
  const sub = reject
    ? tooMany && !rejectLabel
      ? `You’re dragging ${count?.toLocaleString()}.`
      : null
    : (hint ?? (types ? capitalize(types) : null))

  return (
    <div
      aria-hidden="true"
      data-slot="drop-zone-overlay"
      data-drop-state={state}
      data-variant={variant}
      className={cn(
        "pointer-events-none grid place-items-center",
        page ? "fixed inset-0 z-50 p-3" : "absolute inset-0 z-10 rounded-[inherit] p-2",
        page
          ? "bg-[color-mix(in_oklch,var(--chatcn-bg)_88%,transparent)]"
          : "bg-[color-mix(in_oklch,var(--chatcn-card)_94%,transparent)]",
        "ease-chatcn-out transition-[opacity,visibility] duration-150 motion-reduce:duration-100",
        visible ? "visible opacity-100" : "invisible opacity-0",
        className,
      )}
      {...props}
    >
      <div
        className={cn(
          "border-chatcn-border-strong grid size-full place-items-center border border-dashed",
          // Concentric: 12 px inset plus 12 px radius fits a 24 px window corner; 8 plus 8 fits 16.
          page ? "rounded-chatcn-lg border-2" : "rounded-chatcn-md",
          reject && "border-solid",
        )}
      >
        <div
          className={cn(
            "flex flex-col items-center gap-1.5 px-4 text-center",
            "ease-chatcn-out transition-[scale] duration-150 motion-reduce:scale-none!",
            visible ? "scale-100" : "scale-98",
          )}
        >
          <span
            key={reject ? "reject" : "accept"}
            className={cn(
              "grid place-items-center motion-safe:animate-[chatcn-swap_150ms_var(--chatcn-ease-out)]",
              page ? "mb-1 [&_svg]:size-6" : "[&_svg]:size-5",
              reject ? "text-chatcn-red" : "text-chatcn-fg",
            )}
          >
            {reject ? <BanIcon /> : <UploadIcon />}
          </span>
          <p
            className={cn(
              "text-chatcn-fg font-medium text-balance!",
              page ? "chatcn-text-20" : "chatcn-text-14",
            )}
          >
            {title}
          </p>
          {sub ? (
            <p
              className={cn(
                "text-chatcn-fg-subtle tabular-nums",
                page ? "chatcn-text-14" : "chatcn-text-13",
              )}
            >
              {sub}
            </p>
          ) : null}
        </div>
      </div>
    </div>
  )
}

/* -------------------------------------------------------------------------------------------------
 * DropZone
 * -----------------------------------------------------------------------------------------------*/

interface DropZoneProps
  extends Omit<React.ComponentProps<"div">, "onDrop">,
    Omit<UseChatcnFileDropOptions, "target"> {
  label?: React.ReactNode
  hint?: React.ReactNode
  rejectLabel?: React.ReactNode
}

/**
 * A drop target the size of its content, such as a composer. Wrap the area and the overlay
 * appears inside it, following its corner radius. Pair it with an attach button: dragging is
 * never the only way in.
 */
function DropZone({
  accept,
  maxSize,
  maxFiles,
  disabled,
  onFiles,
  onReject,
  label,
  hint,
  rejectLabel,
  className,
  children,
  ...props
}: DropZoneProps) {
  const { state, rootProps } = useChatcnFileDrop({
    accept,
    maxSize,
    maxFiles,
    disabled,
    onFiles,
    onReject,
  })
  const [count, setCount] = React.useState<number>()
  // Count the files on the way in, so the overlay can say what one drop will do.
  const dropProps = {
    ...rootProps,
    onDragEnter: (event: React.DragEvent) => {
      rootProps.onDragEnter(event)
      setCount(countDragFiles(event) || undefined)
    },
  }
  return (
    <div
      data-slot="drop-zone"
      data-drop-state={state}
      className={cn("relative", className)}
      {...dropProps}
      {...props}
    >
      {children}
      <DropZoneOverlay
        state={state}
        accept={accept}
        count={count}
        maxFiles={maxFiles}
        label={label}
        hint={hint}
        rejectLabel={rejectLabel}
      />
    </div>
  )
}

/* -------------------------------------------------------------------------------------------------
 * DropZonePage
 * -----------------------------------------------------------------------------------------------*/

interface DropZonePageProps extends Omit<UseChatcnFileDropOptions, "target"> {
  label?: React.ReactNode
  hint?: React.ReactNode
  rejectLabel?: React.ReactNode
  className?: string
}

/** Listens on the whole window and covers the page while files are dragged anywhere over it. */
function DropZonePage({ label, hint, rejectLabel, className, ...options }: DropZonePageProps) {
  const { state } = useChatcnFileDrop({ ...options, target: "window" })
  const [mounted, setMounted] = React.useState(false)
  const [count, setCount] = React.useState<number>()
  React.useEffect(() => setMounted(true), [])
  React.useEffect(() => {
    const enter = (event: DragEvent) => setCount(countDragFiles(event) || undefined)
    window.addEventListener("dragenter", enter)
    return () => window.removeEventListener("dragenter", enter)
  }, [])
  if (!mounted) return null
  return createPortal(
    <DropZoneOverlay
      variant="page"
      state={state}
      accept={options.accept}
      count={count}
      maxFiles={options.maxFiles}
      label={label}
      hint={hint}
      rejectLabel={rejectLabel}
      className={className}
    />,
    document.body,
  )
}

export type { DropState, DropZoneOverlayProps, DropZonePageProps, DropZoneProps, FileRejection }
export { DropZone, DropZoneOverlay, DropZonePage, describeAccept }
