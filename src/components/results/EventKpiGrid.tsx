import { formatCurrency, formatSeconds } from './formatters'
import type { EventResultsSummary } from './types'

interface EventKpiGridProps {
  summary: EventResultsSummary
}

function KpiCard({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="rounded-xl border border-border bg-card p-4 text-center">
      <div className="text-sm text-muted-foreground">{label}</div>
      <div className="text-2xl font-semibold">{value}</div>
    </div>
  )
}

export function EventKpiGrid({ summary }: EventKpiGridProps) {
  return (
    <div className="mb-6 grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-6">
      <KpiCard label="Equipos Calificados" value={summary.qualifiedTeams} />
      <KpiCard label="Corridas Limpias" value={summary.cleanRuns} />
      <KpiCard label="NT" value={summary.ntCount} />
      <KpiCard label="DQ" value={summary.dqCount} />
      <KpiCard label="Mejor Ronda" value={formatSeconds(summary.bestRunTime)} />
      <KpiCard label="Total Repartido" value={formatCurrency(summary.totalPayout)} />
    </div>
  )
}
