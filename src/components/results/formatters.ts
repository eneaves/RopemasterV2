export function formatCurrency(n: number) {
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
    maximumFractionDigits: 0,
  }).format(n)
}

export function formatSeconds(value: number | null | undefined) {
  if (value === null || value === undefined) return '—'
  return `${value.toFixed(2)}s`
}

export function formatPercent(value: number | null | undefined) {
  if (value === null || value === undefined) return '—'
  return `${Math.round(value)}%`
}
