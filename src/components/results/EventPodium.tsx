import { formatCurrency, formatSeconds } from './formatters'
import type { EventStandingRow } from './types'

interface EventPodiumProps {
  first?: EventStandingRow
  second?: EventStandingRow
  third?: EventStandingRow
}

function PodiumSlot({
  medal,
  row,
  highlight = false,
}: {
  medal: string
  row?: EventStandingRow
  highlight?: boolean
}) {
  if (!row) {
    return (
      <div
        className={
          highlight
            ? 'rounded-xl bg-orange-500 p-8 text-center text-white shadow-lg'
            : 'rounded-xl bg-card p-6 text-center shadow-sm'
        }
      >
        <div className="mb-4 text-4xl">{medal}</div>
        <div className={highlight ? 'opacity-80' : 'text-muted-foreground'}>Sin datos</div>
      </div>
    )
  }

  return (
    <div
      className={
        highlight
          ? 'rounded-xl bg-orange-500 p-8 text-center text-white shadow-lg md:scale-105'
          : 'rounded-xl bg-card p-6 text-center shadow-sm'
      }
    >
      <div className="mb-4 text-4xl">{medal}</div>
      <div className={highlight ? 'mt-3 text-xl font-semibold' : 'text-lg font-medium'}>
        {row.headerName}
      </div>
      <div className={highlight ? 'text-sm opacity-90' : 'text-sm text-muted-foreground'}>
        &amp; {row.heelerName}
      </div>
      <div className={highlight ? 'mt-6 rounded-md bg-white/20 p-6' : 'mt-4 rounded-md bg-muted p-4'}>
        <div className={highlight ? 'text-3xl font-bold' : 'text-2xl font-bold'}>
          {formatSeconds(row.totalTime)}
        </div>
        <div className={highlight ? 'mt-2 text-xs uppercase tracking-wide opacity-80' : 'text-xs text-muted-foreground'}>
          Tiempo Total
        </div>
      </div>
      <div className={highlight ? 'mt-3 text-sm' : 'mt-3 text-sm text-muted-foreground'}>
        Best run: {formatSeconds(row.bestTime)}
      </div>
      <div className={highlight ? 'mt-2 text-2xl font-bold' : 'mt-2 text-lg font-medium text-orange-700'}>
        {formatCurrency(row.payoff)}
      </div>
    </div>
  )
}

export function EventPodium({ first, second, third }: EventPodiumProps) {
  return (
    <div className="mb-6 grid grid-cols-1 items-end gap-6 md:grid-cols-3">
      <div className="order-2 md:order-1">
        <PodiumSlot medal="🥈" row={second} />
      </div>
      <div className="order-1 md:order-2">
        <PodiumSlot medal="🏆" row={first} highlight />
      </div>
      <div className="order-3">
        <PodiumSlot medal="🥉" row={third} />
      </div>
    </div>
  )
}
