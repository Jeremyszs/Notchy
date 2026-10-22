// The font choices. Ids mirror FONTS in src-tauri/src/config.rs, and the order is the order shown
// in Settings. Each stack ends in a generic family, so a missing face still renders.
export type FontId =
  | "sf"
  | "inter"
  | "manrope"
  | "dm-sans"
  | "space-grotesk"
  | "playfair"
  | "system";

export interface FontOption {
  id: FontId;
  label: string;
  stack: string;
}

export const FONT_OPTIONS: FontOption[] = [
  {
    id: "sf",
    label: "SF Pro (if installed)",
    stack: `"SF Pro Display", "SF Pro Text", Inter, system-ui, sans-serif`,
  },
  { id: "inter", label: "Inter", stack: `Inter, system-ui, sans-serif` },
  { id: "manrope", label: "Manrope", stack: `Manrope, system-ui, sans-serif` },
  { id: "dm-sans", label: "DM Sans", stack: `"DM Sans", system-ui, sans-serif` },
  {
    id: "space-grotesk",
    label: "Space Grotesk",
    stack: `"Space Grotesk", system-ui, sans-serif`,
  },
  {
    id: "playfair",
    label: "Playfair Display",
    stack: `"Playfair Display", Georgia, serif`,
  },
  {
    id: "system",
    label: "System (Segoe UI Variable)",
    stack: `"Segoe UI Variable", "Segoe UI", system-ui, sans-serif`,
  },
];

/** The stack for an id. An unknown id gets Inter, the same fallback the backend uses. */
export function fontStack(id: string): string {
  const option = FONT_OPTIONS.find((font) => font.id === id);
  return (option ?? FONT_OPTIONS[1]).stack;
}
