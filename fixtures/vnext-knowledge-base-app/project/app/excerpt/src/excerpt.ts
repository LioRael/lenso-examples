const DEFAULT_EXCERPT_LIMIT = 96;
const graphemes = new Intl.Segmenter("und", { granularity: "grapheme" });

export function excerpt(text: string, limit = DEFAULT_EXCERPT_LIMIT): string {
  const normalized = text.trim().replace(/\s+/gu, " ");
  const characters = Array.from(graphemes.segment(normalized), ({ segment }) => segment);
  if (characters.length <= limit) return normalized;
  return `${characters.slice(0, limit - 1).join("")}…`;
}
