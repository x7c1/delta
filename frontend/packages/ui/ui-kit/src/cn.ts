import { extendTailwindMerge } from 'tailwind-merge';

/**
 * twMerge classifies unknown `text-*` values as text-COLOR utilities: known
 * scale names (`text-sm`) and arbitrary lengths (`text-[13px]`) land in the
 * font-size group, but the semantic size tokens defined in the app's Tailwind
 * theme (`text-body` / `text-secondary` / `text-caption` / `text-terminal`)
 * are not known to it. Misclassified as colors, they conflict with real color
 * utilities, so `cn('text-secondary', active && 'text-accent')` silently
 * dropped the SIZE on active rows (the element then inherits the ancestor
 * size). Registering the token names in the `text` theme scale (the
 * font-size scale, Tailwind's `--text-*` namespace) restores the correct
 * pairing: sizes conflict only with sizes, colors only with colors.
 * Keep this list in sync with the `--text-*` entries of the `@theme` block in
 * apps/web/src/index.css.
 */
const twMerge = extendTailwindMerge({
  extend: {
    theme: {
      text: ['body', 'secondary', 'caption', 'code', 'terminal'],
    },
  },
});

/**
 * Join truthy class name fragments and dedup conflicting Tailwind utilities.
 *
 * Bare concatenation lets a primitive's default (e.g. `max-w-md`) survive
 * alongside a consumer's override (`max-w-4xl`); the CSS source order
 * decides the winner, not the consumer's intent. Routing through twMerge
 * makes the "later-passed className wins" contract hold per utility class.
 */
export function cn(...parts: Array<string | false | null | undefined>): string {
  return twMerge(parts.filter(Boolean).join(' '));
}
