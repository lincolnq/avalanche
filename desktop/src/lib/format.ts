/**
 * Returns up to 2 uppercase initials from a display name.
 * Empty or whitespace-only names return "".
 */
export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter((w) => w.length > 0)
    .slice(0, 2)
    .map((w) => w[0].toUpperCase())
    .join("");
}

/**
 * Returns the hostname of `url`, or `fallback` if the URL cannot be parsed.
 */
export function displayHost(url: string, fallback: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return fallback;
  }
}
