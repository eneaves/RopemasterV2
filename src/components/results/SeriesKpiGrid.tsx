import { formatCurrency, formatSeconds } from './formatters'
import type { SeriesRopersSummary } from './types'

interface SeriesKpiGridProps {
  summary: SeriesRopersSummary
}

function KpiCard({
  label,
  value,
  helper,
}: {
  label: string
  value: string | number
  helper?: string
}) {
  return (
    <div className="rounded-xl border border-border bg-card p-4">
      <div className="text-sm text-muted-foreground">{label}</div>
      <div className="mt-1 text-2xl font-semibold text-foreground">{value}</div>
      {helper ? <div className="mt-1 text-xs text-muted-foreground">{helper}</div> : null}
    </div>
  )
}

export function SeriesKpiGrid({ summary }: SeriesKpiGridProps) {
  return (
    <div className="mb-6 grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-6">
      <KpiCard label="Ropers únicos" value={summary.uniqueRopers} />
      <KpiCard label="Eventos cerrados" value={summary.closedEvents} />
      <KpiCard label="Corridas válidas" value={summary.validRuns} />
      <KpiCard label="Bolsa repartida" value={formatCurrency(summary.totalDistributed)} />
      <KpiCard
        label="Más rápido"
        value={summary.fastestRoperName ?? '—'}
        helper={formatSeconds(summary.fastestAvgTime)}
      />
      <KpiCard
        label="Más victorias"
        value={summary.mostWinsRoperName ?? '—'}
        helper={summary.mostWinsCount > 0 ? `${summary.mostWinsCount} victorias` : 'Sin victorias'}
      />
    </div>
  )
}
