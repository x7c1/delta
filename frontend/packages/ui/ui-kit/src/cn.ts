import { twMerge } from 'tailwind-merge';

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
