// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import { DropdownMenu as Menu } from "radix-ui"
import * as React from "react"
import { buttonVariants } from "@/components/ui/chatcn/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/chatcn/dialog"
import { CheckIcon, ChevronDownIcon, CopyIcon, Maximize2Icon } from "@/components/ui/chatcn/lib/chatcn-icons"
import { cn } from "@/lib/utils"

/* -------------------------------------------------------------------------------------------------
 * Export helpers
 * -----------------------------------------------------------------------------------------------*/

type TableFormat = "csv" | "tsv" | "markdown"

interface TableData {
  headers: string[]
  rows: string[][]
}

const clean = (text: string | null) => (text ?? "").replace(/\s+/g, " ").trim()

/** Reads headers and rows out of a rendered table, so any children can be exported. */
function readTable(table: HTMLTableElement): TableData {
  const all = Array.from(table.rows)
  const headRows = all.filter((r) => r.parentElement?.tagName === "THEAD")
  const head = headRows.at(-1) ?? (all[0]?.querySelector("th") ? all[0] : undefined)
  const headers = head ? Array.from(head.cells).map((c) => clean(c.textContent)) : []
  const rows = all
    .filter((r) => r !== head && !headRows.includes(r))
    .map((r) => Array.from(r.cells).map((c) => clean(c.textContent)))
  return { headers, rows }
}

function toDelimited({ headers, rows }: TableData, delimiter: "," | "\t") {
  const cell = (value: string) =>
    delimiter === "," && /[",\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value
  return [headers, ...rows]
    .filter((r) => r.length)
    .map((r) => r.map(cell).join(delimiter))
    .join("\n")
}

function toMarkdown({ headers, rows }: TableData) {
  const width = Math.max(headers.length, ...rows.map((r) => r.length))
  const cell = (value = "") => value.replace(/\|/g, "\\|")
  const line = (r: string[]) =>
    `| ${Array.from({ length: width }, (_, i) => cell(r[i])).join(" | ")} |`
  return [
    line(headers),
    `| ${Array.from({ length: width }, () => "---").join(" | ")} |`,
    ...rows.map(line),
  ].join("\n")
}

/** Serializes table data as CSV, TSV or a Markdown table. */
function formatTable(data: TableData, format: TableFormat) {
  if (format === "markdown") return toMarkdown(data)
  return toDelimited(data, format === "csv" ? "," : "\t")
}

/* -------------------------------------------------------------------------------------------------
 * Table
 * -----------------------------------------------------------------------------------------------*/

interface TableProps extends Omit<React.ComponentProps<"figure">, "children" | "onCopy"> {
  /** `thead` and `tbody` elements, as a Markdown renderer or your own JSX produces them. */
  children?: React.ReactNode
  /** Or plain data: one header row and body rows. */
  data?: TableData
  /** Short name, used for the region label and the expanded view title. */
  caption?: string
  /** Max height before the body scrolls under the sticky header. Default 400 px. */
  maxHeight?: number | string
  /** Hide copy and expand. */
  hideActions?: boolean
  onCopy?: (format: TableFormat, text: string) => void
}

/** Numbers, money, percentages and durations: "96 ms", "$4.00", "0.02%", "1.2M", "-3". */
const NUMERIC = /^[-+\u2212]?[$\u20ac\u00a3\u00a5]?\d[\d,.]*\s?(%|[kKMBx]|ms|s|m|h|[kKMGT]?B)?$/

/** Columns where every filled cell is a number line up on the right, the way figures compare. */
function numericColumns({ headers, rows }: TableData) {
  const width = Math.max(headers.length, ...rows.map((r) => r.length))
  return Array.from({ length: width }, (_, c) => {
    const cells = rows.map((r) => (r[c] ?? "").trim()).filter(Boolean)
    return cells.length > 0 && cells.every((cell) => NUMERIC.test(cell))
  })
}

/** Columns with sentences in them keep a readable width; the table scrolls instead of stacking words. */
function proseColumns({ headers, rows }: TableData) {
  const width = Math.max(headers.length, ...rows.map((r) => r.length))
  return Array.from({ length: width }, (_, c) => rows.some((r) => (r[c] ?? "").trim().length > 24))
}

function TableGrid({
  children,
  data,
  tableRef,
}: {
  children?: React.ReactNode
  data?: TableData
  tableRef?: React.Ref<HTMLTableElement>
}) {
  const numeric = React.useMemo(() => (data ? numericColumns(data) : []), [data])
  const prose = React.useMemo(() => (data ? proseColumns(data) : []), [data])
  return (
    <table
      ref={tableRef}
      className={cn(
        "chatcn-text-14 w-full border-separate border-spacing-0 text-start",
        // Header: sticky, quiet, never wraps.
        "[&_th]:chatcn-text-13 [&_th]:text-chatcn-fg-muted [&_th]:h-9 [&_th]:px-3 [&_th]:text-start [&_th]:font-medium [&_th]:whitespace-nowrap",
        "[&_th]:border-chatcn-border [&_th]:bg-chatcn-bg-subtle [&_th]:border-b",
        "[&_thead_th]:sticky [&_thead_th]:top-0 [&_thead_th]:z-10",
        // Body: hairline rows, numbers line up. Cells are opaque so a pinned column can cover
        // what scrolls under it; hover tints the whole row so a wide row reads across.
        "[&_td]:border-chatcn-border [&_td]:bg-chatcn-bg [&_td]:border-b [&_td]:px-3 [&_td]:py-2.5 [&_td]:align-top [&_td]:tabular-nums",
        "[@media(hover:hover)]:[&_tbody_tr:hover_td]:bg-[linear-gradient(var(--chatcn-alpha-200),var(--chatcn-alpha-200))]",
        "[&_tbody_tr:last-child_td]:border-b-0 [&_td]:text-pretty",
        // No column narrower than a short phrase: wide tables scroll instead of stacking words.
        "[&_td]:min-w-24 [&_td[data-prose]]:min-w-48",
        "[&_[align=center]]:text-center [&_[align=right]]:text-end",
        "[&_td_code]:text-[0.85em]",
      )}
    >
      {data ? (
        <>
          <thead>
            <tr>
              {data.headers.map((h, i) => (
                <th key={i} scope="col" align={numeric[i] ? "right" : undefined}>
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {data.rows.map((row, r) => (
              <tr key={r}>
                {row.map((cell, c) => (
                  <td
                    key={c}
                    align={numeric[c] ? "right" : undefined}
                    className={prose[c] ? "min-w-48" : undefined}
                  >
                    {cell}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </>
      ) : (
        children
      )}
    </table>
  )
}

const menuItem = cn(
  "chatcn-text-13 text-chatcn-fg rounded-chatcn-sm flex h-8 cursor-default items-center gap-2 px-2.5 outline-none select-none",
  "data-[highlighted]:bg-chatcn-fill-hover",
)

function TableCopyMenu({
  read,
  onCopy,
}: {
  read: () => TableData | undefined
  onCopy?: TableProps["onCopy"]
}) {
  const [copied, setCopied] = React.useState(false)
  React.useEffect(() => {
    if (!copied) return
    const id = window.setTimeout(() => setCopied(false), 1500)
    return () => window.clearTimeout(id)
  }, [copied])

  const copy = (format: TableFormat) => {
    const data = read()
    if (!data) return
    const text = formatTable(data, format)
    void navigator.clipboard?.writeText(text).then(() => {
      setCopied(true)
      onCopy?.(format, text)
    })
  }

  const layer = "absolute transition-[opacity,scale,filter] duration-150 ease-chatcn-out"
  return (
    <Menu.Root>
      <Menu.Trigger className={cn(buttonVariants({ variant: "ghost", size: "xs" }), "ps-1.5 pe-1")}>
        <span className="relative grid size-3.5 place-items-center">
          <CopyIcon
            className={cn(layer, copied ? "scale-75 opacity-0 blur-[2px]" : "opacity-100")}
          />
          <CheckIcon
            className={cn(layer, copied ? "opacity-100" : "scale-75 opacity-0 blur-[2px]")}
          />
        </span>
        {copied ? "Copied" : "Copy"}
        <ChevronDownIcon className="text-chatcn-fg-subtle" />
        <span aria-live="polite" className="sr-only">
          {copied ? "Table copied" : ""}
        </span>
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Content
          loop
          collisionPadding={5}
          side="bottom"
          align="end"
          sideOffset={4}
          className={cn(
            "z-50",
            "bg-chatcn-surface shadow-chatcn-lg rounded-chatcn-lg min-w-44 origin-(--radix-dropdown-menu-content-transform-origin) p-1 outline-none",
            "ease-chatcn-out transition-[opacity,scale] duration-180",
            "starting:scale-(--chatcn-enter-scale) starting:opacity-0",
            "data-[state=closed]:scale-(--chatcn-exit-scale) data-[state=closed]:opacity-0 data-[state=closed]:duration-140 data-[state=closed]:animate-[chatcn-exit-hold_var(--tw-duration,150ms)]",
            "data-[instant]:transition-none",
          )}
        >
          <Menu.Item className={menuItem} onSelect={() => copy("markdown")}>
            Copy as Markdown
          </Menu.Item>
          <Menu.Item className={menuItem} onSelect={() => copy("csv")}>
            Copy as CSV
          </Menu.Item>
          <Menu.Item className={menuItem} onSelect={() => copy("tsv")}>
            Copy for spreadsheets
          </Menu.Item>
        </Menu.Content>
      </Menu.Portal>
    </Menu.Root>
  )
}

/**
 * A table that stays readable at any width: the body scrolls in both directions under a sticky
 * header, and the whole table copies as Markdown, CSV or TSV, or opens in a larger view.
 */
function Table({
  children,
  data,
  caption,
  maxHeight = 400,
  hideActions = false,
  onCopy,
  className,
  ...props
}: TableProps) {
  const tableRef = React.useRef<HTMLTableElement>(null)
  const scrollRef = React.useRef<HTMLElement>(null)
  const [expanded, setExpanded] = React.useState(false)

  // Tables built from children (Markdown, your own JSX) get the same rule as `data` tables:
  // sentence columns keep a readable width and the table scrolls instead of stacking words.
  React.useLayoutEffect(() => {
    const table = tableRef.current
    if (data || !table) return
    const prose = proseColumns(readTable(table))
    for (const body of Array.from(table.tBodies))
      for (const row of Array.from(body.rows))
        Array.from(row.cells).forEach((cell, c) => {
          cell.toggleAttribute("data-prose", prose[c] === true)
        })
  })

  // Soft edges only on the sides that have more to scroll to, and at the bottom while rows
  // continue below, so a row is never cut mid-height without a cue. Attributes, not state:
  // scrolling never re-renders the table.
  const measureEdges = React.useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    const start = Math.abs(el.scrollLeft)
    el.toggleAttribute("data-scroll-start", start > 1)
    el.toggleAttribute("data-scroll-end", start + el.clientWidth < el.scrollWidth - 1)
    el.toggleAttribute("data-scroll-bottom", el.scrollTop + el.clientHeight < el.scrollHeight - 1)
  }, [])
  // A table wider than its frame pins its first column, so each row keeps its name while the
  // figures scroll past. Only when that column is narrow enough to leave room for the rest.
  const measure = React.useCallback(() => {
    const el = scrollRef.current
    const first = tableRef.current?.rows[0]?.cells[0]
    if (!el) return
    const wide = el.scrollWidth > el.clientWidth + 1
    const pin = wide && first !== undefined && first.offsetWidth <= el.clientWidth * 0.4
    el.toggleAttribute("data-pin", pin)
    el.style.setProperty("--pin-w", pin && first ? `${first.offsetWidth}px` : "0px")
    measureEdges()
  }, [measureEdges])
  React.useEffect(() => {
    const el = scrollRef.current
    if (!el) return
    measure()
    if (typeof ResizeObserver === "undefined") return
    const observer = new ResizeObserver(measure)
    observer.observe(el)
    if (tableRef.current) observer.observe(tableRef.current)
    return () => observer.disconnect()
  }, [measure])
  const [size, setSize] = React.useState<{ rows: number; columns: number }>()
  const read = React.useCallback(
    () => data ?? (tableRef.current ? readTable(tableRef.current) : undefined),
    [data],
  )

  const open = () => {
    const d = read()
    if (d)
      setSize({ rows: d.rows.length, columns: Math.max(d.headers.length, d.rows[0]?.length ?? 0) })
    setExpanded(true)
  }

  const label = caption ?? "Table"
  return (
    <figure data-slot="table" className={cn("group/table relative min-w-0", className)} {...props}>
      {/* The focus ring lives on this frame, outside the scroll area, so the sticky header can
          never paint over it. */}
      <div className="rounded-chatcn-lg bg-chatcn-bg shadow-chatcn-border has-[>section:focus-visible]:outline-chatcn-accent overflow-hidden has-[>section:focus-visible]:outline-2 has-[>section:focus-visible]:outline-offset-2 has-[>section:focus-visible]:outline-solid forced-colors:outline-1 forced-colors:outline-solid">
        <section
          aria-label={label}
          // biome-ignore lint/a11y/noNoninteractiveTabindex: scrollable regions must be reachable by keyboard
          tabIndex={0}
          ref={scrollRef}
          onScroll={measureEdges}
          className={cn(
            "chatcn-scroll overflow-auto overscroll-x-contain outline-none [--pin-w:0px]",
            "[--edge-bottom:black] [--edge-end:black] [--edge-start:black] [--fade-bottom:0px] [--fade-end:0px] [--fade-start:0px]",
            "data-[scroll-start]:[--edge-start:transparent] data-[scroll-start]:[--fade-start:32px]",
            "data-[scroll-end]:[--edge-end:transparent] data-[scroll-end]:[--fade-end:32px]",
            "data-[scroll-bottom]:[--edge-bottom:transparent] data-[scroll-bottom]:[--fade-bottom:32px]",
            // The start fade begins after a pinned column, so rows slide out from under it.
            "[--sides:linear-gradient(to_right,black_var(--pin-w),var(--edge-start)_var(--pin-w),black_calc(var(--pin-w)+var(--fade-start)),black_calc(100%-var(--fade-end)),var(--edge-end))]",
            "rtl:[--sides:linear-gradient(to_left,black_var(--pin-w),var(--edge-start)_var(--pin-w),black_calc(var(--pin-w)+var(--fade-start)),black_calc(100%-var(--fade-end)),var(--edge-end))]",
            "[mask-image:var(--sides),linear-gradient(to_bottom,black_calc(100%-var(--fade-bottom)),var(--edge-bottom))] [mask-composite:intersect]",
            "focus-visible:[mask-image:none]",
            // Pinned first column: covers what scrolls under it, a hairline only once scrolled.
            "data-[pin]:[&_tr>:first-child]:sticky data-[pin]:[&_tr>:first-child]:start-0 data-[pin]:[&_tr>:first-child]:z-[5]",
            "data-[pin]:[&_thead_tr>:first-child]:z-20",
            "data-[pin]:data-[scroll-start]:[&_tr>:first-child]:shadow-[inset_-1px_0_0_var(--chatcn-border)]",
            "rtl:data-[pin]:data-[scroll-start]:[&_tr>:first-child]:shadow-[inset_1px_0_0_var(--chatcn-border)]",
          )}
          style={{ maxHeight: typeof maxHeight === "number" ? `${maxHeight}px` : maxHeight }}
        >
          <TableGrid tableRef={tableRef} data={data}>
            {children}
          </TableGrid>
        </section>
      </div>
      {hideActions ? null : (
        <div
          data-slot="table-actions"
          className={cn(
            // Touch: a row under the table, always there.
            "flex h-8 items-center justify-end gap-0.5 pt-1",
            // Desktop: floats in the header row on hover or focus, instantly, so it costs no space.
            "pointer-fine:bg-chatcn-bg-subtle pointer-fine:rounded-chatcn-md pointer-fine:absolute pointer-fine:end-1.5 pointer-fine:top-1.5 pointer-fine:z-20 pointer-fine:h-auto pointer-fine:p-0 pointer-fine:ps-2",
            "pointer-fine:opacity-0 pointer-fine:group-focus-within/table:opacity-100 pointer-fine:group-hover/table:opacity-100",
            "has-[[data-state=open]]:opacity-100",
          )}
        >
          <TableCopyMenu read={read} onCopy={onCopy} />
          <button
            type="button"
            onClick={open}
            className={cn(buttonVariants({ variant: "ghost", size: "xs" }), "ps-1.5")}
          >
            <Maximize2Icon />
            Expand
          </button>
        </div>
      )}
      <Dialog open={expanded} onOpenChange={setExpanded}>
        <DialogContent size="xl" className="max-h-[calc(100dvh-4rem)]">
          <DialogHeader className="pb-4">
            <DialogTitle>{label}</DialogTitle>
            {size ? (
              <DialogDescription className="tabular-nums">
                {size.rows} {size.rows === 1 ? "row" : "rows"}, {size.columns}{" "}
                {size.columns === 1 ? "column" : "columns"}
              </DialogDescription>
            ) : null}
          </DialogHeader>
          <div className="flex min-h-0 flex-1 flex-col px-5 pb-5">
            <div className="rounded-chatcn-lg shadow-chatcn-border chatcn-scroll min-h-0 overflow-auto">
              <TableGrid data={data}>{children}</TableGrid>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </figure>
  )
}

export type { TableData, TableFormat, TableProps }
export { formatTable, readTable, Table }
