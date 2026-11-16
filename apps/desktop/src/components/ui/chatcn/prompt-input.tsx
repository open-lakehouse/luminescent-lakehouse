// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import * as React from "react"
import { Button, type ButtonProps } from "@/components/ui/chatcn/button"
import { DropZoneOverlay } from "@/components/ui/chatcn/drop-zone"
import { SendButton, type SendButtonProps } from "@/components/ui/chatcn/send-button"
import { Tooltip } from "@/components/ui/chatcn/tooltip"
import { useChatcnAutoResize } from "@/components/ui/chatcn/hooks/use-chatcn-auto-resize"
import { type FileRejection, sortFiles, useChatcnFileDrop } from "@/components/ui/chatcn/hooks/use-chatcn-file-drop"
import { type SubmitOn, useChatcnImeSubmit } from "@/components/ui/chatcn/hooks/use-chatcn-ime-submit"
import { PaperclipIcon } from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * Types and context
 * -----------------------------------------------------------------------------------------------*/

type PromptInputStatus = "idle" | "submitting" | "streaming"
type PromptInputVariant = "default" | "floating" | "minimal" | "hero"
type FileSource = "paste" | "drop" | "picker"

interface PromptInputContextValue {
  value: string
  setValue: (value: string) => void
  submit: () => void
  stop: () => void
  status: PromptInputStatus
  disabled: boolean
  /** Nothing to send: no text and no `hasContent`. */
  empty: boolean
  variant: PromptInputVariant
  submitOn: SubmitOn
  textareaRef: React.RefObject<HTMLTextAreaElement | null>
  openFilePicker: () => void
  /** False when no `onFiles` handler is set. */
  acceptsFiles: boolean
  handlePaste: (event: React.ClipboardEvent<HTMLTextAreaElement>) => void
}

const PromptInputContext = React.createContext<PromptInputContextValue | null>(null)

/** Reads the composer state from inside a `PromptInput`, for custom tools and actions. */
function usePromptInput() {
  const context = React.useContext(PromptInputContext)
  if (!context) throw new Error("usePromptInput must be used inside <PromptInput>.")
  return context
}

/* -------------------------------------------------------------------------------------------------
 * PromptInput
 * -----------------------------------------------------------------------------------------------*/

interface PromptInputProps
  extends Omit<React.ComponentProps<"form">, "onSubmit" | "onChange" | "defaultValue"> {
  value?: string
  defaultValue?: string
  onValueChange?: (value: string) => void
  /** Enter, Cmd/Ctrl+Enter or the send button. Not called while empty, disabled or busy. */
  onSubmit?: (message: { text: string }) => void
  /** `streaming` turns Send into Stop and makes Escape call `onStop`. */
  status?: PromptInputStatus
  onStop?: () => void
  disabled?: boolean
  /**
   * - `default`: card with a toolbar row under the text.
   * - `floating`: translucent with a background blur, for composers over content.
   * - `minimal`: one row, text and actions inline.
   * - `hero`: larger type and padding for empty states.
   */
  variant?: PromptInputVariant
  /** Files from paste, drop or the attach button, already filtered by `accept` and limits. */
  onFiles?: (files: File[], source: FileSource) => void
  /** Files refused by `accept`, `maxFileSize` or `maxFiles`, with the reason. */
  onFilesRejected?: (rejections: FileRejection[]) => void
  /** Same syntax as `<input accept>`. */
  accept?: string
  maxFileSize?: number
  maxFiles?: number
  /** Pasted text this long or longer becomes a `Pasted text.txt` file. Off by default. */
  pasteTextAsFileAt?: number
  /** Allow sending with empty text, for example when files are attached. */
  hasContent?: boolean
  submitOn?: SubmitOn
  /** Clear the text after sending in uncontrolled mode. Default true. */
  clearOnSubmit?: boolean
  /** Drop overlay copy. */
  dropLabel?: React.ReactNode
  dropHint?: React.ReactNode
}

const surface: Record<PromptInputVariant, string> = {
  default: "rounded-chatcn-xl bg-chatcn-card shadow-chatcn-sm",
  // Floating chrome is the one place translucency is allowed. Solid where blur is unsupported.
  floating: cn(
    "rounded-chatcn-xl shadow-chatcn-lg bg-chatcn-card",
    "supports-[backdrop-filter:blur(0)]:bg-[color-mix(in_oklch,var(--chatcn-card)_80%,transparent)] supports-[backdrop-filter:blur(0)]:backdrop-blur-lg",
  ),
  minimal: "rounded-chatcn-2xl bg-chatcn-card shadow-chatcn-sm flex-row items-end gap-1 p-1.5",
  hero: "rounded-chatcn-2xl bg-chatcn-card shadow-chatcn-sm",
}

/** The input-focus ring from the spec: 1 px ink edge plus a 3 px soft halo. Never animated. */
const focusRing =
  "focus-within:shadow-[0_0_0_1px_var(--chatcn-gray-900),0_0_0_4px_var(--chatcn-alpha-300)]"

/**
 * The composer. A form with a growing textarea, slots for attachments and tools, and the send
 * button. Enter sends, Shift+Enter adds a line, Cmd/Ctrl+Enter also sends, IME composition never
 * sends, Escape stops a streaming response. Images pasted or dropped become files, never a lost
 * paste.
 */
function PromptInput({
  value: valueProp,
  defaultValue = "",
  onValueChange,
  onSubmit,
  status = "idle",
  onStop,
  disabled = false,
  variant = "default",
  onFiles,
  onFilesRejected,
  accept,
  maxFileSize,
  maxFiles,
  pasteTextAsFileAt,
  hasContent = false,
  submitOn = "auto",
  clearOnSubmit = true,
  dropLabel,
  dropHint,
  className,
  children,
  onKeyDown,
  onPointerDown,
  ...props
}: PromptInputProps) {
  const [valueState, setValueState] = React.useState(defaultValue)
  const controlled = valueProp !== undefined
  const value = controlled ? valueProp : valueState
  const textareaRef = React.useRef<HTMLTextAreaElement>(null)
  const fileInputRef = React.useRef<HTMLInputElement>(null)

  const setValue = React.useCallback(
    (next: string) => {
      if (!controlled) setValueState(next)
      onValueChange?.(next)
    },
    [controlled, onValueChange],
  )

  const empty = value.trim().length === 0 && !hasContent

  const submit = () => {
    if (disabled || status !== "idle" || empty) return
    onSubmit?.({ text: value })
    if (!controlled && clearOnSubmit) setValueState("")
  }

  const stop = () => {
    if (status === "streaming") onStop?.()
  }

  const limits = { accept, maxSize: maxFileSize, maxFiles }
  const deliver = (files: File[], source: FileSource) => {
    const { accepted, rejected } = sortFiles(files, limits)
    if (accepted.length) onFiles?.(accepted, source)
    if (rejected.length) onFilesRejected?.(rejected)
  }

  const drop = useChatcnFileDrop({
    ...limits,
    disabled: disabled || !onFiles,
    onFiles: (files) => onFiles?.(files, "drop"),
    onReject: onFilesRejected,
  })

  const handlePaste = (event: React.ClipboardEvent<HTMLTextAreaElement>) => {
    if (!onFiles) return
    const data = event.clipboardData
    const files = Array.from(data.files)
    const text = data.getData("text/plain")
    // Only take over when the paste would otherwise be lost (files, no text) or when the text is
    // long enough to be a document. Ordinary text pastes are never blocked.
    if (files.length > 0 && text.length === 0) {
      event.preventDefault()
      deliver(files, "paste")
    } else if (pasteTextAsFileAt !== undefined && text.length >= pasteTextAsFileAt) {
      event.preventDefault()
      deliver([new File([text], "Pasted text.txt", { type: "text/plain" })], "paste")
    }
  }

  const context: PromptInputContextValue = {
    value,
    setValue,
    submit,
    stop,
    status,
    disabled,
    empty,
    variant,
    submitOn,
    textareaRef,
    openFilePicker: () => fileInputRef.current?.click(),
    acceptsFiles: Boolean(onFiles),
    handlePaste,
  }

  // Announce the switch to Stop once, not every token.
  const [announcement, setAnnouncement] = React.useState("")
  const prevStatus = React.useRef(status)
  React.useEffect(() => {
    if (prevStatus.current === status) return
    if (status === "streaming") setAnnouncement("Generating. Press Escape to stop.")
    else if (prevStatus.current === "streaming") setAnnouncement("")
    prevStatus.current = status
  }, [status])

  return (
    <PromptInputContext.Provider value={context}>
      <form
        data-slot="prompt-input"
        data-variant={variant}
        data-prompt-state={status}
        data-disabled={disabled ? "" : undefined}
        aria-disabled={disabled || undefined}
        className={cn(
          "group/prompt-input @container/prompt-input relative flex w-full min-w-0 cursor-text flex-col",
          "forced-colors:border forced-colors:border-[CanvasText]",
          surface[variant],
          !disabled && focusRing,
          disabled && "bg-chatcn-bg-subtle shadow-chatcn-border cursor-not-allowed",
          className,
        )}
        onSubmit={(event) => {
          event.preventDefault()
          if (status === "streaming") return
          submit()
        }}
        onKeyDown={(event) => {
          onKeyDown?.(event)
          // Escape from anywhere in the composer (not from portalled menus) stops the response.
          if (
            event.key === "Escape" &&
            !event.defaultPrevented &&
            status === "streaming" &&
            event.currentTarget.contains(event.target as Node)
          ) {
            event.preventDefault()
            stop()
          }
        }}
        onPointerDown={(event) => {
          onPointerDown?.(event)
          // A click on the surface (not a control or editable text) puts the caret in the textarea.
          const target = event.target as HTMLElement
          if (
            event.button === 0 &&
            !disabled &&
            !target.closest(
              "button,a,input,textarea,select,[role=button],[data-slot=attachment],[contenteditable=true]",
            )
          ) {
            event.preventDefault()
            textareaRef.current?.focus()
          }
        }}
        {...drop.rootProps}
        {...props}
      >
        {children}
        {onFiles ? (
          <>
            <input
              ref={fileInputRef}
              type="file"
              multiple={maxFiles !== 1}
              accept={accept}
              tabIndex={-1}
              aria-hidden="true"
              className="hidden"
              onChange={(event) => {
                deliver(Array.from(event.target.files ?? []), "picker")
                event.target.value = ""
              }}
            />
            <DropZoneOverlay state={drop.state} accept={accept} label={dropLabel} hint={dropHint} />
          </>
        ) : null}
        <span aria-live="polite" className="sr-only">
          {announcement}
        </span>
      </form>
    </PromptInputContext.Provider>
  )
}

/* -------------------------------------------------------------------------------------------------
 * Textarea
 * -----------------------------------------------------------------------------------------------*/

const textareaVariant: Record<PromptInputVariant, string> = {
  default: "chatcn-text-15 min-h-12 px-4 pt-3.5 pb-1.5",
  floating: "chatcn-text-15 min-h-12 px-4 pt-3.5 pb-1.5",
  minimal: "chatcn-text-15 min-h-9 flex-1 px-2.5 py-1.5",
  hero: "chatcn-text-18 min-h-24 px-5 pt-5 pb-2 tracking-[-0.005em]",
}

interface PromptInputTextareaProps
  extends Omit<React.ComponentProps<"textarea">, "value" | "defaultValue"> {}

/**
 * Grows with its content up to 40% of the viewport, then scrolls. On touch screens the type is
 * at least 16 px so iOS never zooms in on focus.
 */
function PromptInputTextarea({
  className,
  placeholder = "Ask anything…",
  onChange,
  onKeyDown,
  onPaste,
  onScroll,
  onCompositionStart,
  onCompositionEnd,
  ref,
  ...props
}: PromptInputTextareaProps) {
  const {
    value,
    setValue,
    submit,
    stop,
    status,
    disabled,
    variant,
    submitOn,
    textareaRef,
    handlePaste,
  } = usePromptInput()
  useChatcnAutoResize(textareaRef, { value })
  const edges = useScrollEdges(textareaRef, value)
  const ime = useChatcnImeSubmit({
    onSubmit: () => {
      if (status === "idle") submit()
    },
    onStop: stop,
    streaming: status === "streaming",
    submitOn,
    disabled,
  })

  const setRefs = (node: HTMLTextAreaElement | null) => {
    textareaRef.current = node
    if (typeof ref === "function") ref(node)
    else if (ref) ref.current = node
  }

  return (
    <textarea
      ref={setRefs}
      data-slot="prompt-input-textarea"
      name="message"
      rows={1}
      aria-label={props["aria-label"] ?? "Message"}
      autoComplete="off"
      placeholder={placeholder}
      disabled={disabled}
      value={value}
      onChange={(event) => {
        onChange?.(event)
        setValue(event.target.value)
      }}
      onKeyDown={(event) => {
        onKeyDown?.(event)
        ime.onKeyDown(event)
      }}
      onPaste={(event) => {
        onPaste?.(event)
        if (!event.defaultPrevented) handlePaste(event)
      }}
      onScroll={(event) => {
        onScroll?.(event)
        edges.update()
      }}
      data-fade-top={edges.top ? "" : undefined}
      data-fade-bottom={edges.bottom ? "" : undefined}
      onCompositionStart={(event) => {
        onCompositionStart?.(event)
        ime.onCompositionStart()
      }}
      onCompositionEnd={(event) => {
        onCompositionEnd?.(event)
        ime.onCompositionEnd()
      }}
      className={cn(
        "text-chatcn-fg placeholder:text-chatcn-fg-subtle block w-full resize-none bg-transparent outline-none",
        "chatcn-scroll field-sizing-content max-h-[40vh] overflow-y-auto [overflow-wrap:anywhere]",
        // Past the 40vh cap, the line cut by an edge fades out instead of ending mid-glyph.
        "data-[fade-bottom]:[--chatcn-fade-bottom:20px] data-[fade-top]:[--chatcn-fade-top:20px]",
        "[mask-image:linear-gradient(to_bottom,transparent,#000_var(--chatcn-fade-top,0px),#000_calc(100%-var(--chatcn-fade-bottom,0px)),transparent)]",
        "pointer-coarse:chatcn-text-16 disabled:cursor-not-allowed disabled:opacity-60",
        textareaVariant[variant],
        className,
      )}
      {...props}
    />
  )
}

/**
 * Which edges of a scrolled textarea hide text. Read on scroll, on value changes and on resize;
 * the attributes only change when an edge flips, so typing never re-renders for it.
 */
function useScrollEdges(ref: React.RefObject<HTMLTextAreaElement | null>, value: string) {
  const [edges, setEdges] = React.useState({ top: false, bottom: false })
  const update = React.useCallback(() => {
    const el = ref.current
    if (!el) return
    const top = el.scrollTop > 1
    const bottom = el.scrollHeight - el.clientHeight - el.scrollTop > 1
    setEdges((prev) => (prev.top === top && prev.bottom === bottom ? prev : { top, bottom }))
  }, [ref])
  React.useLayoutEffect(update, [update, value])
  React.useEffect(() => {
    const el = ref.current
    if (!el || typeof ResizeObserver === "undefined") return
    const observer = new ResizeObserver(update)
    observer.observe(el)
    return () => observer.disconnect()
  }, [ref, update])
  return { ...edges, update }
}

/* -------------------------------------------------------------------------------------------------
 * Slots
 * -----------------------------------------------------------------------------------------------*/

function hasChildren(children: React.ReactNode) {
  return React.Children.toArray(children).length > 0
}

/** Slot above the text for an `AttachmentList`. Renders nothing while empty. */
function PromptInputAttachments({ className, children, ...props }: React.ComponentProps<"div">) {
  const { variant } = usePromptInput()
  if (!hasChildren(children)) return null
  return (
    <div
      data-slot="prompt-input-attachments"
      className={cn(slotInset(variant), className)}
      {...props}
    >
      {children}
    </div>
  )
}

/**
 * Header, attachments and toolbar share one inset, so chips, cards and toolbar buttons line up on
 * the same edge, and 8 px chips sit concentric inside the 16 px frame.
 */
function slotInset(variant: PromptInputVariant) {
  return variant === "hero" ? "px-3 pt-3" : "px-2 pt-2"
}

/** Slot above the text for context chips: files, a selection, a URL. Renders nothing while empty. */
function PromptInputHeader({ className, children, ...props }: React.ComponentProps<"div">) {
  const { variant } = usePromptInput()
  if (!hasChildren(children)) return null
  return (
    <div
      data-slot="prompt-input-header"
      className={cn("flex min-w-0 items-center gap-1", slotInset(variant), className)}
      {...props}
    >
      {children}
    </div>
  )
}

/** The row under the text. Put `PromptInputTools` first and `PromptInputActions` last. */
function PromptInputToolbar({ className, ...props }: React.ComponentProps<"div">) {
  const { variant } = usePromptInput()
  return (
    <div
      data-slot="prompt-input-toolbar"
      className={cn(
        "flex min-w-0 items-center gap-1",
        variant === "hero" ? "p-3 pt-1" : "p-2 pt-1",
        className,
      )}
      {...props}
    />
  )
}

/**
 * Leading group: attach, tools menu, mode chips. It never gets narrower than its controls (mode
 * chips fold to their icons first), so in a tight composer the model name truncates instead of
 * sliding under the chips.
 */
function PromptInputTools({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="prompt-input-tools"
      className={cn("flex min-w-fit flex-1 items-center gap-1", className)}
      {...props}
    />
  )
}

/** Trailing group: model picker, dictation, send. Only a truncating model name ever shrinks. */
function PromptInputActions({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="prompt-input-actions"
      className={cn("ms-auto flex min-w-0 items-center gap-1", className)}
      {...props}
    />
  )
}

/* -------------------------------------------------------------------------------------------------
 * Controls
 * -----------------------------------------------------------------------------------------------*/

interface PromptInputButtonProps extends ButtonProps {
  /** Accessible name and tooltip for icon-only buttons. */
  tooltip?: string
  /** Shortcut shown in the tooltip, for example `⌘U`. */
  shortcut?: string
}

/** A quiet toolbar button. Icon-only by default; pass text children for a labelled one. */
function PromptInputButton({
  tooltip,
  shortcut,
  size = "icon-md",
  variant = "ghost",
  className,
  ...props
}: PromptInputButtonProps) {
  const button = (
    <Button
      data-slot="prompt-input-button"
      aria-label={props["aria-label"] ?? tooltip}
      size={size}
      variant={variant}
      className={cn(
        size.startsWith("icon") ? "pointer-coarse:size-11" : "pointer-coarse:h-11",
        className,
      )}
      {...props}
    />
  )
  return tooltip ? (
    <Tooltip content={tooltip} shortcut={shortcut}>
      {button}
    </Tooltip>
  ) : (
    button
  )
}

/** Opens the file picker. Results go to `onFiles` on the `PromptInput`. */
function PromptInputAttachButton({
  tooltip = "Attach files",
  children,
  ...props
}: PromptInputButtonProps) {
  const { openFilePicker, acceptsFiles, disabled } = usePromptInput()
  if (!acceptsFiles) return null
  return (
    <PromptInputButton
      tooltip={tooltip}
      disabled={disabled}
      onClick={openFilePicker}
      data-slot="prompt-input-attach"
      {...props}
    >
      {children ?? <PaperclipIcon />}
    </PromptInputButton>
  )
}

/** The send button, wired to the composer: disabled while empty, Stop while streaming. */
function PromptInputSubmit(props: Omit<SendButtonProps, "status" | "empty" | "onStop">) {
  const { status, empty, stop, disabled, variant } = usePromptInput()
  return (
    <SendButton
      status={status}
      empty={empty}
      onStop={stop}
      disabled={disabled}
      size={variant === "hero" ? "lg" : "md"}
      {...props}
    />
  )
}

export type {
  FileSource,
  PromptInputButtonProps,
  PromptInputContextValue,
  PromptInputProps,
  PromptInputStatus,
  PromptInputTextareaProps,
  PromptInputVariant,
}
export {
  PromptInput,
  PromptInputActions,
  PromptInputAttachButton,
  PromptInputAttachments,
  PromptInputButton,
  PromptInputHeader,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputToolbar,
  PromptInputTools,
  usePromptInput,
}
