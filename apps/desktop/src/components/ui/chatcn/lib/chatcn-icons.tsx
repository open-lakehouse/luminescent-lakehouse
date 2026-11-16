// From chatcn (https://chatcn.dev), under the chatcn License, not this repository's
// Apache-2.0: see NOTICE.md in src/components/ui/chatcn.
import {
  type CSSProperties,
  createElement,
  type ForwardRefExoticComponent,
  forwardRef,
  type RefAttributes,
  type SVGAttributes,
} from "react"

/** Shape elements an icon may contain. */
export type IconElement = "path" | "circle" | "rect" | "ellipse" | "line" | "polyline" | "polygon"

/** An icon's drawing: a flat list of [element, attributes] pairs, lucide compatible. */
export type IconNode = ReadonlyArray<readonly [IconElement, Readonly<Record<string, string>>]>

export interface IconProps extends SVGAttributes<SVGSVGElement> {
  /** Width and height. Number means px. Default 24. */
  size?: number | string
  /** Stroke width in viewBox units (24 grid). Default 1.5. */
  strokeWidth?: number | string
  /** Keep the rendered stroke at `strokeWidth` px at any size. */
  absoluteStrokeWidth?: boolean
  /** Stroke color. Default currentColor. */
  color?: string
  /** Accessible name. Renders <title> and sets role="img"; without it the icon is aria-hidden. */
  title?: string
  /**
   * Draw the icon stroke by stroke (needs `chatcn-icons/draw.css` imported once, or the chatcn
   * theme, which carries the same rules). `"hover"`: when the icon is hovered or focused.
   * `"parent"` (or `true`): when the nearest `button`, `a`, `[role=button]` or
   * `[data-icon-trigger]` is. `"mount"`: once, when it appears. Reduced motion keeps the icon
   * still. Without the prop nothing is added.
   */
  draw?: boolean | IconDraw
}

/** When an icon draws itself. */
export type IconDraw = "hover" | "parent" | "mount"

export type Icon = ForwardRefExoticComponent<IconProps & RefAttributes<SVGSVGElement>>

/** Drop-in aliases for code migrating from lucide-react. */
export type LucideIcon = Icon
export type LucideProps = IconProps

const toPascal = (name: string): string =>
  name
    .split("-")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join("")

/**
 * Builds a React icon component from an icon node.
 * `name` is the kebab-case icon name, used for the class `chatcn-icon-<name>` and displayName.
 */
export function createIcon(name: string, iconNode: IconNode): Icon {
  const Component = forwardRef<SVGSVGElement, IconProps>(function ChatcnIcon(
    {
      size = 24,
      strokeWidth = 1.5,
      absoluteStrokeWidth = false,
      color = "currentColor",
      title,
      draw,
      className,
      children,
      ...rest
    },
    ref,
  ) {
    const stroke = absoluteStrokeWidth
      ? (Number(strokeWidth) * 24) / Number.parseFloat(String(size))
      : strokeWidth
    const labelled = Boolean(title || rest["aria-label"] || rest["aria-labelledby"])
    const drawMode = draw === true ? "parent" : draw || undefined
    return createElement(
      "svg",
      {
        ref,
        xmlns: "http://www.w3.org/2000/svg",
        width: size,
        height: size,
        viewBox: "0 0 24 24",
        fill: "none",
        stroke: color,
        strokeWidth: stroke,
        strokeLinecap: "round",
        strokeLinejoin: "round",
        "data-slot": "icon",
        className: className
          ? `chatcn-icon chatcn-icon-${name} ${className}`
          : `chatcn-icon chatcn-icon-${name}`,
        ...(labelled ? { role: "img" } : { "aria-hidden": true }),
        ...(drawMode ? { "data-draw": drawMode } : null),
        ...rest,
      },
      title ? createElement("title", null, title) : null,
      ...iconNode.map(([tag, attrs], i) =>
        createElement(
          tag,
          drawMode
            ? // Path order is draw order: each path's index staggers it (draw.css caps it at 3).
              { key: i, ...attrs, pathLength: 1, style: { "--ci-i": i } as CSSProperties }
            : { key: i, ...attrs },
        ),
      ),
      children,
    )
  })
  Component.displayName = `${toPascal(name)}Icon`
  return Component
}

export const AgentIcon: Icon = createIcon("agent", [
  ["path", { d: "M18.32 17.3a8.25 8.25 0 1 1 0-10.6" }],
  ["path", { d: "M16 8.5h2.5A1.5 1.5 0 0 0 20 7V4.5" }],
  [
    "path",
    {
      d: "M11 9.75h2A1.25 1.25 0 0 1 14.25 11v2A1.25 1.25 0 0 1 13 14.25h-2A1.25 1.25 0 0 1 9.75 13v-2A1.25 1.25 0 0 1 11 9.75",
    },
  ],
])

export const AlignCenterIcon: Icon = createIcon("align-center", [
  ["path", { d: "M3.5 6h17" }],
  ["path", { d: "M7 12h10" }],
  ["path", { d: "M5 18h14" }],
])

export const AlignJustifyIcon: Icon = createIcon("align-justify", [
  ["path", { d: "M3.5 6h17" }],
  ["path", { d: "M3.5 12h17" }],
  ["path", { d: "M3.5 18h17" }],
])

export const AlignLeftIcon: Icon = createIcon("align-left", [
  ["path", { d: "M3.5 6h17" }],
  ["path", { d: "M3.5 12h11" }],
  ["path", { d: "M3.5 18h14" }],
])

export const AlignRightIcon: Icon = createIcon("align-right", [
  ["path", { d: "M20.5 6h-17" }],
  ["path", { d: "M20.5 12h-11" }],
  ["path", { d: "M20.5 18h-14" }],
])

export const ArchiveIcon: Icon = createIcon("archive", [
  [
    "path",
    { d: "M4.75 3.5h14.5a2 2 0 0 1 2 2v1a2 2 0 0 1-2 2H4.75a2 2 0 0 1-2-2v-1a2 2 0 0 1 2-2" },
  ],
  ["path", { d: "M4.5 11.5v6.75a2.5 2.5 0 0 0 2.5 2.5h10a2.5 2.5 0 0 0 2.5-2.5V11.5" }],
  ["path", { d: "M10 12.75h4" }],
])

export const ArrowDownIcon: Icon = createIcon("arrow-down", [
  ["path", { d: "m5.5 12.5 4.73 4.73a2.5 2.5 0 0 0 3.54 0l4.73-4.73" }],
  ["path", { d: "M12 5v9" }],
])

export const ArrowDownLeftIcon: Icon = createIcon("arrow-down-left", [
  ["path", { d: "M7 7v7.5A2.5 2.5 0 0 0 9.5 17H17" }],
  ["path", { d: "m17 7-6.46 6.46" }],
])

export const ArrowDownRightIcon: Icon = createIcon("arrow-down-right", [
  ["path", { d: "M17 7v7.5a2.5 2.5 0 0 1-2.5 2.5H7" }],
  ["path", { d: "m7 7 6.46 6.46" }],
])

export const ArrowLeftIcon: Icon = createIcon("arrow-left", [
  ["path", { d: "m11.5 5.5-4.73 4.73a2.5 2.5 0 0 0 0 3.54l4.73 4.73" }],
  ["path", { d: "M19 12h-9" }],
])

export const ArrowRightIcon: Icon = createIcon("arrow-right", [
  ["path", { d: "m12.5 18.5 4.73-4.73a2.5 2.5 0 0 0 0-3.54L12.5 5.5" }],
  ["path", { d: "M5 12h9" }],
])

export const ArrowRightLeftIcon: Icon = createIcon("arrow-right-left", [
  ["path", { d: "m16.5 11 2.59-2.59a2 2 0 0 0 0-2.82L16.5 3" }],
  ["path", { d: "M3.5 7H16" }],
  ["path", { d: "m7.5 13-2.59 2.59a2 2 0 0 0 0 2.82L7.5 21" }],
  ["path", { d: "M20.5 17H8" }],
])

export const ArrowUpIcon: Icon = createIcon("arrow-up", [
  ["path", { d: "m18.5 11.5-4.73-4.73a2.5 2.5 0 0 0-3.54 0L5.5 11.5" }],
  ["path", { d: "M12 19v-9" }],
])

export const ArrowUpDownIcon: Icon = createIcon("arrow-up-down", [
  ["path", { d: "M10.5 7.25 8.41 5.16a2 2 0 0 0-2.82 0L3.5 7.25" }],
  ["path", { d: "M7 20.25V8" }],
  ["path", { d: "m13.5 16.75 2.09 2.09a2 2 0 0 0 2.82 0l2.09-2.09" }],
  ["path", { d: "M17 3.75V16" }],
])

export const ArrowUpRightIcon: Icon = createIcon("arrow-up-right", [
  ["path", { d: "M7 7h7.5A2.5 2.5 0 0 1 17 9.5V17" }],
  ["path", { d: "m7 17 6.46-6.46" }],
])

export const ArtifactIcon: Icon = createIcon("artifact", [
  ["path", { d: "M6 4.5h12a3 3 0 0 1 3 3v9a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3v-9a3 3 0 0 1 3-3" }],
  [
    "path",
    {
      d: "M13 11h3.5a1.5 1.5 0 0 1 1.5 1.5V15a1.5 1.5 0 0 1-1.5 1.5H13a1.5 1.5 0 0 1-1.5-1.5v-2.5A1.5 1.5 0 0 1 13 11",
    },
  ],
])

export const AtSignIcon: Icon = createIcon("at-sign", [
  ["path", { d: "M8.25 12a3.75 3.75 0 1 0 7.5 0 3.75 3.75 0 1 0-7.5 0" }],
  ["path", { d: "M15.75 8.25V13a2.38 2.38 0 0 0 4.75 0v-1a8.5 8.5 0 1 0-3.62 6.96" }],
])

export const AudioLinesIcon: Icon = createIcon("audio-lines", [
  ["path", { d: "M2.75 10.5v3" }],
  ["path", { d: "M6.45 7v10" }],
  ["path", { d: "M10.15 3.5v17" }],
  ["path", { d: "M13.85 8.5v7" }],
  ["path", { d: "M17.55 5.5v13" }],
  ["path", { d: "M21.25 10.5v3" }],
])

export const BadgeCheckIcon: Icon = createIcon("badge-check", [
  [
    "path",
    {
      d: "m13.62 3.88.92.78a1.5 1.5 0 0 0 .85.36l1.2.09a2.5 2.5 0 0 1 2.3 2.3l.09 1.2a1.5 1.5 0 0 0 .36.85l.78.92a2.5 2.5 0 0 1 0 3.24l-.78.92a1.5 1.5 0 0 0-.36.85l-.09 1.2a2.5 2.5 0 0 1-2.3 2.3l-1.2.09a1.5 1.5 0 0 0-.85.36l-.92.78a2.5 2.5 0 0 1-3.24 0l-.92-.78a1.5 1.5 0 0 0-.85-.36l-1.2-.09a2.5 2.5 0 0 1-2.3-2.3l-.09-1.2a1.5 1.5 0 0 0-.36-.85l-.78-.92a2.5 2.5 0 0 1 0-3.24l.78-.92a1.5 1.5 0 0 0 .36-.85l.09-1.2a2.5 2.5 0 0 1 2.3-2.3l1.2-.09a1.5 1.5 0 0 0 .85-.36l.92-.78a2.5 2.5 0 0 1 3.24 0",
    },
  ],
  ["path", { d: "m8.25 12.25 1.21 1.21a1.5 1.5 0 0 0 2.1.03l4.19-3.99" }],
])

export const BanIcon: Icon = createIcon("ban", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "m7.76 7.76 8.48 8.48" }],
])

export const BatteryFullIcon: Icon = createIcon("battery-full", [
  ["path", { d: "M5.5 6.5h10a3 3 0 0 1 3 3v5a3 3 0 0 1-3 3h-10a3 3 0 0 1-3-3v-5a3 3 0 0 1 3-3" }],
  ["path", { d: "M21.5 10.5v3" }],
  ["path", { d: "M6.5 10.5v3" }],
  ["path", { d: "M10.5 10.5v3" }],
  ["path", { d: "M14.5 10.5v3" }],
])

export const BellIcon: Icon = createIcon("bell", [
  [
    "path",
    {
      d: "M6 12.53V9.5a6 6 0 0 1 12 0v3.03a2 2 0 0 0 .21.89l.7 1.41A1.5 1.5 0 0 1 17.57 17H6.43a1.5 1.5 0 0 1-1.34-2.17l.7-1.41a2 2 0 0 0 .21-.89",
    },
  ],
  ["path", { d: "M14 20a2.5 2.5 0 0 1-4 0" }],
])

export const BellRingIcon: Icon = createIcon("bell-ring", [
  [
    "path",
    {
      d: "M6 12.53V9.5a6 6 0 0 1 12 0v3.03a2 2 0 0 0 .21.89l.7 1.41A1.5 1.5 0 0 1 17.57 17H6.43a1.5 1.5 0 0 1-1.34-2.17l.7-1.41a2 2 0 0 0 .21-.89",
    },
  ],
  ["path", { d: "M14 20a2.5 2.5 0 0 1-4 0" }],
  ["path", { d: "M2.82 7.04a9.5 9.5 0 0 1 2.82-4.6" }],
  ["path", { d: "M18.36 2.44a9.5 9.5 0 0 1 2.82 4.6" }],
])

export const BinaryIcon: Icon = createIcon("binary", [
  [
    "path",
    {
      d: "M7.38 3.5a2.38 2.38 0 0 1 2.37 2.38v2.25a2.38 2.38 0 0 1-2.37 2.37A2.38 2.38 0 0 1 5 8.13V5.88A2.38 2.38 0 0 1 7.38 3.5",
    },
  ],
  ["path", { d: "m14.5 5.75.83-.75a1 1 0 0 1 1.67.75v4.75" }],
  ["path", { d: "m6 15.75.83-.75a1 1 0 0 1 1.67.75v4.75" }],
  [
    "path",
    {
      d: "M16.63 13.5A2.38 2.38 0 0 1 19 15.88v2.25a2.38 2.38 0 0 1-2.37 2.37 2.38 2.38 0 0 1-2.38-2.37v-2.25a2.38 2.38 0 0 1 2.38-2.38",
    },
  ],
])

export const BookIcon: Icon = createIcon("book", [
  [
    "path",
    { d: "M4 19V5.5A2.5 2.5 0 0 1 6.5 3h12A1.5 1.5 0 0 1 20 4.5v15a1.5 1.5 0 0 1-1.5 1.5H6" },
  ],
  ["path", { d: "M20 17H6a2 2 0 0 0 0 4" }],
])

export const BookOpenIcon: Icon = createIcon("book-open", [
  [
    "path",
    {
      d: "M2.75 5.5A1.5 1.5 0 0 1 4.25 4H8a4 4 0 0 1 4 4 4 4 0 0 1 4-4h3.75a1.5 1.5 0 0 1 1.5 1.5v11a1.5 1.5 0 0 1-1.5 1.5H15a3 3 0 0 0-3 3 3 3 0 0 0-3-3H4.25a1.5 1.5 0 0 1-1.5-1.5Z",
    },
  ],
  ["path", { d: "M12 11v7" }],
])

export const BookmarkIcon: Icon = createIcon("bookmark", [
  [
    "path",
    {
      d: "M8 3h8a2.5 2.5 0 0 1 2.5 2.5v13.19a1.25 1.25 0 0 1-1.93 1.05l-3.48-2.27a2 2 0 0 0-2.18 0l-3.48 2.27a1.25 1.25 0 0 1-1.93-1.05V5.5A2.5 2.5 0 0 1 8 3",
    },
  ],
])

export const BotIcon: Icon = createIcon("bot", [
  ["path", { d: "M7 8h10a3 3 0 0 1 3 3v6.5a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3V11a3 3 0 0 1 3-3" }],
  ["path", { d: "M11.1 4.5a.9.9 0 1 0 1.8 0 .9.9 0 1 0-1.8 0" }],
  ["path", { d: "M9 12.75V15" }],
  ["path", { d: "M15 12.75V15" }],
])

export const BoxIcon: Icon = createIcon("box", [
  [
    "path",
    {
      d: "m13 2.68 6.57 3.79a2 2 0 0 1 1 1.73v7.6a2 2 0 0 1-1 1.73L13 21.32a2 2 0 0 1-2 0l-6.57-3.79a2 2 0 0 1-1-1.73V8.2a2 2 0 0 1 1-1.73L11 2.68a2 2 0 0 1 2 0",
    },
  ],
  ["path", { d: "M6.25 8.7 11 11.43a2 2 0 0 0 2 0l4.75-2.73" }],
  ["path", { d: "M12 11.7v6.9" }],
])

export const BracesIcon: Icon = createIcon("braces", [
  ["path", { d: "M9.5 3.5h-1a2 2 0 0 0-2 2V10a2 2 0 0 1-2 2h-1" }],
  ["path", { d: "M9.5 20.5h-1a2 2 0 0 1-2-2V14a2 2 0 0 0-2-2h-1" }],
  ["path", { d: "M14.5 3.5h1a2 2 0 0 1 2 2V10a2 2 0 0 0 2 2h1" }],
  ["path", { d: "M14.5 20.5h1a2 2 0 0 0 2-2V14a2 2 0 0 1 2-2h1" }],
])

export const BrainIcon: Icon = createIcon("brain", [
  [
    "path",
    {
      d: "M10.38 4.94a3.25 3.25 0 0 0-4.44 4.44L5 9.97a3.5 3.5 0 0 0 .55 6.32l1.12.27a2.75 2.75 0 0 0 3.29 3.6",
    },
  ],
  [
    "path",
    {
      d: "M13.62 4.94a3.25 3.25 0 0 1 4.44 4.44l.94.59a3.5 3.5 0 0 1-.55 6.32l-1.12.27a2.75 2.75 0 0 1-3.29 3.6",
    },
  ],
  ["path", { d: "M12 4.75V20.5" }],
])

export const BriefcaseIcon: Icon = createIcon("briefcase", [
  ["path", { d: "M5.5 7h13a3 3 0 0 1 3 3v7.5a3 3 0 0 1-3 3h-13a3 3 0 0 1-3-3V10a3 3 0 0 1 3-3" }],
  ["path", { d: "M8.5 7V5A1.5 1.5 0 0 1 10 3.5h4A1.5 1.5 0 0 1 15.5 5v2" }],
])

export const BrushIcon: Icon = createIcon("brush", [
  ["path", { d: "m11 13 7.6-7.6a1.5 1.5 0 0 1 2.1 2.1L13 15.1" }],
  ["path", { d: "M9.5 14.25c-2.25 0-3.75 1.5-4.25 5L5 20l.75-.25c3.5-.5 5-2 5-4.25" }],
])

export const BugIcon: Icon = createIcon("bug", [
  ["path", { d: "M12 9a5 5 0 0 1 5 5v2a5 5 0 0 1-5 5 5 5 0 0 1-5-5v-2a5 5 0 0 1 5-5" }],
  ["path", { d: "M9 6.5a3 3 0 0 1 6 0" }],
  ["path", { d: "M12 12.5V18" }],
  ["path", { d: "M2.75 15H7" }],
  ["path", { d: "M21.25 15H17" }],
  ["path", { d: "m3.75 10 3.75 2.25" }],
  ["path", { d: "m20.25 10-3.75 2.25" }],
  ["path", { d: "m3.75 20.5 3.75-2.25" }],
  ["path", { d: "m20.25 20.5-3.75-2.25" }],
])

export const BuildingIcon: Icon = createIcon("building", [
  [
    "path",
    {
      d: "M7 2.5h10A2.5 2.5 0 0 1 19.5 5v14a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M8.5 6.5h.01" }],
  ["path", { d: "M12 6.5h.01" }],
  ["path", { d: "M15.5 6.5h.01" }],
  ["path", { d: "M8.5 10h.01" }],
  ["path", { d: "M12 10h.01" }],
  ["path", { d: "M15.5 10h.01" }],
  ["path", { d: "M8.5 13.5h.01" }],
  ["path", { d: "M12 13.5h.01" }],
  ["path", { d: "M15.5 13.5h.01" }],
  ["path", { d: "M10 21.5v-2.75a1.25 1.25 0 0 1 1.25-1.25h1.5A1.25 1.25 0 0 1 14 18.75v2.75" }],
])

export const CalendarIcon: Icon = createIcon("calendar", [
  ["path", { d: "M11 4.75h2" }],
  [
    "path",
    {
      d: "M18.98 4.92A3 3 0 0 1 21 7.75V18a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V7.75a3 3 0 0 1 2.02-2.83",
    },
  ],
  ["path", { d: "M8 2.5V7" }],
  ["path", { d: "M16 2.5V7" }],
  ["path", { d: "M6.25 10.25h11.5" }],
])

export const CalendarCheckIcon: Icon = createIcon("calendar-check", [
  ["path", { d: "M11 4.75h2" }],
  [
    "path",
    {
      d: "M18.98 4.92A3 3 0 0 1 21 7.75V18a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V7.75a3 3 0 0 1 2.02-2.83",
    },
  ],
  ["path", { d: "M8 2.5V7" }],
  ["path", { d: "M16 2.5V7" }],
  ["path", { d: "M6.25 10.25h11.5" }],
  ["path", { d: "m8.75 15.25 1.37 1.37a1.25 1.25 0 0 0 1.76 0l3.37-3.37" }],
])

export const CalendarClockIcon: Icon = createIcon("calendar-clock", [
  ["path", { d: "M11 4.75h2" }],
  ["path", { d: "M18.98 4.92A3 3 0 0 1 21 7.75v5.94" }],
  ["path", { d: "M13.69 21H6a3 3 0 0 1-3-3V7.75a3 3 0 0 1 2.02-2.83" }],
  ["path", { d: "M8 2.5V7" }],
  ["path", { d: "M16 2.5V7" }],
  ["path", { d: "M6.25 10.25h11.5" }],
  ["path", { d: "M14.25 18a3.75 3.75 0 1 0 7.5 0 3.75 3.75 0 1 0-7.5 0" }],
  ["path", { d: "M18 16.25v1.47a.5.5 0 0 0 .24.43l1.01.6" }],
])

export const CalendarPlusIcon: Icon = createIcon("calendar-plus", [
  ["path", { d: "M11 4.75h2" }],
  [
    "path",
    {
      d: "M18.98 4.92A3 3 0 0 1 21 7.75V18a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V7.75a3 3 0 0 1 2.02-2.83",
    },
  ],
  ["path", { d: "M8 2.5V7" }],
  ["path", { d: "M16 2.5V7" }],
  ["path", { d: "M6.25 10.25h11.5" }],
  ["path", { d: "M9.25 15.5h5.5" }],
  ["path", { d: "M12 12.75v5.5" }],
])

export const CameraIcon: Icon = createIcon("camera", [
  [
    "path",
    {
      d: "M5.25 7.5h1.03a1.25 1.25 0 0 0 1.08-.62l1.03-1.76a1.25 1.25 0 0 1 1.08-.62h5.06a1.25 1.25 0 0 1 1.08.62l1.03 1.76a1.25 1.25 0 0 0 1.08.62h1.03a2.5 2.5 0 0 1 2.5 2.5v7a2.5 2.5 0 0 1-2.5 2.5H5.25a2.5 2.5 0 0 1-2.5-2.5v-7a2.5 2.5 0 0 1 2.5-2.5",
    },
  ],
  ["path", { d: "M8.5 13.25a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0" }],
])

export const CameraOffIcon: Icon = createIcon("camera-off", [
  ["path", { d: "m4 4 16 16" }],
  [
    "path",
    {
      d: "M8.82 4.68a1.25 1.25 0 0 1 .65-.18h5.06a1.25 1.25 0 0 1 1.08.62l1.03 1.76a1.25 1.25 0 0 0 1.08.62h1.03a2.5 2.5 0 0 1 2.5 2.5v6.81",
    },
  ],
  ["path", { d: "M15.97 19.5H5.25a2.5 2.5 0 0 1-2.5-2.5v-7a2.5 2.5 0 0 1 1.52-2.3" }],
  ["path", { d: "M12.54 16.71a3.5 3.5 0 0 1-4-4" }],
  ["path", { d: "M13.22 9.97a3.5 3.5 0 0 1 2.06 2.06" }],
])

export const CaptionsIcon: Icon = createIcon("captions", [
  ["path", { d: "M6 5h12a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V8a3 3 0 0 1 3-3" }],
  ["path", { d: "M7 11h2.5" }],
  ["path", { d: "M12.5 11H17" }],
  ["path", { d: "M7 15h6" }],
  ["path", { d: "M16 15h1" }],
])

export const CarIcon: Icon = createIcon("car", [
  ["path", { d: "M9.25 17a2.25 2.25 0 1 1-4.5 0 2.25 2.25 0 1 1 4.5 0" }],
  ["path", { d: "M19.25 17a2.25 2.25 0 1 1-4.5 0 2.25 2.25 0 1 1 4.5 0" }],
  [
    "path",
    {
      d: "M2.75 14.59v-2.24a1.5 1.5 0 0 1 .16-.67l1.68-3.35a1.5 1.5 0 0 1 1.34-.83h6.45a1.5 1.5 0 0 1 1.06.44l2.72 2.72a1.5 1.5 0 0 0 .81.42l2.06.34a1.5 1.5 0 0 1 .81.42l1.04 1.04a1.25 1.25 0 0 1 .37.89v.82",
    },
  ],
])

export const ChartAreaIcon: Icon = createIcon("chart-area", [
  ["path", { d: "M3 3v15a3 3 0 0 0 3 3h15" }],
  [
    "path",
    {
      d: "M7 16.25v-2.98a1.25 1.25 0 0 1 .37-.89l2.28-2.28a1.25 1.25 0 0 1 1.73-.04l1.42 1.31a1.25 1.25 0 0 0 1.79-.1l1.71-1.98a1.25 1.25 0 0 1 2.2.82v6.14a1.25 1.25 0 0 1-1.25 1.25h-9A1.25 1.25 0 0 1 7 16.25",
    },
  ],
])

export const ChartBarIcon: Icon = createIcon("chart-bar", [
  ["path", { d: "M3 3v15a3 3 0 0 0 3 3h15" }],
  ["path", { d: "M6.75 6.5h3.75" }],
  ["path", { d: "M6.75 11.25H18.5" }],
  ["path", { d: "M6.75 16h7.5" }],
])

export const ChartColumnIcon: Icon = createIcon("chart-column", [
  ["path", { d: "M3 3v15a3 3 0 0 0 3 3h15" }],
  ["path", { d: "M8 18v-4" }],
  ["path", { d: "M12.75 18V6" }],
  ["path", { d: "M17.5 18v-8" }],
])

export const ChartLineIcon: Icon = createIcon("chart-line", [
  ["path", { d: "M3 3v15a3 3 0 0 0 3 3h15" }],
  ["path", { d: "m7 16 2.73-4a1.5 1.5 0 0 1 2.3-.22l.8.8a1.5 1.5 0 0 0 2.22-.12L19.5 7" }],
])

export const ChartScatterIcon: Icon = createIcon("chart-scatter", [
  ["path", { d: "M3 3v15a3 3 0 0 0 3 3h15" }],
  ["path", { d: "M7.15 7.5a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M17.4 5.5a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M11.4 11.25a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M7.4 16.25a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M16.65 14.25a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
])

export const CheckIcon: Icon = createIcon("check", [
  ["path", { d: "m4.5 12.75 3.23 3.23a2.5 2.5 0 0 0 3.54 0l8.48-8.48" }],
])

export const CheckCheckIcon: Icon = createIcon("check-check", [
  ["path", { d: "m2.5 12.5 3.09 3.09a2 2 0 0 0 2.82 0L17.5 6.5" }],
  ["path", { d: "m13 15.5.44.44a1.5 1.5 0 0 0 2.12 0L21.5 10" }],
])

export const ChevronDownIcon: Icon = createIcon("chevron-down", [
  ["path", { d: "m6 9.25 4.23 4.23a2.5 2.5 0 0 0 3.54 0L18 9.25" }],
])

export const ChevronLeftIcon: Icon = createIcon("chevron-left", [
  ["path", { d: "m14.75 6-4.23 4.23a2.5 2.5 0 0 0 0 3.54L14.75 18" }],
])

export const ChevronRightIcon: Icon = createIcon("chevron-right", [
  ["path", { d: "m9.25 18 4.23-4.23a2.5 2.5 0 0 0 0-3.54L9.25 6" }],
])

export const ChevronUpIcon: Icon = createIcon("chevron-up", [
  ["path", { d: "m18 14.75-4.23-4.23a2.5 2.5 0 0 0-3.54 0L6 14.75" }],
])

export const ChevronsDownUpIcon: Icon = createIcon("chevrons-down-up", [
  ["path", { d: "m7.5 4.5 3.09 3.09a2 2 0 0 0 2.82 0L16.5 4.5" }],
  ["path", { d: "m7.5 19.5 3.09-3.09a2 2 0 0 1 2.82 0l3.09 3.09" }],
])

export const ChevronsLeftIcon: Icon = createIcon("chevrons-left", [
  ["path", { d: "m10.83 7-3.59 3.59a2 2 0 0 0 0 2.82L10.83 17" }],
  ["path", { d: "m17.33 7-3.59 3.59a2 2 0 0 0 0 2.82L17.33 17" }],
])

export const ChevronsRightIcon: Icon = createIcon("chevrons-right", [
  ["path", { d: "m13.17 7 3.59 3.59a2 2 0 0 1 0 2.82L13.17 17" }],
  ["path", { d: "m6.67 7 3.59 3.59a2 2 0 0 1 0 2.82L6.67 17" }],
])

export const ChevronsUpDownIcon: Icon = createIcon("chevrons-up-down", [
  ["path", { d: "m7.5 9 3.09-3.09a2 2 0 0 1 2.82 0L16.5 9" }],
  ["path", { d: "m7.5 15 3.09 3.09a2 2 0 0 0 2.82 0L16.5 15" }],
])

export const CircleIcon: Icon = createIcon("circle", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
])

export const CircleAlertIcon: Icon = createIcon("circle-alert", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M12 7.75v5" }],
  ["path", { d: "M12 16.25h.01" }],
])

export const CircleCheckIcon: Icon = createIcon("circle-check", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "m8 12.25 1.51 1.51a1.75 1.75 0 0 0 2.48 0l4.26-4.26" }],
])

export const CircleDashedIcon: Icon = createIcon("circle-dashed", [
  ["path", { d: "M20.85 13.64a9 9 0 0 1-1.43 3.46" }],
  ["path", { d: "M17.1 19.42a9 9 0 0 1-3.46 1.43" }],
  ["path", { d: "M10.36 20.85a9 9 0 0 1-3.46-1.43" }],
  ["path", { d: "M4.58 17.1a9 9 0 0 1-1.43-3.46" }],
  ["path", { d: "M3.15 10.36A9 9 0 0 1 4.58 6.9" }],
  ["path", { d: "M6.9 4.58a9 9 0 0 1 3.46-1.43" }],
  ["path", { d: "M13.64 3.15a9 9 0 0 1 3.46 1.43" }],
  ["path", { d: "M19.42 6.9a9 9 0 0 1 1.43 3.46" }],
])

export const CircleDotIcon: Icon = createIcon("circle-dot", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M10.75 12a1.25 1.25 0 1 0 2.5 0 1.25 1.25 0 1 0-2.5 0" }],
])

export const CircleHelpIcon: Icon = createIcon("circle-help", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M9.34 9.04a2.75 2.75 0 1 1 4.04 3.09L12 13.5" }],
  ["path", { d: "M12 16.75h.01" }],
])

export const CircleMinusIcon: Icon = createIcon("circle-minus", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M8 12h8" }],
])

export const CirclePauseIcon: Icon = createIcon("circle-pause", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M10 9v6" }],
  ["path", { d: "M14 9v6" }],
])

export const CirclePlusIcon: Icon = createIcon("circle-plus", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M8 12h8" }],
  ["path", { d: "M12 8v8" }],
])

export const CircleStopIcon: Icon = createIcon("circle-stop", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  [
    "path",
    { d: "M10.75 8.75h2.5a2 2 0 0 1 2 2v2.5a2 2 0 0 1-2 2h-2.5a2 2 0 0 1-2-2v-2.5a2 2 0 0 1 2-2" },
  ],
])

export const CircleXIcon: Icon = createIcon("circle-x", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "m9 9 6 6" }],
  ["path", { d: "m15 9-6 6" }],
])

export const CitationIcon: Icon = createIcon("citation", [
  ["path", { d: "M3.5 7h9" }],
  ["path", { d: "M3.5 12h17" }],
  ["path", { d: "M3.5 17H15" }],
  [
    "path",
    {
      d: "M17.5 4.5h2A1.5 1.5 0 0 1 21 6v2a1.5 1.5 0 0 1-1.5 1.5h-2A1.5 1.5 0 0 1 16 8V6a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
])

export const ClipboardIcon: Icon = createIcon("clipboard", [
  [
    "path",
    {
      d: "M6.5 4.75a2 2 0 0 0-2 2V19A2.5 2.5 0 0 0 7 21.5h10a2.5 2.5 0 0 0 2.5-2.5V6.75a2 2 0 0 0-2-2",
    },
  ],
  [
    "path",
    {
      d: "M10.75 2.75h2.5a1.5 1.5 0 0 1 1.5 1.5v1a1.5 1.5 0 0 1-1.5 1.5h-2.5a1.5 1.5 0 0 1-1.5-1.5v-1a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
])

export const ClipboardCheckIcon: Icon = createIcon("clipboard-check", [
  [
    "path",
    {
      d: "M6.5 4.75a2 2 0 0 0-2 2V19A2.5 2.5 0 0 0 7 21.5h10a2.5 2.5 0 0 0 2.5-2.5V6.75a2 2 0 0 0-2-2",
    },
  ],
  [
    "path",
    {
      d: "M10.75 2.75h2.5a1.5 1.5 0 0 1 1.5 1.5v1a1.5 1.5 0 0 1-1.5 1.5h-2.5a1.5 1.5 0 0 1-1.5-1.5v-1a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
  ["path", { d: "m8.5 14 .69.69a1.5 1.5 0 0 0 2.12 0l4.19-4.19" }],
])

export const ClockIcon: Icon = createIcon("clock", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M12 6.75v4.26a1.75 1.75 0 0 0 .85 1.5l2.9 1.74" }],
])

export const CloudIcon: Icon = createIcon("cloud", [
  ["path", { d: "M7 19a4.5 4.5 0 0 1-.23-8.99 5.75 5.75 0 0 1 11.48.56A4.25 4.25 0 0 1 17.5 19z" }],
])

export const CloudLightningIcon: Icon = createIcon("cloud-lightning", [
  [
    "path",
    {
      d: "m8.31 13.25-.56.01a3.83 3.83 0 0 1-.2-7.65 4.89 4.89 0 0 1 9.77.38 3.61 3.61 0 0 1-.64 7.16l-1.43.02",
    },
  ],
  [
    "path",
    { d: "m13.88 10.41-2.92 3.89a.83.83 0 0 0 .78 1.31l1.52-.22a.83.83 0 0 1 .78 1.31l-2.91 3.89" },
  ],
])

export const CloudRainIcon: Icon = createIcon("cloud-rain", [
  [
    "path",
    { d: "M7.75 13.68a3.83 3.83 0 0 1-.2-7.65 4.89 4.89 0 0 1 9.76.48 3.61 3.61 0 0 1-.63 7.17z" },
  ],
  ["path", { d: "m8.25 17-1 3.75" }],
  ["path", { d: "m12.25 17-1 3.75" }],
  ["path", { d: "m16.25 17-1 3.75" }],
])

export const CloudSunIcon: Icon = createIcon("cloud-sun", [
  [
    "path",
    { d: "M10.25 20.5a3.75 3.75 0 0 1-.22-7.49 4.75 4.75 0 0 1 9.47.57 3.5 3.5 0 0 1-.75 6.92z" },
  ],
  ["path", { d: "M6.31 11.23a3.5 3.5 0 0 1 4.37-5.47" }],
  ["path", { d: "m4.43 12.57-1.06 1.06" }],
  ["path", { d: "M2.75 8.5h-1.5" }],
  ["path", { d: "M4.43 4.43 3.37 3.37" }],
  ["path", { d: "M8.5 2.75v-1.5" }],
  ["path", { d: "m12.57 4.43 1.06-1.06" }],
])

export const CloudUploadIcon: Icon = createIcon("cloud-upload", [
  [
    "path",
    { d: "M9.25 17H7a4 4 0 1 1 .26-7.99 5.25 5.25 0 1 1 10.48.5A3.75 3.75 0 0 1 17.5 17h-2.75" },
  ],
  ["path", { d: "M12 14.5v6.75" }],
  ["path", { d: "m9 13.5 1.94-1.94a1.5 1.5 0 0 1 2.12 0L15 13.5" }],
])

export const CodeIcon: Icon = createIcon("code", [
  ["path", { d: "m8.5 6.5-4.09 4.09a2 2 0 0 0 0 2.82L8.5 17.5" }],
  ["path", { d: "m15.5 6.5 4.09 4.09a2 2 0 0 1 0 2.82L15.5 17.5" }],
])

export const CodeXmlIcon: Icon = createIcon("code-xml", [
  ["path", { d: "m8.5 6.5-4.09 4.09a2 2 0 0 0 0 2.82L8.5 17.5" }],
  ["path", { d: "m15.5 6.5 4.09 4.09a2 2 0 0 1 0 2.82L15.5 17.5" }],
  ["path", { d: "m14.5 4-5 16" }],
])

export const CoinsIcon: Icon = createIcon("coins", [
  ["path", { d: "M15 9A6 6 0 1 1 3 9a6 6 0 1 1 12 0" }],
  ["path", { d: "M17.95 10.02a6 6 0 0 1 3.55 5.48 6 6 0 0 1-11.48 2.45" }],
])

export const Columns2Icon: Icon = createIcon("columns-2", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "M12 6.25v11.5" }],
])

export const Columns3Icon: Icon = createIcon("columns-3", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "M9 6.25v11.5" }],
  ["path", { d: "M15 6.25v11.5" }],
])

export const CommandIcon: Icon = createIcon("command", [
  ["path", { d: "M17.75 9H18a3 3 0 1 0-3-3v6.25" }],
  ["path", { d: "M15 17.75V18a3 3 0 1 0 3-3h-6.25" }],
  ["path", { d: "M6.25 15H6a3 3 0 1 0 3 3v-6.25" }],
  ["path", { d: "M9 6.25V6a3 3 0 1 0-3 3h6.25" }],
])

export const CompassIcon: Icon = createIcon("compass", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  [
    "path",
    {
      d: "m15.03 10.31-1.38 2.87a1 1 0 0 1-.47.47l-2.87 1.38a1 1 0 0 1-1.34-1.34l1.38-2.87a1 1 0 0 1 .47-.47l2.87-1.38a1 1 0 0 1 1.34 1.34",
    },
  ],
])

export const ComponentIcon: Icon = createIcon("component", [
  [
    "path",
    {
      d: "m12.88 3.13 1.74 1.74a1.25 1.25 0 0 1 0 1.76l-1.74 1.74a1.25 1.25 0 0 1-1.76 0L9.38 6.63a1.25 1.25 0 0 1 0-1.76l1.74-1.74a1.25 1.25 0 0 1 1.76 0",
    },
  ],
  [
    "path",
    {
      d: "m19.13 9.38 1.74 1.74a1.25 1.25 0 0 1 0 1.76l-1.74 1.74a1.25 1.25 0 0 1-1.76 0l-1.74-1.74a1.25 1.25 0 0 1 0-1.76l1.74-1.74a1.25 1.25 0 0 1 1.76 0",
    },
  ],
  [
    "path",
    {
      d: "m12.88 15.63 1.74 1.74a1.25 1.25 0 0 1 0 1.76l-1.74 1.74a1.25 1.25 0 0 1-1.76 0l-1.74-1.74a1.25 1.25 0 0 1 0-1.76l1.74-1.74a1.25 1.25 0 0 1 1.76 0",
    },
  ],
  [
    "path",
    {
      d: "m6.63 9.38 1.74 1.74a1.25 1.25 0 0 1 0 1.76l-1.74 1.74a1.25 1.25 0 0 1-1.76 0l-1.74-1.74a1.25 1.25 0 0 1 0-1.76l1.74-1.74a1.25 1.25 0 0 1 1.76 0",
    },
  ],
])

export const ComputerUseIcon: Icon = createIcon("computer-use", [
  [
    "path",
    { d: "M5.75 3.5h12.5a3 3 0 0 1 3 3v6.75a3 3 0 0 1-3 3H5.75a3 3 0 0 1-3-3V6.5a3 3 0 0 1 3-3" },
  ],
  ["path", { d: "M8 20.25h8" }],
  [
    "path",
    {
      d: "m10.1 7.35 4.5 1.8a.75.75 0 0 1 0 1.4l-1.6.65a.75.75 0 0 0-.4.4l-.65 1.6a.75.75 0 0 1-1.4 0l-1.8-4.5a.75.75 0 0 1 1.35-1.35",
    },
  ],
])

export const ContextWindowIcon: Icon = createIcon("context-window", [
  ["path", { d: "M7 4.5H6a3 3 0 0 0-3 3v9a3 3 0 0 0 3 3h1" }],
  ["path", { d: "M17 4.5h1a3 3 0 0 1 3 3v9a3 3 0 0 1-3 3h-1" }],
  ["path", { d: "M8 9h8" }],
  ["path", { d: "M8 12h8" }],
  ["path", { d: "M8 15h4.5" }],
])

export const CopyIcon: Icon = createIcon("copy", [
  [
    "path",
    {
      d: "M11.5 9h7a2.5 2.5 0 0 1 2.5 2.5v7a2.5 2.5 0 0 1-2.5 2.5h-7A2.5 2.5 0 0 1 9 18.5v-7A2.5 2.5 0 0 1 11.5 9",
    },
  ],
  ["path", { d: "M6 15h-.5A2.5 2.5 0 0 1 3 12.5v-7A2.5 2.5 0 0 1 5.5 3h7A2.5 2.5 0 0 1 15 5.5V6" }],
])

export const CopyPlusIcon: Icon = createIcon("copy-plus", [
  [
    "path",
    {
      d: "M11.5 9h7a2.5 2.5 0 0 1 2.5 2.5v7a2.5 2.5 0 0 1-2.5 2.5h-7A2.5 2.5 0 0 1 9 18.5v-7A2.5 2.5 0 0 1 11.5 9",
    },
  ],
  ["path", { d: "M6 15h-.5A2.5 2.5 0 0 1 3 12.5v-7A2.5 2.5 0 0 1 5.5 3h7A2.5 2.5 0 0 1 15 5.5V6" }],
  ["path", { d: "M12.25 15h5.5" }],
  ["path", { d: "M15 12.25v5.5" }],
])

export const CornerDownLeftIcon: Icon = createIcon("corner-down-left", [
  ["path", { d: "M20 4v7a4 4 0 0 1-4 4H8.75" }],
  ["path", { d: "m9 10-3.59 3.59a2 2 0 0 0 0 2.82L9 20" }],
])

export const CpuIcon: Icon = createIcon("cpu", [
  ["path", { d: "M8 5h8a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3V8a3 3 0 0 1 3-3" }],
  [
    "path",
    {
      d: "M10.75 9.25h2.5a1.5 1.5 0 0 1 1.5 1.5v2.5a1.5 1.5 0 0 1-1.5 1.5h-2.5a1.5 1.5 0 0 1-1.5-1.5v-2.5a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
  ["path", { d: "M9.5 2.5V5" }],
  ["path", { d: "M9.5 19v2.5" }],
  ["path", { d: "M2.5 9.5H5" }],
  ["path", { d: "M19 9.5h2.5" }],
  ["path", { d: "M14.5 2.5V5" }],
  ["path", { d: "M14.5 19v2.5" }],
  ["path", { d: "M2.5 14.5H5" }],
  ["path", { d: "M19 14.5h2.5" }],
])

export const CreditCardIcon: Icon = createIcon("credit-card", [
  ["path", { d: "M5.75 5h12.5a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H5.75a3 3 0 0 1-3-3V8a3 3 0 0 1 3-3" }],
  ["path", { d: "M5.75 10h12.5" }],
  ["path", { d: "M6.5 15h3" }],
])

export const DatabaseIcon: Icon = createIcon("database", [
  ["path", { d: "M3.5 5.5a8.5 3 0 1 0 17 0 8.5 3 0 1 0-17 0" }],
  ["path", { d: "M3.5 5.5v13a8.5 3 0 0 0 17 0v-13" }],
  ["path", { d: "M3.5 12a8.5 3 0 0 0 17 0" }],
])

export const Dice5Icon: Icon = createIcon("dice-5", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "M8 8h.01" }],
  ["path", { d: "M16 8h.01" }],
  ["path", { d: "M12 12h.01" }],
  ["path", { d: "M8 16h.01" }],
  ["path", { d: "M16 16h.01" }],
])

export const DiffIcon: Icon = createIcon("diff", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "M12 7.5v5" }],
  ["path", { d: "M9.5 10h5" }],
  ["path", { d: "M9.5 16h5" }],
])

export const DownloadIcon: Icon = createIcon("download", [
  ["path", { d: "M3.5 15v2.5a3 3 0 0 0 3 3h11a3 3 0 0 0 3-3V15" }],
  ["path", { d: "m7.5 10.5 3.09 3.09a2 2 0 0 0 2.82 0l3.09-3.09" }],
  ["path", { d: "M12 3.5v7" }],
])

export const DropletIcon: Icon = createIcon("droplet", [
  ["path", { d: "M17.55 11.62a6.5 6.5 0 1 1-11.1 0l4.7-7.72a1 1 0 0 1 1.7 0z" }],
])

export const EllipsisIcon: Icon = createIcon("ellipsis", [
  ["path", { d: "M4.9 12a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M11.4 12a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M17.9 12a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
])

export const EraserIcon: Icon = createIcon("eraser", [
  [
    "path",
    {
      d: "m5.6 13.4 7.8-7.8a2 2 0 0 1 2.8 0l3.2 3.2a2 2 0 0 1 0 2.8L12 19H8.4a2 2 0 0 1-1.4-.6L5.6 17a2.5 2.5 0 0 1 0-3.6",
    },
  ],
  ["path", { d: "m9.5 9.5 6 6" }],
  ["path", { d: "M15 19h5" }],
])

export const ExternalLinkIcon: Icon = createIcon("external-link", [
  ["path", { d: "M10.5 6.5h-4a3 3 0 0 0-3 3v8a3 3 0 0 0 3 3h8a3 3 0 0 0 3-3v-4" }],
  ["path", { d: "M14.5 3.5H18A2.5 2.5 0 0 1 20.5 6v3.5" }],
  ["path", { d: "m10.5 13.5 6.46-6.46" }],
])

export const EyeIcon: Icon = createIcon("eye", [
  [
    "path",
    {
      d: "M2.77 11.36a10.19 10.19 0 0 1 18.46 0 1.5 1.5 0 0 1 0 1.28 10.19 10.19 0 0 1-18.46 0 1.5 1.5 0 0 1 0-1.28",
    },
  ],
  ["path", { d: "M9 12a3 3 0 1 0 6 0 3 3 0 1 0-6 0" }],
])

export const EyeOffIcon: Icon = createIcon("eye-off", [
  ["path", { d: "m4 4 16 16" }],
  [
    "path",
    {
      d: "M14.24 18.25a10.19 10.19 0 0 1-11.47-5.61 1.5 1.5 0 0 1 0-1.28 10.19 10.19 0 0 1 1.7-2.54",
    },
  ],
  [
    "path",
    {
      d: "M9.76 5.75a10.19 10.19 0 0 1 11.47 5.61 1.5 1.5 0 0 1 0 1.28 10.19 10.19 0 0 1-1.7 2.54",
    },
  ],
  ["path", { d: "M11.36 14.93a3 3 0 0 1-2.29-2.29" }],
  ["path", { d: "M12.64 9.07a3 3 0 0 1 2.29 2.29" }],
])

export const FileIcon: Icon = createIcon("file", [
  [
    "path",
    {
      d: "M7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
])

export const FileArchiveIcon: Icon = createIcon("file-archive", [
  [
    "path",
    {
      d: "M7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
  ["path", { d: "M10 5.5h.01" }],
  ["path", { d: "M10 8.5h.01" }],
  ["path", { d: "M10 11.5h.01" }],
  [
    "path",
    {
      d: "M9.5 14h1a1.5 1.5 0 0 1 1.5 1.5V17a1.5 1.5 0 0 1-1.5 1.5h-1A1.5 1.5 0 0 1 8 17v-1.5A1.5 1.5 0 0 1 9.5 14",
    },
  ],
])

export const FileCodeIcon: Icon = createIcon("file-code", [
  [
    "path",
    {
      d: "M7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
  ["path", { d: "m10 12-1.5 1.88a1 1 0 0 0 0 1.24L10 17" }],
  ["path", { d: "m14 12 1.5 1.88a1 1 0 0 1 0 1.24L14 17" }],
])

export const FileCogIcon: Icon = createIcon("file-cog", [
  [
    "path",
    {
      d: "M7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
  ["path", { d: "M9.75 14.75a2.25 2.25 0 1 0 4.5 0 2.25 2.25 0 1 0-4.5 0" }],
  ["path", { d: "M14.25 14.75h2" }],
  ["path", { d: "m13.13 16.7 1 1.73" }],
  ["path", { d: "m10.88 16.7-1 1.73" }],
  ["path", { d: "M9.75 14.75h-2" }],
  ["path", { d: "m10.88 12.8-1.01-1.73" }],
  ["path", { d: "m13.13 12.8 1-1.73" }],
])

export const FileDiffIcon: Icon = createIcon("file-diff", [
  [
    "path",
    {
      d: "M7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
  ["path", { d: "M12 10.75v4" }],
  ["path", { d: "M10 12.75h4" }],
  ["path", { d: "M10 17.75h4" }],
])

export const FileHeadphoneIcon: Icon = createIcon("file-headphone", [
  [
    "path",
    {
      d: "M7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
  ["path", { d: "M8.5 16a3.5 3.5 0 0 1 7 0" }],
  ["path", { d: "M8.5 16v2.5" }],
  ["path", { d: "M15.5 16v2.5" }],
])

export const FileImageIcon: Icon = createIcon("file-image", [
  [
    "path",
    {
      d: "M7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
  ["path", { d: "M8.25 11.75a1.25 1.25 0 1 0 2.5 0 1.25 1.25 0 1 0-2.5 0" }],
  ["path", { d: "m7.75 18.5 3-3a1.5 1.5 0 0 1 2.06-.06l3.44 3.06" }],
])

export const FilePenIcon: Icon = createIcon("file-pen", [
  [
    "path",
    {
      d: "M4.5 12.75V5A2.5 2.5 0 0 1 7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H8.46",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
  [
    "path",
    {
      d: "m4.04 18.63.87-2.6a1.5 1.5 0 0 1 .36-.58l2.93-2.94a2.5 2.5 0 0 1 3.54 0 2.5 2.5 0 0 1 0 3.54L8.8 18.98a1.5 1.5 0 0 1-.58.36l-2.6.87a1.25 1.25 0 0 1-1.58-1.58",
    },
  ],
])

export const FilePlus2Icon: Icon = createIcon("file-plus-2", [
  [
    "path",
    {
      d: "M7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
  ["path", { d: "M9 14.75h6" }],
  ["path", { d: "M12 11.75v6" }],
])

export const FileSpreadsheetIcon: Icon = createIcon("file-spreadsheet", [
  [
    "path",
    {
      d: "M7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
  ["path", { d: "M8 13h2" }],
  ["path", { d: "M14 13h2" }],
  ["path", { d: "M8 17h2" }],
  ["path", { d: "M14 17h2" }],
])

export const FileTextIcon: Icon = createIcon("file-text", [
  [
    "path",
    {
      d: "M7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
  ["path", { d: "M8.25 12.5h7.5" }],
  ["path", { d: "M8.25 16.5h7.5" }],
  ["path", { d: "M8.25 8.5H10" }],
])

export const FileVideoCameraIcon: Icon = createIcon("file-video-camera", [
  [
    "path",
    {
      d: "M7 2.5h5.88a1.5 1.5 0 0 1 1.06.44l5.12 5.12a1.5 1.5 0 0 1 .44 1.06V19a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 19V5A2.5 2.5 0 0 1 7 2.5",
    },
  ],
  ["path", { d: "M13.5 3.25v3.5a1.75 1.75 0 0 0 1.75 1.75h3.5" }],
  [
    "path",
    {
      d: "M8.75 13H12a1.5 1.5 0 0 1 1.5 1.5V17a1.5 1.5 0 0 1-1.5 1.5H8.75a1.5 1.5 0 0 1-1.5-1.5v-2.5a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
  ["path", { d: "M16.5 14v3.5" }],
])

export const FilmIcon: Icon = createIcon("film", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "M8 6.25v11.5" }],
  ["path", { d: "M16 6.25v11.5" }],
  ["path", { d: "M5.5 8h.01" }],
  ["path", { d: "M5.5 12h.01" }],
  ["path", { d: "M5.5 16h.01" }],
  ["path", { d: "M18.5 8h.01" }],
  ["path", { d: "M18.5 12h.01" }],
  ["path", { d: "M18.5 16h.01" }],
])

export const FlagIcon: Icon = createIcon("flag", [
  [
    "path",
    { d: "M4.5 21.25V4c3-2.25 5.25-1.5 7.5-.25s4.5 2 7.5 0v10c-3 2-5.25 1.25-7.5 0s-4.5-2-7.5.25" },
  ],
])

export const FolderIcon: Icon = createIcon("folder", [
  [
    "path",
    {
      d: "M4.75 4h3.24a1.5 1.5 0 0 1 1.21.62l1.1 1.51a1.5 1.5 0 0 0 1.21.62h7.24a2.5 2.5 0 0 1 2.5 2.5v8.25a2.5 2.5 0 0 1-2.5 2.5H5.25a2.5 2.5 0 0 1-2.5-2.5V6a2 2 0 0 1 2-2",
    },
  ],
])

export const FolderGit2Icon: Icon = createIcon("folder-git-2", [
  [
    "path",
    {
      d: "M4.75 4h3.24a1.5 1.5 0 0 1 1.21.62l1.1 1.51a1.5 1.5 0 0 0 1.21.62h7.24a2.5 2.5 0 0 1 2.5 2.5v8.25a2.5 2.5 0 0 1-2.5 2.5H5.25a2.5 2.5 0 0 1-2.5-2.5V6a2 2 0 0 1 2-2",
    },
  ],
  ["path", { d: "M9.75 13.25a2.25 2.25 0 1 0 4.5 0 2.25 2.25 0 1 0-4.5 0" }],
  ["path", { d: "M6.5 13.25h3.25" }],
  ["path", { d: "M14.25 13.25h3.25" }],
])

export const FolderInputIcon: Icon = createIcon("folder-input", [
  [
    "path",
    {
      d: "M2.75 10.25V6a2 2 0 0 1 2-2h3.24a1.5 1.5 0 0 1 1.21.62l1.1 1.51a1.5 1.5 0 0 0 1.21.62h7.24a2.5 2.5 0 0 1 2.5 2.5v8.25a2.5 2.5 0 0 1-2.5 2.5H5.25a2.5 2.5 0 0 1-2.5-2.5v-1.25",
    },
  ],
  ["path", { d: "M2.25 13.25H9.5" }],
  ["path", { d: "m11.25 9.75 2.26 2.26a1.75 1.75 0 0 1 0 2.48l-2.26 2.26" }],
])

export const FolderKanbanIcon: Icon = createIcon("folder-kanban", [
  [
    "path",
    {
      d: "M4.75 4h3.24a1.5 1.5 0 0 1 1.21.62l1.1 1.51a1.5 1.5 0 0 0 1.21.62h7.24a2.5 2.5 0 0 1 2.5 2.5v8.25a2.5 2.5 0 0 1-2.5 2.5H5.25a2.5 2.5 0 0 1-2.5-2.5V6a2 2 0 0 1 2-2",
    },
  ],
  ["path", { d: "M8 10.5V15" }],
  ["path", { d: "M12 10.5V13" }],
  ["path", { d: "M16 10.5v6" }],
])

export const FolderOpenIcon: Icon = createIcon("folder-open", [
  [
    "path",
    {
      d: "M3.25 13V6a2 2 0 0 1 2-2h3.03a1.5 1.5 0 0 1 1.17.56l1.1 1.38a1.5 1.5 0 0 0 1.17.56h5.03",
    },
  ],
  [
    "path",
    {
      d: "M8.72 10h10.81a1.5 1.5 0 0 1 1.45 1.9l-1.93 7a1.5 1.5 0 0 1-1.44 1.1H5.57a1.5 1.5 0 0 1-1.37-2.12l3.15-7A1.5 1.5 0 0 1 8.72 10",
    },
  ],
])

export const FrameIcon: Icon = createIcon("frame", [
  ["path", { d: "M7 2.25V14.5" }],
  ["path", { d: "M7 19.5v2.25" }],
  ["path", { d: "M17 2.25V4.5" }],
  ["path", { d: "M17 9.5v12.25" }],
  ["path", { d: "M2.25 7H4.5" }],
  ["path", { d: "M9.5 7h12.25" }],
  ["path", { d: "M2.25 17H14.5" }],
  ["path", { d: "M19.5 17h2.25" }],
])

export const FrownIcon: Icon = createIcon("frown", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M9 9.5h.01" }],
  ["path", { d: "M15 9.5h.01" }],
  ["path", { d: "M8.1 17.5a4.5 4.5 0 0 1 7.8 0" }],
])

export const GaugeIcon: Icon = createIcon("gauge", [
  ["path", { d: "M4.63 18.66a9 9 0 1 1 14.74 0" }],
  ["path", { d: "m12 13.5 3.75-3.75" }],
])

export const GitBranchIcon: Icon = createIcon("git-branch", [
  ["path", { d: "M6 2.75V15" }],
  ["path", { d: "M3 18a3 3 0 1 0 6 0 3 3 0 1 0-6 0" }],
  ["path", { d: "M15 6a3 3 0 1 0 6 0 3 3 0 1 0-6 0" }],
  ["path", { d: "M15 6a9 9 0 0 0-9 9" }],
])

export const GitCommitHorizontalIcon: Icon = createIcon("git-commit-horizontal", [
  ["path", { d: "M8.5 12a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0" }],
  ["path", { d: "M2.5 12h6" }],
  ["path", { d: "M15.5 12h6" }],
])

export const GitForkIcon: Icon = createIcon("git-fork", [
  ["path", { d: "M3.25 5.5a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "M15.25 5.5a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "M9.25 18.5a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "M6 8.25V9.5A2.5 2.5 0 0 0 8.5 12h7A2.5 2.5 0 0 0 18 9.5V8.25" }],
  ["path", { d: "M12 12v3.75" }],
])

export const GitMergeIcon: Icon = createIcon("git-merge", [
  ["path", { d: "M3.25 5.5a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "M15.25 18a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "M6 8.25v13" }],
  ["path", { d: "M6 9a9 9 0 0 0 9 9" }],
])

export const GitPullRequestIcon: Icon = createIcon("git-pull-request", [
  ["path", { d: "M3.25 5.5a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "M15.25 18.5a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "M6 8.25v13" }],
  ["path", { d: "M11.5 5.5H15a3 3 0 0 1 3 3v7.25" }],
])

export const GithubIcon: Icon = createIcon("github", [
  [
    "path",
    {
      d: "M9.75 21.25v-3.5C6.5 17 4.5 14.75 4.5 11c0-1.25.35-2.4 1-3.35L5.25 3.5 9 5.4c1-.25 2-.4 3-.4s2 .15 3 .4l3.75-1.9-.25 4.15c.65.95 1 2.1 1 3.35 0 3.75-2 6-5.25 6.75v3.5",
    },
  ],
  ["path", { d: "M9.75 19.25c-3 1-3.5-1.5-5.5-1.5" }],
])

export const GlobeIcon: Icon = createIcon("globe", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  [
    "path",
    {
      d: "M12.72 3.8a11.9 11.9 0 0 1 0 16.4 1 1 0 0 1-1.44 0 11.9 11.9 0 0 1 0-16.4 1 1 0 0 1 1.44 0",
    },
  ],
  ["path", { d: "M3 12h2.25" }],
  ["path", { d: "M10.75 12h2.5" }],
  ["path", { d: "M18.75 12H21" }],
])

export const GraduationCapIcon: Icon = createIcon("graduation-cap", [
  [
    "path",
    {
      d: "m12.58 4.81 6.82 3.58a1.25 1.25 0 0 1 0 2.22l-6.82 3.58a1.25 1.25 0 0 1-1.16 0L4.6 10.61a1.25 1.25 0 0 1 0-2.22l6.82-3.58a1.25 1.25 0 0 1 1.16 0",
    },
  ],
  ["path", { d: "M21.5 9.5V15" }],
  ["path", { d: "M6.5 14.5V16a3 3 0 0 0 3 3h5a3 3 0 0 0 3-3v-1.5" }],
])

export const GripHorizontalIcon: Icon = createIcon("grip-horizontal", [
  ["path", { d: "M4.9 9a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M4.9 15a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M11.4 9a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M11.4 15a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M17.9 9a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M17.9 15a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
])

export const GripVerticalIcon: Icon = createIcon("grip-vertical", [
  ["path", { d: "M8.4 5.5a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M14.4 5.5a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M8.4 12a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M14.4 12a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M8.4 18.5a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
  ["path", { d: "M14.4 18.5a.6.6 0 1 0 1.2 0 .6.6 0 1 0-1.2 0" }],
])

export const HandIcon: Icon = createIcon("hand", [
  ["path", { d: "M6.25 14.5V8.25A1.75 1.75 0 0 1 8 6.5a1.75 1.75 0 0 1 1.75 1.75v3.25" }],
  ["path", { d: "M9.75 11V6a1.75 1.75 0 0 1 1.75-1.75A1.75 1.75 0 0 1 13.25 6v5" }],
  ["path", { d: "M13.25 11V7A1.75 1.75 0 0 1 15 5.25 1.75 1.75 0 0 1 16.75 7v4.5" }],
  [
    "path",
    {
      d: "M16.75 12v-2a1.75 1.75 0 0 1 1.75-1.75A1.75 1.75 0 0 1 20.25 10v6.75a4.5 4.5 0 0 1-4.5 4.5H10.1a3 3 0 0 1-2.25-1.01l-4.32-4.89a1.25 1.25 0 0 1-.2-1.33l.02-.06a1.25 1.25 0 0 1 1.91-.48l.99.77",
    },
  ],
])

export const HandoffIcon: Icon = createIcon("handoff", [
  ["path", { d: "M3.5 16.5a2.5 2.5 0 1 0 5 0 2.5 2.5 0 1 0-5 0" }],
  ["path", { d: "M15.5 16.5a2.5 2.5 0 1 0 5 0 2.5 2.5 0 1 0-5 0" }],
  ["path", { d: "M6 12.25a6 6 0 0 1 12 0" }],
  ["path", { d: "m15.75 10.25 1.37 1.37a1.25 1.25 0 0 0 1.76 0l1.37-1.37" }],
])

export const HardDriveIcon: Icon = createIcon("hard-drive", [
  [
    "path",
    {
      d: "M8.02 4h7.96a2 2 0 0 1 1.81 1.15l3.02 6.45a2 2 0 0 1 .19.85V17a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3v-4.55a2 2 0 0 1 .19-.85l3.02-6.45A2 2 0 0 1 8.02 4",
    },
  ],
  ["path", { d: "M3.5 12.5h17" }],
  ["path", { d: "M7 16.25h.01" }],
  ["path", { d: "M10.5 16.25h.01" }],
])

export const HashIcon: Icon = createIcon("hash", [
  ["path", { d: "M4 9h16" }],
  ["path", { d: "M4 15h16" }],
  ["path", { d: "m10 3.5-2 17" }],
  ["path", { d: "m16 3.5-2 17" }],
])

export const HeadingIcon: Icon = createIcon("heading", [
  ["path", { d: "M6 4v16" }],
  ["path", { d: "M18 4v16" }],
  ["path", { d: "M9 12h6" }],
])

export const HeadphonesIcon: Icon = createIcon("headphones", [
  ["path", { d: "M3.5 15v-3a8.5 8.5 0 0 1 17 0v3" }],
  ["path", { d: "M5.5 14h.75a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5.5a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2" }],
  [
    "path",
    { d: "M17.75 14h.75a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2h-.75a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2" },
  ],
])

export const HeadsetIcon: Icon = createIcon("headset", [
  ["path", { d: "M3.5 14.5V12a8.5 8.5 0 0 1 17 0v5.5a4 4 0 0 1-4 4h-3" }],
  [
    "path",
    { d: "M5.5 12.5h.75a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5.5a2 2 0 0 1-2-2v-2.5a2 2 0 0 1 2-2" },
  ],
  [
    "path",
    { d: "M17.75 12.5h.75a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2h-.75a2 2 0 0 1-2-2v-2.5a2 2 0 0 1 2-2" },
  ],
])

export const HexagonIcon: Icon = createIcon("hexagon", [
  [
    "path",
    {
      d: "m13 2.68 6.57 3.79a2 2 0 0 1 1 1.73v7.6a2 2 0 0 1-1 1.73L13 21.32a2 2 0 0 1-2 0l-6.57-3.79a2 2 0 0 1-1-1.73V8.2a2 2 0 0 1 1-1.73L11 2.68a2 2 0 0 1 2 0",
    },
  ],
])

export const HistoryIcon: Icon = createIcon("history", [
  ["path", { d: "M5.49 17.46A8.5 8.5 0 1 0 6.54 5.49" }],
  ["path", { d: "m8.21 8.65-2.94-.26a2 2 0 0 1-1.82-2.16l.26-2.94" }],
  ["path", { d: "M12 8v3.14a1.5 1.5 0 0 0 .74 1.29L15 13.75" }],
])

export const HourglassIcon: Icon = createIcon("hourglass", [
  ["path", { d: "M5.5 2.75h13" }],
  ["path", { d: "M5.5 21.25h13" }],
  ["path", { d: "M7.25 5.75v.46a2.5 2.5 0 0 0 .73 1.77l8.04 8.04a2.5 2.5 0 0 1 .73 1.77v.46" }],
  ["path", { d: "M16.75 5.75v.46a2.5 2.5 0 0 1-.73 1.77l-1.9 1.9" }],
  ["path", { d: "m9.88 14.12-1.9 1.9a2.5 2.5 0 0 0-.73 1.77v.46" }],
])

export const HouseIcon: Icon = createIcon("house", [
  [
    "path",
    {
      d: "m3.77 9.15 6.7-5.21a2.5 2.5 0 0 1 3.06 0l6.7 5.21a2 2 0 0 1 .77 1.58v7.77a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 18.5v-7.77a2 2 0 0 1 .77-1.58",
    },
  ],
  ["path", { d: "M9.5 18v-2.25a2 2 0 0 1 2-2h1a2 2 0 0 1 2 2V18" }],
])

export const IdCardIcon: Icon = createIcon("id-card", [
  ["path", { d: "M5.75 5h12.5a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H5.75a3 3 0 0 1-3-3V8a3 3 0 0 1 3-3" }],
  ["path", { d: "M6.5 10.5a2.25 2.25 0 1 0 4.5 0 2.25 2.25 0 1 0-4.5 0" }],
  ["path", { d: "M5.35 16.17a3.75 3.75 0 0 1 6.8 0" }],
  ["path", { d: "M13.75 10h4" }],
  ["path", { d: "M13.75 14h4" }],
])

export const ImageIcon: Icon = createIcon("image", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "M6.75 8.5a1.75 1.75 0 1 0 3.5 0 1.75 1.75 0 1 0-3.5 0" }],
  ["path", { d: "m6.5 18 5.59-5.59a2 2 0 0 1 2.82 0L18 15.5" }],
])

export const ImageOffIcon: Icon = createIcon("image-off", [
  ["path", { d: "m3.5 3.5 17 17" }],
  ["path", { d: "M6.64 3H18a3 3 0 0 1 3 3v11.36" }],
  ["path", { d: "M17.36 21H6a3 3 0 0 1-3-3V6.64" }],
  ["path", { d: "M13.75 8.5a1.75 1.75 0 1 0 3.5 0 1.75 1.75 0 1 0-3.5 0" }],
  ["path", { d: "m6.25 18 1.94-1.94a1.5 1.5 0 0 1 2.12 0L12.25 18" }],
])

export const ImagePlusIcon: Icon = createIcon("image-plus", [
  ["path", { d: "M13 3H6a3 3 0 0 0-3 3v12a3 3 0 0 0 3 3h12a3 3 0 0 0 3-3v-7" }],
  ["path", { d: "M15.25 5.5h6" }],
  ["path", { d: "M18.25 2.5v6" }],
  ["path", { d: "M6.75 8.5a1.75 1.75 0 1 0 3.5 0 1.75 1.75 0 1 0-3.5 0" }],
  ["path", { d: "m6.5 18 4.59-4.59a2 2 0 0 1 2.82 0L17 16.5" }],
])

export const ImagesIcon: Icon = createIcon("images", [
  [
    "path",
    {
      d: "M10 3h8.5A2.5 2.5 0 0 1 21 5.5V14a2.5 2.5 0 0 1-2.5 2.5H10A2.5 2.5 0 0 1 7.5 14V5.5A2.5 2.5 0 0 1 10 3",
    },
  ],
  ["path", { d: "M3 8v8a2.5 2.5 0 0 0 2.5 2.5H16" }],
  ["path", { d: "M10.75 7.5a1.25 1.25 0 1 0 2.5 0 1.25 1.25 0 1 0-2.5 0" }],
  ["path", { d: "m10.5 13.5 3.01-3.01a1.75 1.75 0 0 1 2.48 0L18.5 13" }],
])

export const InboxIcon: Icon = createIcon("inbox", [
  [
    "path",
    {
      d: "M8.02 4h7.96a2 2 0 0 1 1.81 1.15l3.02 6.45a2 2 0 0 1 .19.85V17a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3v-4.55a2 2 0 0 1 .19-.85l3.02-6.45A2 2 0 0 1 8.02 4",
    },
  ],
  [
    "path",
    {
      d: "M3.5 12.5h3.57a1.5 1.5 0 0 1 1.34.83l.68 1.34a1.5 1.5 0 0 0 1.34.83h3.14a1.5 1.5 0 0 0 1.34-.83l.68-1.34a1.5 1.5 0 0 1 1.34-.83h3.57",
    },
  ],
])

export const InfinityIcon: Icon = createIcon("infinity", [
  ["path", { d: "m9.94 9.81-.53-.56a4 4 0 1 0 0 5.5l5.18-5.5a4 4 0 1 1 0 5.5l-.53-.56" }],
])

export const InfoIcon: Icon = createIcon("info", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M12 11.25v5.25" }],
  ["path", { d: "M12 7.75h.01" }],
])

export const KanbanIcon: Icon = createIcon("kanban", [
  [
    "path",
    {
      d: "M4.5 3H6a1.5 1.5 0 0 1 1.5 1.5v11A1.5 1.5 0 0 1 6 17H4.5A1.5 1.5 0 0 1 3 15.5v-11A1.5 1.5 0 0 1 4.5 3",
    },
  ],
  [
    "path",
    {
      d: "M11.25 3h1.5a1.5 1.5 0 0 1 1.5 1.5v15a1.5 1.5 0 0 1-1.5 1.5h-1.5a1.5 1.5 0 0 1-1.5-1.5v-15a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
  [
    "path",
    {
      d: "M18 3h1.5A1.5 1.5 0 0 1 21 4.5V11a1.5 1.5 0 0 1-1.5 1.5H18a1.5 1.5 0 0 1-1.5-1.5V4.5A1.5 1.5 0 0 1 18 3",
    },
  ],
])

export const KeyRoundIcon: Icon = createIcon("key-round", [
  ["path", { d: "M9.25 8.75a6 6 0 1 0 12 0 6 6 0 1 0-12 0" }],
  ["path", { d: "m11 13-7.75 7.75" }],
  ["path", { d: "m5.75 18.25 2 2" }],
  ["path", { d: "M16 7.25a.75.75 0 1 0 1.5 0 .75.75 0 1 0-1.5 0" }],
])

export const KeyboardIcon: Icon = createIcon("keyboard", [
  ["path", { d: "M5.5 5h13a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3h-13a3 3 0 0 1-3-3V8a3 3 0 0 1 3-3" }],
  ["path", { d: "M6.5 9.25h.01" }],
  ["path", { d: "M6.5 12.25h.01" }],
  ["path", { d: "M10.17 9.25h.01" }],
  ["path", { d: "M10.17 12.25h.01" }],
  ["path", { d: "M13.83 9.25h.01" }],
  ["path", { d: "M13.83 12.25h.01" }],
  ["path", { d: "M17.5 9.25h.01" }],
  ["path", { d: "M17.5 12.25h.01" }],
  ["path", { d: "M8 15.5h8" }],
])

export const LandmarkIcon: Icon = createIcon("landmark", [
  [
    "path",
    {
      d: "m12.6 3.07 3.82 2.08a1.25 1.25 0 0 1-.59 2.35H8.17a1.25 1.25 0 0 1-.59-2.35l3.82-2.08a1.25 1.25 0 0 1 1.2 0",
    },
  ],
  ["path", { d: "M6 10.5v6.75" }],
  ["path", { d: "M10 10.5v6.75" }],
  ["path", { d: "M14 10.5v6.75" }],
  ["path", { d: "M18 10.5v6.75" }],
  ["path", { d: "M3.25 20.5h17.5" }],
])

export const LaptopIcon: Icon = createIcon("laptop", [
  [
    "path",
    {
      d: "M7 4.5h10A2.5 2.5 0 0 1 19.5 7v6a2.5 2.5 0 0 1-2.5 2.5H7A2.5 2.5 0 0 1 4.5 13V7A2.5 2.5 0 0 1 7 4.5",
    },
  ],
  ["path", { d: "M2.75 19.25h18.5" }],
])

export const LayersIcon: Icon = createIcon("layers", [
  [
    "path",
    {
      d: "m12.63 3.04 5.65 2.6a1.5 1.5 0 0 1 0 2.72l-5.65 2.6a1.5 1.5 0 0 1-1.26 0l-5.65-2.6a1.5 1.5 0 0 1 0-2.72l5.65-2.6a1.5 1.5 0 0 1 1.26 0",
    },
  ],
  ["path", { d: "m3 12 8.36 3.95a1.5 1.5 0 0 0 1.28 0L21 12" }],
  ["path", { d: "m3 16.75 8.36 3.95a1.5 1.5 0 0 0 1.28 0L21 16.75" }],
])

export const LayoutGridIcon: Icon = createIcon("layout-grid", [
  ["path", { d: "M5.5 3.5h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2h-3a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2" }],
  ["path", { d: "M15.5 3.5h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2h-3a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2" }],
  ["path", { d: "M5.5 13.5h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2h-3a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2" }],
  ["path", { d: "M15.5 13.5h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2h-3a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2" }],
])

export const LayoutListIcon: Icon = createIcon("layout-list", [
  ["path", { d: "M5 3h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2" }],
  ["path", { d: "M5 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2" }],
  ["path", { d: "M13.5 4.5H21" }],
  ["path", { d: "M13.5 8.5h5" }],
  ["path", { d: "M13.5 15.5H21" }],
  ["path", { d: "M13.5 19.5h5" }],
])

export const LayoutTemplateIcon: Icon = createIcon("layout-template", [
  ["path", { d: "M5 3h14a2 2 0 0 1 2 2v3.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2" }],
  ["path", { d: "M5 13.5h5.5a2 2 0 0 1 2 2V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-3.5a2 2 0 0 1 2-2" }],
  [
    "path",
    { d: "M17.5 13.5H19a2 2 0 0 1 2 2V19a2 2 0 0 1-2 2h-1.5a2 2 0 0 1-2-2v-3.5a2 2 0 0 1 2-2" },
  ],
])

export const LibraryIcon: Icon = createIcon("library", [
  ["path", { d: "M4 3.5v17" }],
  ["path", { d: "M8 6.5v14" }],
  ["path", { d: "M12 5v15.5" }],
  ["path", { d: "m15.5 6 4.25 14" }],
])

export const LifeBuoyIcon: Icon = createIcon("life-buoy", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M8 12a4 4 0 1 0 8 0 4 4 0 1 0-8 0" }],
  ["path", { d: "m14.83 14.83 3.53 3.53" }],
  ["path", { d: "m9.17 14.83-3.53 3.53" }],
  ["path", { d: "M9.17 9.17 5.64 5.64" }],
  ["path", { d: "m14.83 9.17 3.53-3.53" }],
])

export const LightbulbIcon: Icon = createIcon("lightbulb", [
  ["path", { d: "m9.25 15.5-1.1-1.57a6.25 6.25 0 1 1 7.7 0l-1.1 1.57" }],
  ["path", { d: "M9.5 18.5h5" }],
  ["path", { d: "M10.5 21.25h3" }],
])

export const LinkIcon: Icon = createIcon("link", [
  ["path", { d: "M13.93 17.44a5.5 5.5 0 1 1-.56-5.77" }],
  ["path", { d: "M10.07 6.56a5.5 5.5 0 1 1 .56 5.77" }],
])

export const Link2Icon: Icon = createIcon("link-2", [
  ["path", { d: "M10 7H7a5 5 0 0 0-5 5 5 5 0 0 0 5 5h3" }],
  ["path", { d: "M14 7h3a5 5 0 0 1 5 5 5 5 0 0 1-5 5h-3" }],
  ["path", { d: "M8 12h8" }],
])

export const ListIcon: Icon = createIcon("list", [
  ["path", { d: "M4 6h.01" }],
  ["path", { d: "M4 12h.01" }],
  ["path", { d: "M4 18h.01" }],
  ["path", { d: "M8.5 6h12" }],
  ["path", { d: "M8.5 12h12" }],
  ["path", { d: "M8.5 18h12" }],
])

export const ListChecksIcon: Icon = createIcon("list-checks", [
  ["path", { d: "m3 6.5 1.04 1.04a1 1 0 0 0 1.42 0L8.5 4.5" }],
  ["path", { d: "m3 17.5 1.04 1.04a1 1 0 0 0 1.42 0L8.5 15.5" }],
  ["path", { d: "M12.5 6h8" }],
  ["path", { d: "M12.5 12h8" }],
  ["path", { d: "M12.5 18h8" }],
])

export const ListOrderedIcon: Icon = createIcon("list-ordered", [
  ["path", { d: "M3.5 5.75 4.33 5A1 1 0 0 1 6 5.75V9.5" }],
  ["path", { d: "M3.32 15.98a2 2 0 1 1 3.13 2.12l-.8.6a1 1 0 0 0 .6 1.8H7.5" }],
  ["path", { d: "M11 6h9.5" }],
  ["path", { d: "M11 12h9.5" }],
  ["path", { d: "M11 18h9.5" }],
])

export const ListTreeIcon: Icon = createIcon("list-tree", [
  ["path", { d: "M4 3.5v6A2.5 2.5 0 0 0 6.5 12h3" }],
  ["path", { d: "M4 12v3.5A2.5 2.5 0 0 0 6.5 18h3" }],
  ["path", { d: "M9 6h11.5" }],
  ["path", { d: "M13.5 12h7" }],
  ["path", { d: "M13.5 18h7" }],
])

export const LoaderCircleIcon: Icon = createIcon("loader-circle", [
  ["path", { d: "M12 3a9 9 0 1 1-9 9" }],
])

export const LocateFixedIcon: Icon = createIcon("locate-fixed", [
  ["path", { d: "M5 12a7 7 0 1 0 14 0 7 7 0 1 0-14 0" }],
  ["path", { d: "M9.5 12a2.5 2.5 0 1 0 5 0 2.5 2.5 0 1 0-5 0" }],
  ["path", { d: "M12 2.25V5" }],
  ["path", { d: "M12 19v2.75" }],
  ["path", { d: "M2.25 12H5" }],
  ["path", { d: "M19 12h2.75" }],
])

export const LockIcon: Icon = createIcon("lock", [
  ["path", { d: "M7 10.5h10a3 3 0 0 1 3 3V18a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3v-4.5a3 3 0 0 1 3-3" }],
  ["path", { d: "M8 7.5V7a4 4 0 0 1 4-4 4 4 0 0 1 4 4v.5" }],
])

export const LockKeyholeIcon: Icon = createIcon("lock-keyhole", [
  ["path", { d: "M7 10.5h10a3 3 0 0 1 3 3V18a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3v-4.5a3 3 0 0 1 3-3" }],
  ["path", { d: "M8 7.5V7a4 4 0 0 1 4-4 4 4 0 0 1 4 4v.5" }],
  ["path", { d: "M11 14.75a1 1 0 1 0 2 0 1 1 0 1 0-2 0" }],
  ["path", { d: "M12 15.75v2" }],
])

export const LockKeyholeOpenIcon: Icon = createIcon("lock-keyhole-open", [
  ["path", { d: "M7 10.5h10a3 3 0 0 1 3 3V18a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3v-4.5a3 3 0 0 1 3-3" }],
  ["path", { d: "M8 7.5V7a4 4 0 0 1 7.46-2" }],
  ["path", { d: "M11 14.75a1 1 0 1 0 2 0 1 1 0 1 0-2 0" }],
  ["path", { d: "M12 15.75v2" }],
])

export const LogInIcon: Icon = createIcon("log-in", [
  ["path", { d: "M14.5 3h3a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3h-3" }],
  ["path", { d: "m10.75 16.5 3.09-3.09a2 2 0 0 0 0-2.82L10.75 7.5" }],
  ["path", { d: "M3.25 12h7.5" }],
])

export const LogOutIcon: Icon = createIcon("log-out", [
  ["path", { d: "M9.5 3h-3a3 3 0 0 0-3 3v12a3 3 0 0 0 3 3h3" }],
  ["path", { d: "m16.5 16.5 3.09-3.09a2 2 0 0 0 0-2.82L16.5 7.5" }],
  ["path", { d: "M9.25 12h7.25" }],
])

export const MailIcon: Icon = createIcon("mail", [
  ["path", { d: "M5.75 5h12.5a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H5.75a3 3 0 0 1-3-3V8a3 3 0 0 1 3-3" }],
  ["path", { d: "m3.75 7.5 6.86 4.58a2.5 2.5 0 0 0 2.78 0l6.86-4.58" }],
])

export const MapIcon: Icon = createIcon("map", [
  [
    "path",
    {
      d: "m3.92 5.12 4.5-1.88a1.5 1.5 0 0 1 1.16 0l4.84 2.02a1.5 1.5 0 0 0 1.16 0l3.34-1.39A1.5 1.5 0 0 1 21 5.25V17.5a1.5 1.5 0 0 1-.92 1.38l-4.5 1.88a1.5 1.5 0 0 1-1.16 0l-4.84-2.02a1.5 1.5 0 0 0-1.16 0l-3.34 1.39A1.5 1.5 0 0 1 3 18.75V6.5a1.5 1.5 0 0 1 .92-1.38",
    },
  ],
  ["path", { d: "M9 6v9.5" }],
  ["path", { d: "M15 8.5V18" }],
])

export const MapPinIcon: Icon = createIcon("map-pin", [
  ["path", { d: "M6.23 14.54a7.5 7.5 0 1 1 11.54 0l-3.85 4.64a2.5 2.5 0 0 1-3.84 0z" }],
  ["path", { d: "M9.25 9.75a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
])

export const MaximizeIcon: Icon = createIcon("maximize", [
  ["path", { d: "M3 8.5V6a3 3 0 0 1 3-3h2.5" }],
  ["path", { d: "M15.5 3H18a3 3 0 0 1 3 3v2.5" }],
  ["path", { d: "M21 15.5V18a3 3 0 0 1-3 3h-2.5" }],
  ["path", { d: "M8.5 21H6a3 3 0 0 1-3-3v-2.5" }],
])

export const Maximize2Icon: Icon = createIcon("maximize-2", [
  ["path", { d: "M14.5 3.5H18A2.5 2.5 0 0 1 20.5 6v3.5" }],
  ["path", { d: "m13.75 10.25 3.21-3.21" }],
  ["path", { d: "M9.5 20.5H6A2.5 2.5 0 0 1 3.5 18v-3.5" }],
  ["path", { d: "m10.25 13.75-3.21 3.21" }],
])

export const MegaphoneIcon: Icon = createIcon("megaphone", [
  [
    "path",
    {
      d: "M5 6.5h4.76a1.5 1.5 0 0 0 .47-.08l8.3-2.76a1.5 1.5 0 0 1 1.97 1.42v9.84a1.5 1.5 0 0 1-1.97 1.42l-8.3-2.76a1.5 1.5 0 0 0-.47-.08H5a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2",
    },
  ],
  ["path", { d: "m5.75 16.5.74 3.33A1.5 1.5 0 0 0 7.95 21h.03a1.5 1.5 0 0 0 1.48-1.75L9 16.5" }],
])

export const MehIcon: Icon = createIcon("meh", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M9 9.5h.01" }],
  ["path", { d: "M15 9.5h.01" }],
  ["path", { d: "M8.75 15.5h6.5" }],
])

export const MemoryIcon: Icon = createIcon("memory", [
  ["path", { d: "M7 3.5h11a3 3 0 0 1 3 3v11" }],
  [
    "path",
    {
      d: "M5.5 7h9A2.5 2.5 0 0 1 17 9.5v9a2.5 2.5 0 0 1-2.5 2.5h-9A2.5 2.5 0 0 1 3 18.5v-9A2.5 2.5 0 0 1 5.5 7",
    },
  ],
  ["path", { d: "M7.25 7.5V14l2.5-1.75 2.5 1.75V7.5" }],
])

export const MenuIcon: Icon = createIcon("menu", [
  ["path", { d: "M4 6h16" }],
  ["path", { d: "M4 12h16" }],
  ["path", { d: "M4 18h16" }],
])

export const MessageCircleIcon: Icon = createIcon("message-circle", [
  ["path", { d: "M4.21 16a9 9 0 1 1 3.29 3.29l-2.6.9a1 1 0 0 1-1.31-1.15z" }],
])

export const MessageSquareIcon: Icon = createIcon("message-square", [
  [
    "path",
    {
      d: "M6 3.5h12a3 3 0 0 1 3 3V14a3 3 0 0 1-3 3H8.53a1.5 1.5 0 0 0-.94.33l-2.56 2.05A1.25 1.25 0 0 1 3 18.4V6.5a3 3 0 0 1 3-3",
    },
  ],
])

export const MessageSquareTextIcon: Icon = createIcon("message-square-text", [
  [
    "path",
    {
      d: "M6 3.5h12a3 3 0 0 1 3 3V14a3 3 0 0 1-3 3H8.53a1.5 1.5 0 0 0-.94.33l-2.56 2.05A1.25 1.25 0 0 1 3 18.4V6.5a3 3 0 0 1 3-3",
    },
  ],
  ["path", { d: "M7.5 8h9" }],
  ["path", { d: "M7.5 12.5H13" }],
])

export const MessagesSquareIcon: Icon = createIcon("messages-square", [
  [
    "path",
    {
      d: "M5.5 3h7A2.5 2.5 0 0 1 15 5.5v4a2.5 2.5 0 0 1-2.5 2.5H8.04a1.5 1.5 0 0 0-.96.35l-2.03 1.69A1.25 1.25 0 0 1 3 13.08V5.5A2.5 2.5 0 0 1 5.5 3",
    },
  ],
  [
    "path",
    {
      d: "M18 7.5h.5A2.5 2.5 0 0 1 21 10v7.86a1.25 1.25 0 0 1-2.1.92l-1.47-1.37a1.5 1.5 0 0 0-1.02-.41H11a2 2 0 0 1-2-2",
    },
  ],
])

export const MicIcon: Icon = createIcon("mic", [
  ["path", { d: "M9 5.5a3 3 0 0 1 3-3 3 3 0 0 1 3 3V10a3 3 0 0 1-3 3 3 3 0 0 1-3-3z" }],
  ["path", { d: "M18.5 10a6.5 6.5 0 0 1-13 0" }],
  ["path", { d: "M12 16.5V21" }],
])

export const MicOffIcon: Icon = createIcon("mic-off", [
  ["path", { d: "m4 4 16 16" }],
  ["path", { d: "M9 5.75V5.5a3 3 0 0 1 3-3 3 3 0 0 1 3 3V10a3 3 0 0 1-.06.58" }],
  ["path", { d: "m9.87 12.11-.07-.07" }],
  ["path", { d: "M18.5 10a6.5 6.5 0 0 1-.9 3.3" }],
  ["path", { d: "M12.6 16.47A6.5 6.5 0 0 1 5.5 10" }],
  ["path", { d: "M12 16.5V21" }],
])

export const MinimizeIcon: Icon = createIcon("minimize", [
  ["path", { d: "M3.5 8.5H6A2.5 2.5 0 0 0 8.5 6V3.5" }],
  ["path", { d: "M15.5 3.5V6A2.5 2.5 0 0 0 18 8.5h2.5" }],
  ["path", { d: "M20.5 15.5H18a2.5 2.5 0 0 0-2.5 2.5v2.5" }],
  ["path", { d: "M8.5 20.5V18A2.5 2.5 0 0 0 6 15.5H3.5" }],
])

export const Minimize2Icon: Icon = createIcon("minimize-2", [
  ["path", { d: "M14 4v3.5a2.5 2.5 0 0 0 2.5 2.5H20" }],
  ["path", { d: "m20.25 3.75-2.71 2.71" }],
  ["path", { d: "M10 20v-3.5A2.5 2.5 0 0 0 7.5 14H4" }],
  ["path", { d: "m3.75 20.25 2.71-2.71" }],
])

export const MinusIcon: Icon = createIcon("minus", [["path", { d: "M5 12h14" }]])

export const ModelIcon: Icon = createIcon("model", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  [
    "path",
    {
      d: "m12.88 7.88 3.24 3.24a1.25 1.25 0 0 1 0 1.76l-3.24 3.24a1.25 1.25 0 0 1-1.76 0l-3.24-3.24a1.25 1.25 0 0 1 0-1.76l3.24-3.24a1.25 1.25 0 0 1 1.76 0",
    },
  ],
])

export const MonitorIcon: Icon = createIcon("monitor", [
  [
    "path",
    { d: "M5.75 3.5h12.5a3 3 0 0 1 3 3v6.75a3 3 0 0 1-3 3H5.75a3 3 0 0 1-3-3V6.5a3 3 0 0 1 3-3" },
  ],
  ["path", { d: "M8 20.25h8" }],
])

export const MoonIcon: Icon = createIcon("moon", [
  ["path", { d: "M11.47 3.27a8.75 8.75 0 1 0 9.26 9.26 6.75 6.75 0 0 1-9.26-9.26" }],
])

export const MountainIcon: Icon = createIcon("mountain", [
  [
    "path",
    {
      d: "M3.32 17.94 7.1 8.5a1.75 1.75 0 0 1 3.17-.17l1.38 2.61a1.5 1.5 0 0 0 2.52.2l.11-.14a1.75 1.75 0 0 1 2.96.29l3.2 6.55A1.5 1.5 0 0 1 19.1 20H4.72a1.5 1.5 0 0 1-1.4-2.06",
    },
  ],
])

export const MousePointerIcon: Icon = createIcon("mouse-pointer", [
  [
    "path",
    {
      d: "m6.19 4.57 9.27 3.67a1.25 1.25 0 0 1 .12 2.27l-2.98 1.56a1.25 1.25 0 0 0-.53.53l-1.56 2.98a1.25 1.25 0 0 1-2.27-.12L4.57 6.19a1.25 1.25 0 0 1 1.62-1.62",
    },
  ],
  ["path", { d: "m15.5 15.5 4.75 4.75" }],
])

export const MousePointer2Icon: Icon = createIcon("mouse-pointer-2", [
  [
    "path",
    {
      d: "m7.23 5.6 9.96 4.02a1.25 1.25 0 0 1 .02 2.31l-3.24 1.37a1.25 1.25 0 0 0-.67.67l-1.37 3.24a1.25 1.25 0 0 1-2.31-.02L5.6 7.23A1.25 1.25 0 0 1 7.23 5.6",
    },
  ],
])

export const MoveIcon: Icon = createIcon("move", [
  ["path", { d: "m15 5.5-1.94-1.94a1.5 1.5 0 0 0-2.12 0L9 5.5" }],
  ["path", { d: "m9 18.5 1.94 1.94a1.5 1.5 0 0 0 2.12 0L15 18.5" }],
  ["path", { d: "m5.5 9-1.94 1.94a1.5 1.5 0 0 0 0 2.12L5.5 15" }],
  ["path", { d: "m18.5 15 1.94-1.94a1.5 1.5 0 0 0 0-2.12L18.5 9" }],
  ["path", { d: "M12 6.75v10.5" }],
  ["path", { d: "M6.75 12H9" }],
  ["path", { d: "M15 12h2.25" }],
])

export const MusicIcon: Icon = createIcon("music", [
  ["path", { d: "M3 18a3 3 0 1 0 6 0 3 3 0 1 0-6 0" }],
  ["path", { d: "M15 16a3 3 0 1 0 6 0 3 3 0 1 0-6 0" }],
  ["path", { d: "M9 18V6.27a1.5 1.5 0 0 1 1.25-1.48l9-1.5A1.5 1.5 0 0 1 21 4.77V16" }],
])

export const NavigationIcon: Icon = createIcon("navigation", [
  [
    "path",
    {
      d: "m6.88 9.31 10.76-4.59a1.25 1.25 0 0 1 1.64 1.64l-4.59 10.76a1.25 1.25 0 0 1-2.36-.18l-.9-3.48a1.25 1.25 0 0 0-.89-.89l-3.48-.9a1.25 1.25 0 0 1-.18-2.36",
    },
  ],
])

export const NetworkIcon: Icon = createIcon("network", [
  [
    "path",
    {
      d: "M10.75 3h2.5A1.75 1.75 0 0 1 15 4.75v2a1.75 1.75 0 0 1-1.75 1.75h-2.5A1.75 1.75 0 0 1 9 6.75v-2A1.75 1.75 0 0 1 10.75 3",
    },
  ],
  [
    "path",
    {
      d: "M4.25 15.5h2.5a1.75 1.75 0 0 1 1.75 1.75v2.5a1.75 1.75 0 0 1-1.75 1.75h-2.5a1.75 1.75 0 0 1-1.75-1.75v-2.5a1.75 1.75 0 0 1 1.75-1.75",
    },
  ],
  [
    "path",
    {
      d: "M17.25 15.5h2.5a1.75 1.75 0 0 1 1.75 1.75v2.5a1.75 1.75 0 0 1-1.75 1.75h-2.5a1.75 1.75 0 0 1-1.75-1.75v-2.5a1.75 1.75 0 0 1 1.75-1.75",
    },
  ],
  ["path", { d: "M5.5 12.75a1.5 1.5 0 0 1 1.5-1.5h10a1.5 1.5 0 0 1 1.5 1.5" }],
])

export const NewspaperIcon: Icon = createIcon("newspaper", [
  [
    "path",
    {
      d: "M9 3h9.5A2.5 2.5 0 0 1 21 5.5v13a2.5 2.5 0 0 1-2.5 2.5H9a2.5 2.5 0 0 1-2.5-2.5v-13A2.5 2.5 0 0 1 9 3",
    },
  ],
  ["path", { d: "M6.5 9H5a2 2 0 0 0-2 2v7.5A2.5 2.5 0 0 0 5.5 21H7" }],
  [
    "path",
    {
      d: "M11.25 6.5h5a1.25 1.25 0 0 1 1.25 1.25v1.5a1.25 1.25 0 0 1-1.25 1.25h-5A1.25 1.25 0 0 1 10 9.25v-1.5a1.25 1.25 0 0 1 1.25-1.25",
    },
  ],
  ["path", { d: "M10 14h7.5" }],
  ["path", { d: "M10 17.5h5" }],
])

export const OctagonAlertIcon: Icon = createIcon("octagon-alert", [
  [
    "path",
    {
      d: "m20.56 16.37-4.19 4.19a2 2 0 0 1-1.41.59H9.04a2 2 0 0 1-1.41-.59l-4.19-4.19a2 2 0 0 1-.59-1.41V9.04a2 2 0 0 1 .59-1.41l4.19-4.19a2 2 0 0 1 1.41-.59h5.92a2 2 0 0 1 1.41.59l4.19 4.19a2 2 0 0 1 .59 1.41v5.92a2 2 0 0 1-.59 1.41",
    },
  ],
  ["path", { d: "M12 7.75v5" }],
  ["path", { d: "M12 16.25h.01" }],
])

export const PackageIcon: Icon = createIcon("package", [
  [
    "path",
    {
      d: "m13 2.68 6.57 3.79a2 2 0 0 1 1 1.73v7.6a2 2 0 0 1-1 1.73L13 21.32a2 2 0 0 1-2 0l-6.57-3.79a2 2 0 0 1-1-1.73V8.2a2 2 0 0 1 1-1.73L11 2.68a2 2 0 0 1 2 0",
    },
  ],
  ["path", { d: "M6.25 8.7 11 11.43a2 2 0 0 0 2 0l4.75-2.73" }],
  ["path", { d: "M12 11.7v6.9" }],
  ["path", { d: "m7.75 4.6 8.55 4.9" }],
])

export const PaletteIcon: Icon = createIcon("palette", [
  [
    "path",
    {
      d: "M11.22 20.97a9 9 0 1 1 9.58-7.1l-3.5.33a2 2 0 0 0-.99.37l-1.88 1.37a1.5 1.5 0 0 0-.57.83L12.75 21",
    },
  ],
  ["path", { d: "M7 11a.75.75 0 1 0 1.5 0A.75.75 0 1 0 7 11" }],
  ["path", { d: "M9.25 7a.75.75 0 1 0 1.5 0 .75.75 0 1 0-1.5 0" }],
  ["path", { d: "M13.75 7a.75.75 0 1 0 1.5 0 .75.75 0 1 0-1.5 0" }],
  ["path", { d: "M16 10.5a.75.75 0 1 0 1.5 0 .75.75 0 1 0-1.5 0" }],
])

export const PanelLeftIcon: Icon = createIcon("panel-left", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "M9 6.25v11.5" }],
])

export const PanelRightIcon: Icon = createIcon("panel-right", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "M15 6.25v11.5" }],
])

export const PaperclipIcon: Icon = createIcon("paperclip", [
  [
    "path",
    {
      d: "m14.81 7.92-6.36 6.36a1.75 1.75 0 0 0 0 2.47 1.75 1.75 0 0 0 2.47 0l7.07-7.07a3.5 3.5 0 0 0 0-4.95 3.5 3.5 0 0 0-4.95 0L5.97 11.8a5.25 5.25 0 0 0 0 7.43 5.25 5.25 0 0 0 7.43 0l4.95-4.95",
    },
  ],
])

export const PauseIcon: Icon = createIcon("pause", [
  ["path", { d: "M8.5 5v14" }],
  ["path", { d: "M15.5 5v14" }],
])

export const PenLineIcon: Icon = createIcon("pen-line", [
  [
    "path",
    {
      d: "m3.79 18.63.87-2.6a1.5 1.5 0 0 1 .36-.58l7.88-7.89a2.5 2.5 0 0 1 3.54 0 2.5 2.5 0 0 1 0 3.54l-7.89 7.88a1.5 1.5 0 0 1-.58.36l-2.6.87a1.25 1.25 0 0 1-1.58-1.58",
    },
  ],
  ["path", { d: "m11.49 8.98 3.53 3.53" }],
  ["path", { d: "M13.5 20.75h7" }],
])

export const PenToolIcon: Icon = createIcon("pen-tool", [
  [
    "path",
    {
      d: "m5.15 3.88 7.52 2.51a2.5 2.5 0 0 1 1.58 1.58l.83 2.47a1 1 0 0 1-.25 1.03l-3.36 3.36a1 1 0 0 1-1.03.25l-2.47-.83a2.5 2.5 0 0 1-1.58-1.58L3.88 5.15a1 1 0 0 1 1.27-1.27",
    },
  ],
  ["path", { d: "m5.19 5.19 3.19 3.19" }],
  ["path", { d: "M7.94 9.44a1.5 1.5 0 1 0 3 0 1.5 1.5 0 1 0-3 0" }],
  [
    "path",
    {
      d: "m18.81 13.5-.18-.17a1.5 1.5 0 0 1 0 2.12l-3.18 3.18a1.5 1.5 0 0 1-2.12 0l.17.18a1.5 1.5 0 0 1 0-2.12l3.19-3.19a1.5 1.5 0 0 1 2.12 0",
    },
  ],
])

export const PencilIcon: Icon = createIcon("pencil", [
  [
    "path",
    {
      d: "m3.79 18.63.87-2.6a1.5 1.5 0 0 1 .36-.58l9.65-9.65a2.5 2.5 0 0 1 3.53 0 2.5 2.5 0 0 1 0 3.53l-9.65 9.65a1.5 1.5 0 0 1-.58.36l-2.6.87a1.25 1.25 0 0 1-1.58-1.58",
    },
  ],
  ["path", { d: "m13.25 7.21 3.54 3.54" }],
])

export const PhoneIcon: Icon = createIcon("phone", [
  [
    "path",
    {
      d: "M19 21A16 16 0 0 1 3 5a2 2 0 0 1 2-2h2.33a1.5 1.5 0 0 1 1.45 1.14L9.5 7a1.5 1.5 0 0 1-.6 1.59L7.25 9.75a12.67 12.67 0 0 0 7 7l1.16-1.65a1.5 1.5 0 0 1 1.59-.6l2.86.72A1.5 1.5 0 0 1 21 16.67V19a2 2 0 0 1-2 2",
    },
  ],
])

export const PhoneCallIcon: Icon = createIcon("phone-call", [
  [
    "path",
    {
      d: "M19 21A16 16 0 0 1 3 5a2 2 0 0 1 2-2h2.33a1.5 1.5 0 0 1 1.45 1.14L9.5 7a1.5 1.5 0 0 1-.6 1.59L7.25 9.75a12.67 12.67 0 0 0 7 7l1.16-1.65a1.5 1.5 0 0 1 1.59-.6l2.86.72A1.5 1.5 0 0 1 21 16.67V19a2 2 0 0 1-2 2",
    },
  ],
  ["path", { d: "M13 6.75A4.25 4.25 0 0 1 17.25 11" }],
  ["path", { d: "M13 2.75A8.25 8.25 0 0 1 21.25 11" }],
])

export const PhoneOffIcon: Icon = createIcon("phone-off", [
  [
    "path",
    {
      d: "M3 13.75v1.5a1.5 1.5 0 0 0 1.5 1.5h2.75a1.5 1.5 0 0 0 1.5-1.5V13.6a11 11 0 0 1 6.5 0v1.65a1.5 1.5 0 0 0 1.5 1.5h2.75a1.5 1.5 0 0 0 1.5-1.5v-1.5a13 13 0 0 0-18 0",
    },
  ],
])

export const PictureInPictureIcon: Icon = createIcon("picture-in-picture", [
  ["path", { d: "M10.5 19.5H6a3 3 0 0 1-3-3v-9a3 3 0 0 1 3-3h12a3 3 0 0 1 3 3v3" }],
  [
    "path",
    {
      d: "M14.5 13.5h5A1.5 1.5 0 0 1 21 15v3a1.5 1.5 0 0 1-1.5 1.5h-5A1.5 1.5 0 0 1 13 18v-3a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
])

export const PinIcon: Icon = createIcon("pin", [
  [
    "path",
    {
      d: "M9 2.75h6a1.5 1.5 0 0 1 1.5 1.5v.5a1 1 0 0 1-1 1h.25a1 1 0 0 0-1 1v2.8a1.5 1.5 0 0 0 .24.82l2 3.06a1.5 1.5 0 0 1-1.26 2.32H8.27a1.5 1.5 0 0 1-1.26-2.32l2-3.06a1.5 1.5 0 0 0 .24-.82v-2.8a1 1 0 0 0-1-1h.25a1 1 0 0 1-1-1v-.5A1.5 1.5 0 0 1 9 2.75",
    },
  ],
  ["path", { d: "M12 15.75v5.5" }],
])

export const PlaneIcon: Icon = createIcon("plane", [
  [
    "path",
    {
      d: "m18.48 7.42-.04.11a1 1 0 0 1-.24.39l-3.23 3.24a.75.75 0 0 0-.19.74l2.18 7.49a1 1 0 0 1-.25.98l-.14.14a1 1 0 0 1-1.54-.15l-3.23-4.78a.75.75 0 0 0-1.15-.11L8.6 17.52a.75.75 0 0 0-.18.74l.8 2.68a.75.75 0 0 1-.19.75l-.33.33a.75.75 0 0 1-1.11-.06l-2.42-2.98a1 1 0 0 0-.15-.15l-2.98-2.42a.75.75 0 0 1-.06-1.11l.33-.33a.75.75 0 0 1 .75-.19l2.68.8a.75.75 0 0 0 .74-.18l2.05-2.05a.75.75 0 0 0-.11-1.15L3.64 8.97a1 1 0 0 1-.15-1.54l.14-.14a1 1 0 0 1 .98-.25l7.49 2.18a.75.75 0 0 0 .74-.19l3.24-3.23a1 1 0 0 1 .39-.24l.11-.04a1.5 1.5 0 0 1 1.9 1.9",
    },
  ],
])

export const PlayIcon: Icon = createIcon("play", [
  [
    "path",
    {
      d: "m9.51 5.75 7.4 4.31a2.25 2.25 0 0 1 0 3.88l-7.4 4.31a2 2 0 0 1-3.01-1.73V7.48a2 2 0 0 1 3.01-1.73",
    },
  ],
])

export const PlugIcon: Icon = createIcon("plug", [
  [
    "path",
    { d: "M7.5 8.5h9A1.5 1.5 0 0 1 18 10v1a6 6 0 0 1-6 6 6 6 0 0 1-6-6v-1a1.5 1.5 0 0 1 1.5-1.5" },
  ],
  ["path", { d: "M9.5 2.75V5.5" }],
  ["path", { d: "M14.5 2.75V5.5" }],
  ["path", { d: "M12 17v4.25" }],
])

export const PlusIcon: Icon = createIcon("plus", [
  ["path", { d: "M5 12h14" }],
  ["path", { d: "M12 5v14" }],
])

export const PowerIcon: Icon = createIcon("power", [
  ["path", { d: "M16.88 6.04a8.5 8.5 0 1 1-9.76 0" }],
  ["path", { d: "M12 2.75V11" }],
])

export const PrinterIcon: Icon = createIcon("printer", [
  [
    "path",
    {
      d: "M5.25 17.5a2.5 2.5 0 0 1-2.5-2.5v-3.5A2.5 2.5 0 0 1 5.25 9h13.5a2.5 2.5 0 0 1 2.5 2.5V15a2.5 2.5 0 0 1-2.5 2.5",
    },
  ],
  ["path", { d: "M6.75 6V4.5A1.5 1.5 0 0 1 8.25 3h7.5a1.5 1.5 0 0 1 1.5 1.5V6" }],
  [
    "path",
    {
      d: "M9.25 13.25h5.5a1.5 1.5 0 0 1 1.5 1.5v5a1.5 1.5 0 0 1-1.5 1.5h-5.5a1.5 1.5 0 0 1-1.5-1.5v-5a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
])

export const PromptIcon: Icon = createIcon("prompt", [
  [
    "path",
    { d: "M5.75 5.5h12.5a3 3 0 0 1 3 3v7a3 3 0 0 1-3 3H5.75a3 3 0 0 1-3-3v-7a3 3 0 0 1 3-3" },
  ],
  ["path", { d: "M7.25 9.5v5" }],
  ["path", { d: "M10.5 14.5h6.25" }],
])

export const QuoteIcon: Icon = createIcon("quote", [
  [
    "path",
    {
      d: "M5.25 5h3A1.75 1.75 0 0 1 10 6.75v5a5 5 0 0 1-1.1 3.12l-.75.94a1 1 0 0 1-1.74-.91l-.13.45a1.25 1.25 0 0 0-1.2-1.6h.17A1.75 1.75 0 0 1 3.5 12V6.75A1.75 1.75 0 0 1 5.25 5",
    },
  ],
  [
    "path",
    {
      d: "M15.25 5h3A1.75 1.75 0 0 1 20 6.75v5a5 5 0 0 1-1.1 3.12l-.75.94a1 1 0 0 1-1.74-.91l-.13.45a1.25 1.25 0 0 0-1.2-1.6h.17A1.75 1.75 0 0 1 13.5 12V6.75A1.75 1.75 0 0 1 15.25 5",
    },
  ],
])

export const ReasoningIcon: Icon = createIcon("reasoning", [
  ["path", { d: "M4 18a2 2 0 1 0 4 0 2 2 0 1 0-4 0" }],
  ["path", { d: "M9.5 18h4a3 3 0 0 0 0-6h-3a3 3 0 0 1 0-6h4" }],
  [
    "path",
    {
      d: "M17.25 4h1.5A1.25 1.25 0 0 1 20 5.25v1.5A1.25 1.25 0 0 1 18.75 8h-1.5A1.25 1.25 0 0 1 16 6.75v-1.5A1.25 1.25 0 0 1 17.25 4",
    },
  ],
])

export const ReceiptIcon: Icon = createIcon("receipt", [
  [
    "path",
    {
      d: "M7 3h10a2.5 2.5 0 0 1 2.5 2.5v14a1.5 1.5 0 0 1-3 0 1.5 1.5 0 0 0-3 0 1.5 1.5 0 0 1-3 0 1.5 1.5 0 0 0-3 0 1.5 1.5 0 0 1-3 0v-14A2.5 2.5 0 0 1 7 3",
    },
  ],
  ["path", { d: "M8.5 7.5h7" }],
  ["path", { d: "M8.5 11h7" }],
  ["path", { d: "M8.5 14.5h4" }],
])

export const Redo2Icon: Icon = createIcon("redo-2", [
  ["path", { d: "M15.75 9h-6.5a5.5 5.5 0 0 0-5.5 5.5 5.5 5.5 0 0 0 5.5 5.5H13" }],
  ["path", { d: "m15.75 4.5 3.09 3.09a2 2 0 0 1 0 2.82l-3.09 3.09" }],
])

export const RefreshCcwIcon: Icon = createIcon("refresh-ccw", [
  ["path", { d: "M19.63 13.34A7.76 7.76 0 0 0 9.1 4.81" }],
  ["path", { d: "M10.11 7.95 7.55 6.87a1.88 1.88 0 0 1-.99-2.47l1.09-2.55" }],
  ["path", { d: "M4.37 10.66a7.76 7.76 0 0 0 10.53 8.53" }],
  ["path", { d: "m13.89 16.05 2.56 1.08a1.88 1.88 0 0 1 .99 2.47l-1.09 2.55" }],
])

export const RefreshCwIcon: Icon = createIcon("refresh-cw", [
  ["path", { d: "M4.37 13.34A7.76 7.76 0 0 1 14.9 4.81" }],
  ["path", { d: "m13.89 7.95 2.56-1.08a1.88 1.88 0 0 0 .99-2.47l-1.09-2.55" }],
  ["path", { d: "M19.63 10.66A7.76 7.76 0 0 1 9.1 19.19" }],
  ["path", { d: "m10.11 16.05-2.56 1.08a1.88 1.88 0 0 0-.99 2.47l1.09 2.55" }],
])

export const RegexIcon: Icon = createIcon("regex", [
  ["path", { d: "M16.5 3v10" }],
  ["path", { d: "m20.83 10.5-8.66-5" }],
  ["path", { d: "m12.17 10.5 8.66-5" }],
  ["path", { d: "M5 15h2a2 2 0 0 1 2 2v2a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-2a2 2 0 0 1 2-2" }],
])

export const RepeatIcon: Icon = createIcon("repeat", [
  ["path", { d: "M3 11V9a3 3 0 0 1 3-3h11" }],
  ["path", { d: "m17.5 2.5 2.26 2.26a1.75 1.75 0 0 1 0 2.48L17.5 9.5" }],
  ["path", { d: "M21 13v2a3 3 0 0 1-3 3H7" }],
  ["path", { d: "m6.5 14.5-2.26 2.26a1.75 1.75 0 0 0 0 2.48L6.5 21.5" }],
])

export const ReplyIcon: Icon = createIcon("reply", [
  ["path", { d: "M20.25 19v-1.5a5.5 5.5 0 0 0-5.5-5.5h-6" }],
  ["path", { d: "m9.25 6.5-4.09 4.09a2 2 0 0 0 0 2.82l4.09 4.09" }],
])

export const RocketIcon: Icon = createIcon("rocket", [
  [
    "path",
    {
      d: "M5.73 13.32 9.55 9.5a4 4 0 0 1 1.38-.9l3.62-1.42a1.75 1.75 0 0 1 2.27 2.27l-1.42 3.62a4 4 0 0 1-.9 1.38l-3.82 3.82a1.25 1.25 0 0 1-1.77 0l-3.18-3.18a1.25 1.25 0 0 1 0-1.77",
    },
  ],
  ["path", { d: "M7.67 11.38H4.22a1.25 1.25 0 0 0-1.07 1.89l.99 1.64" }],
  ["path", { d: "M12.62 16.33v3.45a1.25 1.25 0 0 1-1.89 1.07l-1.64-.99" }],
  ["path", { d: "M11.05 11.2a1.75 1.75 0 1 0 3.5 0 1.75 1.75 0 1 0-3.5 0" }],
  ["path", { d: "m5.2 18.8-1.95 1.95" }],
])

export const RotateCcwIcon: Icon = createIcon("rotate-ccw", [
  ["path", { d: "M5.49 17.46A8.5 8.5 0 1 0 6.54 5.49" }],
  ["path", { d: "m8.21 8.65-2.94-.26a2 2 0 0 1-1.82-2.16l.26-2.94" }],
])

export const RotateCwIcon: Icon = createIcon("rotate-cw", [
  ["path", { d: "M18.51 17.46a8.5 8.5 0 1 1-1.05-11.97" }],
  ["path", { d: "m15.79 8.65 2.94-.26a2 2 0 0 0 1.82-2.16l-.26-2.94" }],
])

export const Rows2Icon: Icon = createIcon("rows-2", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "M6.25 12h11.5" }],
])

export const SaveIcon: Icon = createIcon("save", [
  [
    "path",
    {
      d: "M6 3h8.78a1.75 1.75 0 0 1 1.23.51l4.48 4.48A1.75 1.75 0 0 1 21 9.22V18a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3",
    },
  ],
  ["path", { d: "M7.5 17.75V15A1.5 1.5 0 0 1 9 13.5h6a1.5 1.5 0 0 1 1.5 1.5v2.75" }],
  ["path", { d: "M7.5 7.25H13" }],
])

export const ScaleIcon: Icon = createIcon("scale", [
  ["path", { d: "M12 3v17.75" }],
  ["path", { d: "M2.75 6.25H9" }],
  ["path", { d: "M15 6.25h6.25" }],
  [
    "path",
    {
      d: "m6.37 10.79 1.99 3.52a1 1 0 0 1-.19 1.23l-.8.74a2.75 2.75 0 0 1-3.74 0l-.8-.74a1 1 0 0 1-.19-1.23l1.99-3.52a1 1 0 0 1 1.74 0",
    },
  ],
  [
    "path",
    {
      d: "m19.37 10.79 1.99 3.52a1 1 0 0 1-.19 1.23l-.8.74a2.75 2.75 0 0 1-3.74 0l-.8-.74a1 1 0 0 1-.19-1.23l1.99-3.52a1 1 0 0 1 1.74 0",
    },
  ],
  ["path", { d: "M8 20.75h8" }],
])

export const ScanIcon: Icon = createIcon("scan", [
  ["path", { d: "M3 8V6a3 3 0 0 1 3-3h2" }],
  ["path", { d: "M16 3h2a3 3 0 0 1 3 3v2" }],
  ["path", { d: "M21 16v2a3 3 0 0 1-3 3h-2" }],
  ["path", { d: "M8 21H6a3 3 0 0 1-3-3v-2" }],
])

export const ScanLineIcon: Icon = createIcon("scan-line", [
  ["path", { d: "M3 8V6a3 3 0 0 1 3-3h2" }],
  ["path", { d: "M16 3h2a3 3 0 0 1 3 3v2" }],
  ["path", { d: "M21 16v2a3 3 0 0 1-3 3h-2" }],
  ["path", { d: "M8 21H6a3 3 0 0 1-3-3v-2" }],
  ["path", { d: "M7 12h10" }],
])

export const ScissorsIcon: Icon = createIcon("scissors", [
  ["path", { d: "M20.5 17.75 4.54 8.87a3 3 0 1 1 3-.05" }],
  ["path", { d: "m20.5 6.25-7.25 4.03" }],
  ["path", { d: "m7.08 13.72-2.54 1.41a3 3 0 1 0 3 .05" }],
])

export const ScrollTextIcon: Icon = createIcon("scroll-text", [
  [
    "path",
    {
      d: "M5.5 7a2 2 0 1 1 2-2v13.5A2.5 2.5 0 0 0 10 21h7a2.5 2.5 0 0 0 2.5-2.5v-13A2.5 2.5 0 0 0 17 3H5.5",
    },
  ],
  ["path", { d: "M11 8.5h5" }],
  ["path", { d: "M11 12.5h5" }],
  ["path", { d: "M11 16.5h3" }],
])

export const SearchIcon: Icon = createIcon("search", [
  ["path", { d: "M4 10.75a6.75 6.75 0 1 0 13.5 0 6.75 6.75 0 1 0-13.5 0" }],
  ["path", { d: "m15.75 15.75 4.5 4.5" }],
])

export const SearchXIcon: Icon = createIcon("search-x", [
  ["path", { d: "M4 10.75a6.75 6.75 0 1 0 13.5 0 6.75 6.75 0 1 0-13.5 0" }],
  ["path", { d: "m15.75 15.75 4.5 4.5" }],
  ["path", { d: "m8.25 8.25 5 5" }],
  ["path", { d: "m13.25 8.25-5 5" }],
])

export const SendIcon: Icon = createIcon("send", [
  [
    "path",
    {
      d: "m19.9 5.69-3.91 11.26a1.5 1.5 0 0 1-2.79.12l-1.72-3.8a1.5 1.5 0 0 0-.75-.75l-3.8-1.72a1.5 1.5 0 0 1 .12-2.79L18.31 4.1a1.25 1.25 0 0 1 1.59 1.59",
    },
  ],
  ["path", { d: "m11.25 12.75 5-5" }],
])

export const ServerIcon: Icon = createIcon("server", [
  [
    "path",
    {
      d: "M5.5 3h13A2.5 2.5 0 0 1 21 5.5V8a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 8V5.5A2.5 2.5 0 0 1 5.5 3",
    },
  ],
  [
    "path",
    {
      d: "M5.5 13.5h13A2.5 2.5 0 0 1 21 16v2.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 18.5V16a2.5 2.5 0 0 1 2.5-2.5",
    },
  ],
  ["path", { d: "M7 6.75h.01" }],
  ["path", { d: "M7 17.25h.01" }],
])

export const SettingsIcon: Icon = createIcon("settings", [
  [
    "path",
    {
      d: "m9.17 5.26.2-.98a1.5 1.5 0 0 1 1.47-1.21h2.32a1.5 1.5 0 0 1 1.47 1.21l.2.98a1.5 1.5 0 0 0 .72 1.01l-.37-.21a1.5 1.5 0 0 0 1.24.12l.95-.32a1.5 1.5 0 0 1 1.79.67l1.16 2.01a1.5 1.5 0 0 1-.31 1.88l-.76.66a1.5 1.5 0 0 0-.52 1.13v-.42a1.5 1.5 0 0 0 .52 1.13l.76.66a1.5 1.5 0 0 1 .31 1.88l-1.16 2.01a1.5 1.5 0 0 1-1.79.67l-.95-.32a1.5 1.5 0 0 0-1.24.12l.37-.21a1.5 1.5 0 0 0-.72 1.01l-.2.98a1.5 1.5 0 0 1-1.47 1.21h-2.32a1.5 1.5 0 0 1-1.47-1.21l-.2-.98a1.5 1.5 0 0 0-.72-1.01l.37.21a1.5 1.5 0 0 0-1.24-.12l-.95.32a1.5 1.5 0 0 1-1.79-.67l-1.16-2.01a1.5 1.5 0 0 1 .31-1.88l.76-.66a1.5 1.5 0 0 0 .52-1.13v.42a1.5 1.5 0 0 0-.52-1.13l-.76-.66a1.5 1.5 0 0 1-.31-1.88l1.16-2.01a1.5 1.5 0 0 1 1.79-.67l.95.32a1.5 1.5 0 0 0 1.24-.12l-.37.21a1.5 1.5 0 0 0 .72-1.01",
    },
  ],
  ["path", { d: "M9.25 12a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
])

export const Settings2Icon: Icon = createIcon("settings-2", [
  ["path", { d: "M5.75 7a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "M14.25 7H21" }],
  ["path", { d: "M12.75 17a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "M3 17h6.75" }],
])

export const ShapesIcon: Icon = createIcon("shapes", [
  [
    "path",
    {
      d: "m13.07 4.29 2.29 3.82A1.25 1.25 0 0 1 14.29 10H9.71a1.25 1.25 0 0 1-1.07-1.89l2.29-3.82a1.25 1.25 0 0 1 2.14 0",
    },
  ],
  ["path", { d: "M5 13.5h3.5a2 2 0 0 1 2 2V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-3.5a2 2 0 0 1 2-2" }],
  ["path", { d: "M13.5 17.25a3.75 3.75 0 1 0 7.5 0 3.75 3.75 0 1 0-7.5 0" }],
])

export const ShareIcon: Icon = createIcon("share", [
  ["path", { d: "M4.5 11v6.75a3 3 0 0 0 3 3h9a3 3 0 0 0 3-3V11" }],
  ["path", { d: "m16 7.25-2.59-2.59a2 2 0 0 0-2.82 0L8 7.25" }],
  ["path", { d: "M12 15.25v-7.5" }],
])

export const Share2Icon: Icon = createIcon("share-2", [
  ["path", { d: "M15.25 5.25a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "M3.25 12a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "M15.25 18.75a2.75 2.75 0 1 0 5.5 0 2.75 2.75 0 1 0-5.5 0" }],
  ["path", { d: "m8.4 10.65 7.2-4.05" }],
  ["path", { d: "m8.4 13.35 7.2 4.05" }],
])

export const ShieldIcon: Icon = createIcon("shield", [
  [
    "path",
    {
      d: "m13.63 3.31 5.02 1.73A2 2 0 0 1 20 6.93v5.32C20 17 16.5 20 12 21.5 7.5 20 4 17 4 12.25V6.93a2 2 0 0 1 1.35-1.89l5.02-1.73a5 5 0 0 1 3.26 0",
    },
  ],
])

export const ShieldAlertIcon: Icon = createIcon("shield-alert", [
  [
    "path",
    {
      d: "m13.63 3.31 5.02 1.73A2 2 0 0 1 20 6.93v5.32C20 17 16.5 20 12 21.5 7.5 20 4 17 4 12.25V6.93a2 2 0 0 1 1.35-1.89l5.02-1.73a5 5 0 0 1 3.26 0",
    },
  ],
  ["path", { d: "M12 7.75v4.75" }],
  ["path", { d: "M12 16h.01" }],
])

export const ShieldCheckIcon: Icon = createIcon("shield-check", [
  [
    "path",
    {
      d: "m13.63 3.31 5.02 1.73A2 2 0 0 1 20 6.93v5.32C20 17 16.5 20 12 21.5 7.5 20 4 17 4 12.25V6.93a2 2 0 0 1 1.35-1.89l5.02-1.73a5 5 0 0 1 3.26 0",
    },
  ],
  ["path", { d: "m8.5 12 1.26 1.26a1.75 1.75 0 0 0 2.48 0l3.51-3.51" }],
])

export const ShieldXIcon: Icon = createIcon("shield-x", [
  [
    "path",
    {
      d: "m13.63 3.31 5.02 1.73A2 2 0 0 1 20 6.93v5.32C20 17 16.5 20 12 21.5 7.5 20 4 17 4 12.25V6.93a2 2 0 0 1 1.35-1.89l5.02-1.73a5 5 0 0 1 3.26 0",
    },
  ],
  ["path", { d: "m9.25 9.25 5.5 5.5" }],
  ["path", { d: "m14.75 9.25-5.5 5.5" }],
])

export const ShoppingBagIcon: Icon = createIcon("shopping-bag", [
  [
    "path",
    {
      d: "M7.04 7.5h9.92a2 2 0 0 1 1.99 1.78l.99 8.94A2.5 2.5 0 0 1 17.46 21H6.54a2.5 2.5 0 0 1-2.48-2.78l.99-8.94A2 2 0 0 1 7.04 7.5",
    },
  ],
  ["path", { d: "M15.5 10.5a3.5 3.5 0 0 1-7 0" }],
])

export const ShoppingCartIcon: Icon = createIcon("shopping-cart", [
  ["path", { d: "M2.5 3.5h.98a1.25 1.25 0 0 1 1.22 1l2.06 10.29A1.5 1.5 0 0 0 8.23 16H18.5" }],
  ["path", { d: "M5.25 6.5h13.93a1.25 1.25 0 0 1 1.22 1.53l-.93 4a1.25 1.25 0 0 1-1.21.97H6.5" }],
  ["path", { d: "M7 20a1.5 1.5 0 1 0 3 0 1.5 1.5 0 1 0-3 0" }],
  ["path", { d: "M16 20a1.5 1.5 0 1 0 3 0 1.5 1.5 0 1 0-3 0" }],
])

export const ShuffleIcon: Icon = createIcon("shuffle", [
  ["path", { d: "M2.75 18h1.96a3 3 0 0 0 2.64-1.57l4.8-8.86A3 3 0 0 1 14.79 6h2.46" }],
  ["path", { d: "M2.75 6h1.96a3 3 0 0 1 2.64 1.57l.69 1.28" }],
  ["path", { d: "m11.46 15.15.69 1.28A3 3 0 0 0 14.79 18h2.46" }],
  ["path", { d: "m18 2.5 2.26 2.26a1.75 1.75 0 0 1 0 2.48L18 9.5" }],
  ["path", { d: "m18 14.5 2.26 2.26a1.75 1.75 0 0 1 0 2.48L18 21.5" }],
])

export const SkipBackIcon: Icon = createIcon("skip-back", [
  [
    "path",
    {
      d: "m16.43 6.68-5.41 3.42a2.25 2.25 0 0 0 0 3.8l5.41 3.42a2 2 0 0 0 3.07-1.7V8.38a2 2 0 0 0-3.07-1.7",
    },
  ],
  ["path", { d: "M4.5 5v14" }],
])

export const SkipForwardIcon: Icon = createIcon("skip-forward", [
  [
    "path",
    {
      d: "m7.57 6.68 5.41 3.42a2.25 2.25 0 0 1 0 3.8l-5.41 3.42a2 2 0 0 1-3.07-1.7V8.38a2 2 0 0 1 3.07-1.7",
    },
  ],
  ["path", { d: "M19.5 5v14" }],
])

export const SlashCommandIcon: Icon = createIcon("slash-command", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "m14 8-4 8" }],
])

export const SlidersHorizontalIcon: Icon = createIcon("sliders-horizontal", [
  ["path", { d: "M6.75 5.5a2.25 2.25 0 1 0 4.5 0 2.25 2.25 0 1 0-4.5 0" }],
  ["path", { d: "M3 5.5h.75" }],
  ["path", { d: "M14.25 5.5H21" }],
  ["path", { d: "M13.25 12a2.25 2.25 0 1 0 4.5 0 2.25 2.25 0 1 0-4.5 0" }],
  ["path", { d: "M3 12h7.25" }],
  ["path", { d: "M20.75 12H21" }],
  ["path", { d: "M5.25 18.5a2.25 2.25 0 1 0 4.5 0 2.25 2.25 0 1 0-4.5 0" }],
  ["path", { d: "M12.75 18.5H21" }],
])

export const SmartphoneIcon: Icon = createIcon("smartphone", [
  ["path", { d: "M9 2.5h6a3 3 0 0 1 3 3v13a3 3 0 0 1-3 3H9a3 3 0 0 1-3-3v-13a3 3 0 0 1 3-3" }],
  ["path", { d: "M12 17.75h.01" }],
])

export const SmileIcon: Icon = createIcon("smile", [
  ["path", { d: "M3 12a9 9 0 1 0 18 0 9 9 0 1 0-18 0" }],
  ["path", { d: "M9 9.5h.01" }],
  ["path", { d: "M15 9.5h.01" }],
  ["path", { d: "M15.9 14a4.5 4.5 0 0 1-7.8 0" }],
])

export const SnowflakeIcon: Icon = createIcon("snowflake", [
  ["path", { d: "M12 2.5v19" }],
  ["path", { d: "m3.77 7.25 16.46 9.5" }],
  ["path", { d: "m3.77 16.75 16.46-9.5" }],
  ["path", { d: "m9.3 3.68 2.21 1.9a.75.75 0 0 0 .98 0l2.21-1.9" }],
  ["path", { d: "m17.85 5.5-.53 2.87a.75.75 0 0 0 .48.84l2.76.97" }],
  ["path", { d: "m20.56 13.82-2.76.97a.75.75 0 0 0-.48.84l.53 2.87" }],
  ["path", { d: "m14.7 20.32-2.21-1.9a.75.75 0 0 0-.98 0l-2.21 1.9" }],
  ["path", { d: "m6.15 18.5.53-2.87a.75.75 0 0 0-.48-.84l-2.76-.97" }],
  ["path", { d: "m3.44 10.18 2.76-.97a.75.75 0 0 0 .48-.84L6.15 5.5" }],
])

export const SquareIcon: Icon = createIcon("square", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
])

export const SquareDashedIcon: Icon = createIcon("square-dashed", [
  ["path", { d: "M3 7V6a3 3 0 0 1 3-3h1" }],
  ["path", { d: "M17 3h1a3 3 0 0 1 3 3v1" }],
  ["path", { d: "M21 17v1a3 3 0 0 1-3 3h-1" }],
  ["path", { d: "M7 21H6a3 3 0 0 1-3-3v-1" }],
  ["path", { d: "M10.5 3h3" }],
  ["path", { d: "M21 10.5v3" }],
  ["path", { d: "M13.5 21h-3" }],
  ["path", { d: "M3 13.5v-3" }],
])

export const SquarePenIcon: Icon = createIcon("square-pen", [
  ["path", { d: "M11.5 3.5h-5a3 3 0 0 0-3 3v11a3 3 0 0 0 3 3h11a3 3 0 0 0 3-3v-5" }],
  ["path", { d: "M18.2 3.8a1.9 1.9 0 0 1 2.7 2.7L13.4 14 10 15l1-3.4Z" }],
])

export const SquareTerminalIcon: Icon = createIcon("square-terminal", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "m7.5 8.75 1.87 1.87a1.25 1.25 0 0 1 0 1.76L7.5 14.25" }],
  ["path", { d: "M12.5 15.25h4" }],
])

export const StarIcon: Icon = createIcon("star", [
  [
    "path",
    {
      d: "m13.39 6.07.96 2.39a1 1 0 0 0 .86.62l2.57.18a1.5 1.5 0 0 1 .86 2.64l-1.97 1.66a1 1 0 0 0-.33 1.01l.63 2.5a1.5 1.5 0 0 1-2.26 1.63l-2.18-1.37a1 1 0 0 0-1.06 0L9.29 18.7a1.5 1.5 0 0 1-2.26-1.63l.63-2.5a1 1 0 0 0-.33-1.01L5.36 11.9a1.5 1.5 0 0 1 .86-2.64l2.57-.18a1 1 0 0 0 .86-.62l.96-2.39a1.5 1.5 0 0 1 2.78 0",
    },
  ],
])

export const StoreIcon: Icon = createIcon("store", [
  [
    "path",
    {
      d: "M6.39 3h11.22a1.5 1.5 0 0 1 1.32.78L21.25 8a2.31 2.31 0 0 1-4.62 0h-.01A2.31 2.31 0 0 1 12 8a2.31 2.31 0 0 1-4.62 0h-.01a2.31 2.31 0 0 1-4.62 0l2.32-4.22A1.5 1.5 0 0 1 6.39 3",
    },
  ],
  ["path", { d: "M4.5 13.25V19a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2v-5.75" }],
  ["path", { d: "M9.5 21v-3a1.5 1.5 0 0 1 1.5-1.5h2a1.5 1.5 0 0 1 1.5 1.5v3" }],
])

export const SubagentIcon: Icon = createIcon("subagent", [
  ["path", { d: "M5 3h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2" }],
  ["path", { d: "M6.5 11.5v4.25a2.5 2.5 0 0 0 2.5 2.5h3.75" }],
  ["path", { d: "M6.5 12h6.25" }],
  [
    "path",
    {
      d: "M15.75 9.75h2A1.25 1.25 0 0 1 19 11v2a1.25 1.25 0 0 1-1.25 1.25h-2A1.25 1.25 0 0 1 14.5 13v-2a1.25 1.25 0 0 1 1.25-1.25",
    },
  ],
  [
    "path",
    {
      d: "M15.75 16h2A1.25 1.25 0 0 1 19 17.25v2a1.25 1.25 0 0 1-1.25 1.25h-2a1.25 1.25 0 0 1-1.25-1.25v-2A1.25 1.25 0 0 1 15.75 16",
    },
  ],
])

export const SunIcon: Icon = createIcon("sun", [
  ["path", { d: "M8 12a4 4 0 1 0 8 0 4 4 0 1 0-8 0" }],
  ["path", { d: "M19 12h2.25" }],
  ["path", { d: "m16.95 16.95 1.59 1.59" }],
  ["path", { d: "M12 19v2.25" }],
  ["path", { d: "m7.05 16.95-1.59 1.59" }],
  ["path", { d: "M5 12H2.75" }],
  ["path", { d: "M7.05 7.05 5.46 5.46" }],
  ["path", { d: "M12 5V2.75" }],
  ["path", { d: "m16.95 7.05 1.59-1.59" }],
])

export const SunriseIcon: Icon = createIcon("sunrise", [
  ["path", { d: "M7.5 18a4.5 4.5 0 0 1 9 0" }],
  ["path", { d: "M5.25 18H3" }],
  ["path", { d: "m7.23 13.23-1.59-1.59" }],
  ["path", { d: "m16.77 13.23 1.59-1.59" }],
  ["path", { d: "M18.75 18H21" }],
  ["path", { d: "M2.5 21.25h19" }],
  ["path", { d: "m9 6 1.94-1.94a1.5 1.5 0 0 1 2.12 0L15 6" }],
  ["path", { d: "M12 7.25v2.5" }],
])

export const TableIcon: Icon = createIcon("table", [
  ["path", { d: "M6 3h12a3 3 0 0 1 3 3v12a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3" }],
  ["path", { d: "M6.25 9h11.5" }],
  ["path", { d: "M6.25 15H9" }],
  ["path", { d: "M15 15h2.75" }],
  ["path", { d: "M12 12v5.75" }],
])

export const TabletIcon: Icon = createIcon("tablet", [
  ["path", { d: "M7 2.5h10a3 3 0 0 1 3 3v13a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3v-13a3 3 0 0 1 3-3" }],
  ["path", { d: "M12 17.75h.01" }],
])

export const TagIcon: Icon = createIcon("tag", [
  [
    "path",
    {
      d: "M5.5 3h5.17a2 2 0 0 1 1.42.59l7.25 7.25a2 2 0 0 1 0 2.82l-5.68 5.68a2 2 0 0 1-2.82 0l-7.25-7.25A2 2 0 0 1 3 10.67V5.5A2.5 2.5 0 0 1 5.5 3",
    },
  ],
  ["path", { d: "M7 7.75a.75.75 0 1 0 1.5 0 .75.75 0 1 0-1.5 0" }],
])

export const TerminalIcon: Icon = createIcon("terminal", [
  ["path", { d: "m4.5 6 4.09 4.09a2 2 0 0 1 0 2.82L4.5 17" }],
  ["path", { d: "M12.5 18h7" }],
])

export const TestTubeIcon: Icon = createIcon("test-tube", [
  ["path", { d: "M9.5 3v15.75a2.5 2.5 0 0 0 2.5 2.5 2.5 2.5 0 0 0 2.5-2.5V3" }],
  ["path", { d: "M7.5 3h9" }],
  ["path", { d: "M9.5 14h5" }],
])

export const ThumbsDownIcon: Icon = createIcon("thumbs-down", [
  [
    "path",
    {
      d: "M9.5 4.5v9.06a1 1 0 0 0 .07.37l1.85 4.71a1.75 1.75 0 0 0 2.66.78l.14-.1a1.5 1.5 0 0 0 .59-1.51l-.57-2.86a1 1 0 0 1 .98-1.2h2.12a2 2 0 0 0 1.97-2.37l-1.26-6.75A2 2 0 0 0 16.09 3H11a1.5 1.5 0 0 0-1.5 1.5",
    },
  ],
  [
    "path",
    {
      d: "M4.25 13.75H5a1.5 1.5 0 0 0 1.5-1.5V4.5A1.5 1.5 0 0 0 5 3h-.75a1.5 1.5 0 0 0-1.5 1.5v7.75a1.5 1.5 0 0 0 1.5 1.5",
    },
  ],
])

export const ThumbsUpIcon: Icon = createIcon("thumbs-up", [
  [
    "path",
    {
      d: "M9.5 19.5v-9.06a1 1 0 0 1 .07-.37l1.85-4.71a1.75 1.75 0 0 1 2.66-.78l.14.1a1.5 1.5 0 0 1 .59 1.51l-.57 2.86a1 1 0 0 0 .98 1.2h2.12a2 2 0 0 1 1.97 2.37l-1.26 6.75A2 2 0 0 1 16.09 21H11a1.5 1.5 0 0 1-1.5-1.5",
    },
  ],
  [
    "path",
    {
      d: "M4.25 10.25H5a1.5 1.5 0 0 1 1.5 1.5v7.75A1.5 1.5 0 0 1 5 21h-.75a1.5 1.5 0 0 1-1.5-1.5v-7.75a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
])

export const TicketIcon: Icon = createIcon("ticket", [
  [
    "path",
    {
      d: "M4.5 5h15a2 2 0 0 1 2 2v2a3 3 0 0 0 0 6v2a2 2 0 0 1-2 2h-15a2 2 0 0 1-2-2v-2a3 3 0 0 0 0-6V7a2 2 0 0 1 2-2",
    },
  ],
  ["path", { d: "M14 8v2" }],
  ["path", { d: "M14 14v2" }],
])

export const TimerIcon: Icon = createIcon("timer", [
  ["path", { d: "M4.25 13.75a7.75 7.75 0 1 0 15.5 0 7.75 7.75 0 1 0-15.5 0" }],
  ["path", { d: "M10 2.75h4" }],
  ["path", { d: "M12 13.75 14.75 11" }],
])

export const ToolIcon: Icon = createIcon("tool", [
  [
    "path",
    {
      d: "M19.76 6.79a5.25 5.25 0 0 1-7.16 6.88l-6.22 6.21a1.6 1.6 0 0 1-2.26-2.26l6.21-6.22a5.25 5.25 0 0 1 6.88-7.16l-2.42 2.43 2.54 2.54Z",
    },
  ],
])

export const TraceIcon: Icon = createIcon("trace", [
  ["path", { d: "M4 5.5a1.5 1.5 0 1 0 3 0 1.5 1.5 0 1 0-3 0" }],
  ["path", { d: "M4 12a1.5 1.5 0 1 0 3 0 1.5 1.5 0 1 0-3 0" }],
  ["path", { d: "M4 18.5a1.5 1.5 0 1 0 3 0 1.5 1.5 0 1 0-3 0" }],
  ["path", { d: "M5.5 8.25v1" }],
  ["path", { d: "M5.5 14.75v1" }],
  ["path", { d: "M10 5.5h10.5" }],
  ["path", { d: "M10 12h8" }],
  ["path", { d: "M10 18.5h10.5" }],
])

export const TrashIcon: Icon = createIcon("trash", [
  ["path", { d: "M3.5 6h17" }],
  ["path", { d: "M10 3h4" }],
  ["path", { d: "m5.75 9.25.6 9.41A2.5 2.5 0 0 0 8.85 21h6.3a2.5 2.5 0 0 0 2.5-2.34l.6-9.41" }],
])

export const Trash2Icon: Icon = createIcon("trash-2", [
  ["path", { d: "M3.5 6h17" }],
  ["path", { d: "M10 3h4" }],
  ["path", { d: "m5.75 9.25.6 9.41A2.5 2.5 0 0 0 8.85 21h6.3a2.5 2.5 0 0 0 2.5-2.34l.6-9.41" }],
  ["path", { d: "M10 11.5V17" }],
  ["path", { d: "M14 11.5V17" }],
])

export const TrendingDownIcon: Icon = createIcon("trending-down", [
  ["path", { d: "m2.5 9 4.94 4.94a1.5 1.5 0 0 0 2.12 0l2.88-2.88a1.5 1.5 0 0 1 2.12 0l2.9 2.9" }],
  ["path", { d: "M15.5 17.5H19a2 2 0 0 0 2-2V12" }],
])

export const TrendingUpIcon: Icon = createIcon("trending-up", [
  ["path", { d: "m2.5 15 4.94-4.94a1.5 1.5 0 0 1 2.12 0l2.88 2.88a1.5 1.5 0 0 0 2.12 0l2.9-2.9" }],
  ["path", { d: "M15.5 6.5H19a2 2 0 0 1 2 2V12" }],
])

export const TriangleIcon: Icon = createIcon("triangle", [
  [
    "path",
    {
      d: "m13.74 4.64 6.95 12.17a2 2 0 0 1-1.74 2.99H5.05a2 2 0 0 1-1.74-2.99l6.95-12.17a2 2 0 0 1 3.48 0",
    },
  ],
])

export const TriangleAlertIcon: Icon = createIcon("triangle-alert", [
  [
    "path",
    {
      d: "m13.74 4.64 6.95 12.17a2 2 0 0 1-1.74 2.99H5.05a2 2 0 0 1-1.74-2.99l6.95-12.17a2 2 0 0 1 3.48 0",
    },
  ],
  ["path", { d: "M12 9.25V13" }],
  ["path", { d: "M12 16.25h.01" }],
])

export const TrophyIcon: Icon = createIcon("trophy", [
  [
    "path",
    {
      d: "M8 3h8a1.5 1.5 0 0 1 1.5 1.5V9a5.5 5.5 0 0 1-5.5 5.5A5.5 5.5 0 0 1 6.5 9V4.5A1.5 1.5 0 0 1 8 3",
    },
  ],
  ["path", { d: "M6.5 9.75a2.75 2.75 0 0 1 0-5.5" }],
  ["path", { d: "M17.5 9.75a2.75 2.75 0 0 0 0-5.5" }],
  ["path", { d: "M12 17.25V21" }],
  ["path", { d: "M7 21h10" }],
])

export const TruckIcon: Icon = createIcon("truck", [
  [
    "path",
    {
      d: "M2.5 16.36a1.5 1.5 0 0 1 0-.11V6.5a2 2 0 0 1 2-2H12a2 2 0 0 1 2 2v1a1 1 0 0 0 1 1h2.07a1.5 1.5 0 0 1 1.13.51l2.92 3.31a1.5 1.5 0 0 1 .38 1v2.93a1.5 1.5 0 0 1 0 .11",
    },
  ],
  ["path", { d: "M9.5 17.75a2.5 2.5 0 1 1-5 0 2.5 2.5 0 1 1 5 0" }],
  ["path", { d: "M19.5 17.75a2.5 2.5 0 1 1-5 0 2.5 2.5 0 1 1 5 0" }],
])

export const TypeIcon: Icon = createIcon("type", [
  ["path", { d: "M4.5 7.5v-2A1.5 1.5 0 0 1 6 4h12a1.5 1.5 0 0 1 1.5 1.5v2" }],
  ["path", { d: "M12 7v13" }],
  ["path", { d: "M9 20h6" }],
])

export const Undo2Icon: Icon = createIcon("undo-2", [
  ["path", { d: "M8.25 9h6.5a5.5 5.5 0 0 1 5.5 5.5 5.5 5.5 0 0 1-5.5 5.5H11" }],
  ["path", { d: "M8.25 4.5 5.16 7.59a2 2 0 0 0 0 2.82l3.09 3.09" }],
])

export const UnlockIcon: Icon = createIcon("unlock", [
  ["path", { d: "M7 10.5h10a3 3 0 0 1 3 3V18a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3v-4.5a3 3 0 0 1 3-3" }],
  ["path", { d: "M8 7.5V7a4 4 0 0 1 7.46-2" }],
])

export const UploadIcon: Icon = createIcon("upload", [
  ["path", { d: "M3.5 15v2.5a3 3 0 0 0 3 3h11a3 3 0 0 0 3-3V15" }],
  ["path", { d: "m16.5 8-3.09-3.09a2 2 0 0 0-2.82 0L7.5 8" }],
  ["path", { d: "M12 15.5V8" }],
])

export const UserIcon: Icon = createIcon("user", [
  ["path", { d: "M7.75 7.75a4.25 4.25 0 1 0 8.5 0 4.25 4.25 0 1 0-8.5 0" }],
  ["path", { d: "M4.75 21v-1.75a4 4 0 0 1 4-4h6.5a4 4 0 0 1 4 4V21" }],
])

export const UserPlusIcon: Icon = createIcon("user-plus", [
  ["path", { d: "M13 7.75a4 4 0 1 1-8 0 4 4 0 1 1 8 0" }],
  ["path", { d: "M2.5 21v-1.5a4 4 0 0 1 4-4h5a4 4 0 0 1 4 4V21" }],
  ["path", { d: "M16.5 10.5h5" }],
  ["path", { d: "M19 8v5" }],
])

export const UserRoundIcon: Icon = createIcon("user-round", [
  ["path", { d: "M7.75 7.5a4.25 4.25 0 1 0 8.5 0 4.25 4.25 0 1 0-8.5 0" }],
  ["path", { d: "M4.76 20.31a7.5 7.5 0 0 1 14.48 0" }],
])

export const UserRoundCheckIcon: Icon = createIcon("user-round-check", [
  ["path", { d: "M5.75 7.5a4.25 4.25 0 1 0 8.5 0 4.25 4.25 0 1 0-8.5 0" }],
  ["path", { d: "M2.76 20.31a7.5 7.5 0 0 1 10.99-4.56" }],
  ["path", { d: "m15.25 18 1.34 1.34a1.25 1.25 0 0 0 1.79-.03l2.87-3.06" }],
])

export const UserXIcon: Icon = createIcon("user-x", [
  ["path", { d: "M5 7.75a4 4 0 1 0 8 0 4 4 0 1 0-8 0" }],
  ["path", { d: "M2.5 21v-1.5a4 4 0 0 1 4-4h5a4 4 0 0 1 4 4V21" }],
  ["path", { d: "m17 8.5 4 4" }],
  ["path", { d: "m21 8.5-4 4" }],
])

export const UsersIcon: Icon = createIcon("users", [
  ["path", { d: "M13 7.75a4 4 0 1 1-8 0 4 4 0 1 1 8 0" }],
  ["path", { d: "M2.5 21v-1.5a4 4 0 0 1 4-4h5a4 4 0 0 1 4 4V21" }],
  ["path", { d: "M15.52 3.76a3.5 3.5 0 0 1 3.73 3.49 3.5 3.5 0 0 1-3.21 3.49" }],
  ["path", { d: "M21.25 21v-3.75a3.75 3.75 0 0 0-3.75-3.75" }],
])

export const UsersRoundIcon: Icon = createIcon("users-round", [
  ["path", { d: "M13 7.75a4 4 0 1 1-8 0 4 4 0 1 1 8 0" }],
  ["path", { d: "M2.24 20.44a7 7 0 0 1 13.52 0" }],
  ["path", { d: "M15.52 3.76a3.5 3.5 0 0 1 3.73 3.49 3.5 3.5 0 0 1-3.21 3.49" }],
  ["path", { d: "M18.23 15.44a7.25 7.25 0 0 1 4.52 4.93" }],
])

export const UtensilsIcon: Icon = createIcon("utensils", [
  ["path", { d: "M7 2.75v18.5" }],
  ["path", { d: "M3.5 2.75V6a2.5 2.5 0 0 0 .71 1.74" }],
  ["path", { d: "M9.79 7.74A2.5 2.5 0 0 0 10.5 6V2.75" }],
  [
    "path",
    {
      d: "M20.5 21.25V5.89a1.25 1.25 0 0 0-2.16-.86l-1.65 1.75A2.5 2.5 0 0 0 16 8.5V13a1.5 1.5 0 0 0 1.5 1.5h.25",
    },
  ],
])

export const VideoIcon: Icon = createIcon("video", [
  ["path", { d: "M5.5 6H12a3 3 0 0 1 3 3v6a3 3 0 0 1-3 3H5.5a3 3 0 0 1-3-3V9a3 3 0 0 1 3-3" }],
  [
    "path",
    {
      d: "m18.42 9.95 1.1-.79a1.25 1.25 0 0 1 1.98 1.02v3.64a1.25 1.25 0 0 1-1.98 1.02l-1.1-.79a1 1 0 0 1-.42-.81v-2.48a1 1 0 0 1 .42-.81",
    },
  ],
])

export const VideoOffIcon: Icon = createIcon("video-off", [
  ["path", { d: "m4 4 16 16" }],
  ["path", { d: "M10.24 6H12a3 3 0 0 1 3 3v1.76" }],
  ["path", { d: "M12.47 17.96A3 3 0 0 1 12 18H5.5a3 3 0 0 1-3-3V9a3 3 0 0 1 .16-.95" }],
  [
    "path",
    {
      d: "m18.42 9.95 1.1-.79a1.25 1.25 0 0 1 1.98 1.02v3.64a1.25 1.25 0 0 1-1.98 1.02l-1.1-.79a1 1 0 0 1-.42-.81v-2.48a1 1 0 0 1 .42-.81",
    },
  ],
])

export const Volume1Icon: Icon = createIcon("volume-1", [
  [
    "path",
    {
      d: "M4.5 8.5h1.63a1.5 1.5 0 0 0 1.06-.44l1.68-1.68a1.25 1.25 0 0 1 2.13.89v9.46a1.25 1.25 0 0 1-2.13.89l-1.68-1.68a1.5 1.5 0 0 0-1.06-.44H4.5A1.5 1.5 0 0 1 3 14v-4a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
  ["path", { d: "M14.93 9.03a4 4 0 0 1 0 5.94" }],
])

export const Volume2Icon: Icon = createIcon("volume-2", [
  [
    "path",
    {
      d: "M4.5 8.5h1.63a1.5 1.5 0 0 0 1.06-.44l1.68-1.68a1.25 1.25 0 0 1 2.13.89v9.46a1.25 1.25 0 0 1-2.13.89l-1.68-1.68a1.5 1.5 0 0 0-1.06-.44H4.5A1.5 1.5 0 0 1 3 14v-4a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
  ["path", { d: "M14.93 9.03a4 4 0 0 1 0 5.94" }],
  ["path", { d: "M17.18 5.7a8 8 0 0 1 0 12.6" }],
])

export const VolumeXIcon: Icon = createIcon("volume-x", [
  [
    "path",
    {
      d: "M4.5 8.5h1.63a1.5 1.5 0 0 0 1.06-.44l1.68-1.68a1.25 1.25 0 0 1 2.13.89v9.46a1.25 1.25 0 0 1-2.13.89l-1.68-1.68a1.5 1.5 0 0 0-1.06-.44H4.5A1.5 1.5 0 0 1 3 14v-4a1.5 1.5 0 0 1 1.5-1.5",
    },
  ],
  ["path", { d: "m15.25 9.25 5.5 5.5" }],
  ["path", { d: "m20.75 9.25-5.5 5.5" }],
])

export const WalletIcon: Icon = createIcon("wallet", [
  ["path", { d: "M5.75 5h12.5a3 3 0 0 1 3 3v9a3 3 0 0 1-3 3H5.75a3 3 0 0 1-3-3V8a3 3 0 0 1 3-3" }],
  ["path", { d: "M21.25 10h-2.5a2.5 2.5 0 0 0-2.5 2.5 2.5 2.5 0 0 0 2.5 2.5h2.5" }],
])

export const WatchIcon: Icon = createIcon("watch", [
  ["path", { d: "M6 12a6 6 0 1 0 12 0 6 6 0 1 0-12 0" }],
  ["path", { d: "m8.25 7.3.53-3.06A1.5 1.5 0 0 1 10.26 3h3.48a1.5 1.5 0 0 1 1.48 1.24l.53 3.06" }],
  [
    "path",
    { d: "m8.25 16.7.53 3.06A1.5 1.5 0 0 0 10.26 21h3.48a1.5 1.5 0 0 0 1.48-1.24l.53-3.06" },
  ],
])

export const WebSearchIcon: Icon = createIcon("web-search", [
  ["path", { d: "M4 10.75a6.75 6.75 0 1 0 13.5 0 6.75 6.75 0 1 0-13.5 0" }],
  ["path", { d: "m15.75 15.75 4.5 4.5" }],
  ["path", { d: "M10.75 6.25C12 7.5 12.5 9 12.5 10.75s-.5 3.25-1.75 4.5" }],
  ["path", { d: "M10.75 6.25C9.5 7.5 9 9 9 10.75s.5 3.25 1.75 4.5" }],
  ["path", { d: "M6.5 10.75H15" }],
])

export const WifiIcon: Icon = createIcon("wifi", [
  ["path", { d: "M8.99 15.49a4.25 4.25 0 0 1 6.02 0" }],
  ["path", { d: "M5.99 12.49a8.5 8.5 0 0 1 12.02 0" }],
  ["path", { d: "M2.98 9.48a12.75 12.75 0 0 1 18.04 0" }],
  ["path", { d: "M12 18.5h.01" }],
])

export const WindIcon: Icon = createIcon("wind", [
  ["path", { d: "M2.5 8h9a2.75 2.75 0 1 0-2.38-4.12" }],
  ["path", { d: "M2.5 12h15a3 3 0 1 0-2.82-4.03" }],
  ["path", { d: "M2.5 16h11a2.5 2.5 0 1 1-2.17 3.75" }],
])

export const WorkflowIcon: Icon = createIcon("workflow", [
  ["path", { d: "M5 3h3.5a2 2 0 0 1 2 2v3.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2" }],
  [
    "path",
    { d: "M15.5 13.5H19a2 2 0 0 1 2 2V19a2 2 0 0 1-2 2h-3.5a2 2 0 0 1-2-2v-3.5a2 2 0 0 1 2-2" },
  ],
  ["path", { d: "M6.75 13.5v1.25a2.5 2.5 0 0 0 2.5 2.5h1.25" }],
])

export const WrapTextIcon: Icon = createIcon("wrap-text", [
  ["path", { d: "M3 6h18" }],
  ["path", { d: "M3 12h15a3 3 0 0 1 0 6h-1.5" }],
  ["path", { d: "m15 15.5-1.62 1.62a1.25 1.25 0 0 0 0 1.76L15 20.5" }],
  ["path", { d: "M3 18h5.5" }],
])

export const XIcon: Icon = createIcon("x", [
  ["path", { d: "m6 6 12 12" }],
  ["path", { d: "m18 6-3.88 3.88" }],
  ["path", { d: "M9.88 14.12 6 18" }],
])

export const ZapIcon: Icon = createIcon("zap", [
  [
    "path",
    {
      d: "M14.5 2.75 9.19 9.82a1.5 1.5 0 0 0 1.43 2.39l2.76-.42a1.5 1.5 0 0 1 1.43 2.39L9.5 21.25",
    },
  ],
])

export const ZoomInIcon: Icon = createIcon("zoom-in", [
  ["path", { d: "M4 10.75a6.75 6.75 0 1 0 13.5 0 6.75 6.75 0 1 0-13.5 0" }],
  ["path", { d: "m15.75 15.75 4.5 4.5" }],
  ["path", { d: "M8 10.75h5.5" }],
  ["path", { d: "M10.75 8v5.5" }],
])

export const ZoomOutIcon: Icon = createIcon("zoom-out", [
  ["path", { d: "M4 10.75a6.75 6.75 0 1 0 13.5 0 6.75 6.75 0 1 0-13.5 0" }],
  ["path", { d: "m15.75 15.75 4.5 4.5" }],
  ["path", { d: "M8 10.75h5.5" }],
])
