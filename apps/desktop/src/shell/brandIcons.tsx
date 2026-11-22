// The logos of the platforms an environment runs on or gets its models from,
// for its hover card: monochrome glyphs drawn in `currentColor`, so they take
// the text's color and re-theme with the app.
//
// Path data is vendored, not fetched, from simple-icons 16.34.0 (CC0-1.0):
// `databricks`, `neon`. Each logo remains its owner's trademark.

interface IconProps {
  className?: string;
}

function Glyph({ d, title, className }: IconProps & { d: string; title: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="currentColor"
      role="img"
      aria-label={title}
      className={className}
    >
      <path d={d} />
    </svg>
  );
}

export function DatabricksIcon({ className }: IconProps) {
  return (
    <Glyph
      title="Databricks"
      className={className}
      d="M.95 14.184L12 20.403l9.919-5.55v2.21L12 22.662l-10.484-5.96-.565.308v.77L12 24l11.05-6.218v-4.317l-.515-.309L12 19.118l-9.867-5.653v-2.21L12 16.805l11.05-6.218V6.32l-.515-.308L12 11.974 2.647 6.681 12 1.388l7.76 4.368.668-.411v-.566L12 0 .95 6.27v.72L12 13.207l9.919-5.55v2.26L12 15.52 1.516 9.56l-.565.308Z"
    />
  );
}

export function NeonIcon({ className }: IconProps) {
  return (
    <Glyph
      title="Neon"
      className={className}
      d="M24 0V24l-9.365-8.045V24H0V0ZM2.942 21.087h8.751V9.563l9.365 8.204V2.919L2.942 2.914Z"
    />
  );
}
