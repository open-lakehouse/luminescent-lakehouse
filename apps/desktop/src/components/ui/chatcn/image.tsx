// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
"use client"

import * as React from "react"
import { Button, buttonVariants } from "@/components/ui/chatcn/button"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/chatcn/dialog"
import { Tooltip } from "@/components/ui/chatcn/tooltip"
import {
  ChevronLeftIcon,
  ChevronRightIcon,
  DownloadIcon,
  ImageOffIcon,
  RotateCcwIcon,
} from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * Types
 * -----------------------------------------------------------------------------------------------*/

interface ImageSource {
  src: string
  /** Required. Describe what the image shows, not that it is an image. */
  alt: string
  /** Intrinsic size. Sets the placeholder's aspect ratio so nothing shifts when it loads. */
  width?: number
  height?: number
  /** A tiny low-resolution version (a data URL works) shown blurred until the image loads. */
  placeholder?: string
  /** Optional caption under the image and in the lightbox. */
  caption?: React.ReactNode
  /** File name for downloads. Defaults to the last part of `src`. */
  filename?: string
}

type ImageStatus = "loading" | "loaded" | "error"

interface ImageProps
  extends ImageSource,
    Omit<React.ComponentProps<"figure">, "placeholder" | "onError" | "onLoad"> {
  /** Aspect ratio when `width` and `height` are unknown, for example `16 / 9`. */
  aspectRatio?: number
  /** `contain` keeps the whole image, `cover` fills the frame. Default `contain`. */
  fit?: "contain" | "cover"
  /** Open a lightbox on click. On by default. */
  lightbox?: boolean
  /** Show a download action in the lightbox. On by default. */
  download?: boolean
  /** Replace the built-in lightbox, for example to open a shared gallery. */
  onOpen?: () => void
  /** Load eagerly for images above the fold. */
  priority?: boolean
  onStatusChange?: (status: ImageStatus) => void
  imgClassName?: string
}

/* -------------------------------------------------------------------------------------------------
 * Helpers
 * -----------------------------------------------------------------------------------------------*/

function filenameFrom(src: string, fallback = "image") {
  try {
    const path = new URL(src, "https://x.invalid").pathname
    const last = decodeURIComponent(path.split("/").pop() ?? "")
    return last || fallback
  } catch {
    return fallback
  }
}

/** A hairline drawn inside the frame so photos never bleed into the canvas. */
const HAIRLINE =
  "pointer-events-none absolute inset-0 rounded-[inherit] shadow-[inset_0_0_0_1px_var(--chatcn-alpha-400)] forced-colors:outline-solid forced-colors:outline-1"

/**
 * Tracks load state for one `<img>`. Cached images report `complete` before React can attach
 * listeners, so the ref callback checks right away and skips the fade for them.
 */
function useImageStatus(src: string, onStatusChange?: (status: ImageStatus) => void) {
  const [attempt, setAttempt] = React.useState(0)
  const id = `${attempt}:${src}`
  const [state, setState] = React.useState({ id, status: "loading" as ImageStatus, instant: false })
  // A new src or a retry starts over. Resetting during render avoids a frame of stale state.
  if (state.id !== id) setState({ id, status: "loading", instant: false })

  const callback = React.useRef(onStatusChange)
  callback.current = onStatusChange
  React.useEffect(() => {
    callback.current?.(state.status)
  }, [state.status])

  const ref = React.useCallback(
    (img: HTMLImageElement | null) => {
      if (img?.complete && img.naturalWidth > 0) {
        setState((s) => (s.id === id ? { id, status: "loaded", instant: true } : s))
      }
    },
    [id],
  )

  return {
    status: state.status,
    instant: state.instant,
    attempt,
    ref,
    onLoad: () => setState((s) => (s.status === "loaded" ? s : { ...s, status: "loaded" })),
    onError: () => setState((s) => ({ ...s, status: "error" })),
    retry: () => setAttempt((a) => a + 1),
  }
}

/* -------------------------------------------------------------------------------------------------
 * Lightbox
 * -----------------------------------------------------------------------------------------------*/

interface ImageLightboxProps {
  images: ImageSource[]
  /** Index of the image on show. */
  index: number
  onIndexChange?: (index: number) => void
  open: boolean
  onOpenChange: (open: boolean) => void
  download?: boolean
}

/**
 * Full-screen viewer for one image or a set. The canvas is solid, the image is contained, and
 * arrow keys page through a set (mirrored in right-to-left layouts). Paging swaps instantly:
 * it is keyboard-driven and frequent.
 */
function ImageLightbox({
  images,
  index,
  onIndexChange,
  open,
  onOpenChange,
  download = true,
}: ImageLightboxProps) {
  const count = images.length
  const current = images[Math.min(Math.max(index, 0), count - 1)]
  const many = count > 1
  const go = (delta: number) => onIndexChange?.((index + delta + count) % count)
  // Focus the stage, not the first button: a ring on Download right after opening is noise.
  const stageRef = React.useRef<HTMLDivElement>(null)

  if (!current) return null
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        size="full"
        initialFocus={stageRef}
        data-slot="image-lightbox"
        className="bg-chatcn-bg"
        onKeyDown={(event) => {
          if (!many) return
          const rtl = getComputedStyle(event.currentTarget).direction === "rtl"
          const next = rtl ? "ArrowLeft" : "ArrowRight"
          const prev = rtl ? "ArrowRight" : "ArrowLeft"
          if (event.key === next) {
            event.preventDefault()
            go(1)
          } else if (event.key === prev) {
            event.preventDefault()
            go(-1)
          }
        }}
      >
        <div className="flex h-13 shrink-0 items-center gap-3 ps-5 pe-14">
          <DialogTitle className="chatcn-text-14 min-w-0 flex-1 truncate font-medium">
            {current.caption ?? current.alt}
          </DialogTitle>
          {many ? (
            <span className="chatcn-text-13 text-chatcn-fg-subtle shrink-0 tabular-nums">
              {index + 1} of {count}
            </span>
          ) : null}
          {download ? (
            <Tooltip content="Download image">
              <a
                href={current.src}
                download={current.filename ?? filenameFrom(current.src)}
                aria-label="Download image"
                className={cn(
                  buttonVariants({ variant: "ghost", size: "icon-sm" }),
                  "pointer-coarse:size-11",
                )}
              >
                <DownloadIcon />
              </a>
            </Tooltip>
          ) : null}
        </div>

        <div
          ref={stageRef}
          tabIndex={-1}
          className="relative flex min-h-0 flex-1 items-center justify-center px-4 pb-4 outline-none sm:px-16"
        >
          <img
            key={current.src}
            src={current.src}
            alt={current.alt}
            width={current.width}
            height={current.height}
            draggable={false}
            className="rounded-chatcn-md max-h-full max-w-full object-contain select-none"
          />
          {many ? (
            <>
              <Button
                variant="outline"
                size="icon-md"
                aria-label="Previous image"
                onClick={() => go(-1)}
                className="absolute start-4 top-1/2 -translate-y-1/2 rounded-full max-sm:hidden pointer-coarse:size-11"
              >
                <ChevronLeftIcon className="rtl:-scale-x-100" />
              </Button>
              <Button
                variant="outline"
                size="icon-md"
                aria-label="Next image"
                onClick={() => go(1)}
                className="absolute end-4 top-1/2 -translate-y-1/2 rounded-full max-sm:hidden pointer-coarse:size-11"
              >
                <ChevronRightIcon className="rtl:-scale-x-100" />
              </Button>
            </>
          ) : null}
        </div>

        {many ? (
          <div className="flex shrink-0 justify-center gap-2 pb-4 sm:hidden">
            <Button
              variant="outline"
              size="icon-lg"
              aria-label="Previous image"
              onClick={() => go(-1)}
            >
              <ChevronLeftIcon className="rtl:-scale-x-100" />
            </Button>
            <Button variant="outline" size="icon-lg" aria-label="Next image" onClick={() => go(1)}>
              <ChevronRightIcon className="rtl:-scale-x-100" />
            </Button>
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

/* -------------------------------------------------------------------------------------------------
 * Image
 * -----------------------------------------------------------------------------------------------*/

/**
 * An image inside a message. The frame holds its final size from the first paint, a blurred
 * placeholder or a plain fill sits behind it, and the image fades in over 200 ms once decoded.
 */
function Image({
  src,
  alt,
  width,
  height,
  placeholder,
  caption,
  filename,
  aspectRatio,
  fit = "contain",
  lightbox = true,
  download = true,
  onOpen,
  priority = false,
  onStatusChange,
  className,
  imgClassName,
  style,
  ...props
}: ImageProps) {
  const { status, instant, attempt, ref, onLoad, onError, retry } = useImageStatus(
    src,
    onStatusChange,
  )
  const [open, setOpen] = React.useState(false)
  const ratio = width && height ? width / height : aspectRatio
  const interactive = status === "loaded" && (onOpen !== undefined || lightbox)

  const frame = (
    <span
      data-slot="image-frame"
      data-image-status={status}
      className={cn(
        "bg-chatcn-fill relative block size-full overflow-hidden rounded-[inherit]",
        ratio === undefined && fit === "contain" && status !== "loaded" && "min-h-40",
      )}
      style={ratio ? { aspectRatio: ratio } : undefined}
    >
      {placeholder && status !== "error" ? (
        <img
          src={placeholder}
          alt=""
          aria-hidden="true"
          className={cn(
            "absolute inset-0 size-full scale-110 object-cover blur-xl",
            "ease-chatcn-out-soft transition-opacity duration-200 motion-reduce:transition-none",
            status === "loaded" ? "opacity-0" : "opacity-100",
          )}
        />
      ) : null}

      {status === "error" ? (
        // One centred unit: what happened, then the way out, in the spot where the image was.
        <span className="absolute inset-0 flex flex-col items-center justify-center gap-3 p-4 text-center">
          <span
            role="img"
            aria-label={`${alt}. The image could not load.`}
            className="text-chatcn-fg-subtle flex flex-col items-center gap-2"
          >
            <ImageOffIcon aria-hidden="true" className="size-5" />
            <span className="chatcn-text-13">Image unavailable</span>
          </span>
          <Button variant="outline" size="xs" onClick={retry} className="pointer-coarse:h-11">
            <RotateCcwIcon />
            Retry
          </Button>
        </span>
      ) : (
        <img
          key={attempt}
          ref={ref}
          src={src}
          alt={alt}
          width={width}
          height={height}
          loading={priority ? "eager" : "lazy"}
          decoding="async"
          draggable={false}
          onLoad={onLoad}
          onError={onError}
          className={cn(
            "relative block size-full",
            fit === "cover" ? "object-cover" : "object-contain",
            !instant && "ease-chatcn-out-soft transition-opacity duration-200",
            status === "loaded" ? "opacity-100" : "opacity-0",
            imgClassName,
          )}
        />
      )}
      <span aria-hidden="true" className={HAIRLINE} />
    </span>
  )

  return (
    <figure
      data-slot="image"
      className={cn("rounded-chatcn-lg relative m-0 flex max-w-full flex-col gap-2", className)}
      style={style}
      {...props}
    >
      {interactive ? (
        <button
          type="button"
          aria-label={`Open image: ${alt}`}
          onClick={() => (onOpen ? onOpen() : setOpen(true))}
          className={cn(
            "chatcn-focus group/image relative block size-full cursor-zoom-in rounded-[inherit]",
          )}
        >
          {frame}
          <span
            aria-hidden="true"
            className={cn(
              "bg-chatcn-alpha-200 pointer-events-none absolute inset-0 rounded-[inherit] opacity-0",
              "ease-chatcn-standard transition-opacity duration-150",
              "[@media(hover:hover)_and_(pointer:fine)]:group-hover/image:opacity-100",
            )}
          />
        </button>
      ) : (
        frame
      )}

      {caption ? (
        <figcaption className="chatcn-text-13 text-chatcn-fg-subtle text-pretty">
          {caption}
        </figcaption>
      ) : null}

      {lightbox && !onOpen ? (
        <ImageLightbox
          images={[{ src, alt, width, height, caption, filename }]}
          index={0}
          open={open}
          onOpenChange={setOpen}
          download={download}
        />
      ) : null}
    </figure>
  )
}

export type { ImageLightboxProps, ImageProps, ImageSource, ImageStatus }
export { Image, ImageLightbox }
