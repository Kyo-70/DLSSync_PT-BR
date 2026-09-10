/** Compare version components without converting a packed u64 to a JS number. */
export function compareVersions(left: string, right: string): number {
  const components = (value: string): bigint[] | null => {
    const match = value.trim().match(/^v?(\d+(?:[.,]\d+){0,3})(?:[-+].*)?$/i);
    return match ? match[1].split(/[.,]/).map(value => BigInt(value)) : null;
  };
  const a = components(left), b = components(right);
  if (!a || !b) return left.localeCompare(right, undefined, { numeric: true });
  for (let index = 0; index < Math.max(a.length, b.length); index++) {
    const x = a[index] ?? 0n, y = b[index] ?? 0n;
    if (x !== y) return x < y ? -1 : 1;
  }
  return 0;
}
