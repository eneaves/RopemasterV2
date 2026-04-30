import { Card } from '../ui/card'
import { Calendar, DollarSign, Target, Trophy, Users } from 'lucide-react'
import { formatCurrency, formatPercent, formatSeconds } from './formatters'
import type { SeriesSummaryPanelData } from './types'

interface SeriesSummaryPanelProps {
  summary: SeriesSummaryPanelData | null
  onSelectRoper?: (roperId: number) => void
}

export function SeriesSummaryPanel({
  summary,
  onSelectRoper,
}: SeriesSummaryPanelProps) {
  if (!summary) {
    return (
      <aside className="w-80 overflow-y-auto border-l border-border bg-card p-6">
        <h3 className="mb-6 text-foreground">Resumen de la Serie</h3>
        <p className="text-muted-foreground">Cargando métricas...</p>
      </aside>
    )
  }

  return (
    <aside className="w-80 overflow-y-auto border-l border-border bg-card p-6">
      <h3 className="mb-6 text-foreground">Resumen de la Serie</h3>

      <div className="mb-8 space-y-4">
        <Card className="border-orange-100 bg-[#FFF4E6] p-4">
          <div className="mb-2 flex items-center justify-between">
            <span className="text-muted-foreground">Eventos cerrados</span>
            <Calendar className="h-5 w-5 text-[#FF7A00]" />
          </div>
          <p className="text-2xl text-[#FF7A00]">{summary.closedEvents}</p>
          <p className="mt-1 text-muted-foreground">Completed y locked</p>
        </Card>

        <Card className="border-blue-100 bg-blue-50 p-4">
          <div className="mb-2 flex items-center justify-between">
            <span className="text-muted-foreground">Ropers únicos</span>
            <Users className="h-5 w-5 text-blue-600" />
          </div>
          <p className="text-2xl text-blue-600">{summary.uniqueRopers}</p>
          <p className="mt-1 text-muted-foreground">En la serie visible</p>
        </Card>

        <Card className="border-green-100 bg-green-50 p-4">
          <div className="mb-2 flex items-center justify-between">
            <span className="text-muted-foreground">Equipos registrados</span>
            <Target className="h-5 w-5 text-green-600" />
          </div>
          <p className="text-2xl text-green-600">{summary.teamsRegistered}</p>
          <p className="mt-1 text-muted-foreground">Eventos cerrados</p>
        </Card>

        <Card className="border-purple-100 bg-purple-50 p-4">
          <div className="mb-2 flex items-center justify-between">
            <span className="text-muted-foreground">Bolsa repartida</span>
            <DollarSign className="h-5 w-5 text-purple-600" />
          </div>
          <p className="text-2xl text-purple-600">{formatCurrency(summary.totalDistributed)}</p>
          <p className="mt-1 text-muted-foreground">Payoff visible</p>
        </Card>

        <Card className="border-yellow-100 bg-yellow-50 p-4">
          <div className="mb-2 flex items-center justify-between">
            <span className="text-muted-foreground">Promedio serie</span>
            <Trophy className="h-5 w-5 text-yellow-600" />
          </div>
          <p className="text-2xl text-yellow-600">{formatSeconds(summary.avgSeriesTime)}</p>
          <p className="mt-1 text-muted-foreground">
            {formatPercent(summary.cleanRunRate)} corridas limpias
          </p>
        </Card>
      </div>

      <div className="mb-8">
        <h4 className="mb-4 text-foreground">Top 5 Ropers</h4>
        <div className="space-y-2">
          {summary.topRopers.length > 0 ? (
            summary.topRopers.map((roper, index) => (
              <button
                key={roper.roperId}
                type="button"
                className="flex w-full items-center justify-between rounded-lg border border-border bg-background px-3 py-2 text-left transition-colors hover:bg-muted/50"
                onClick={() => onSelectRoper?.(roper.roperId)}
              >
                <span className="text-sm text-foreground">
                  {index + 1}. {roper.name}
                </span>
                <span className="text-sm font-medium text-muted-foreground">
                  {formatSeconds(roper.avgTime)}
                </span>
              </button>
            ))
          ) : (
            <p className="text-sm text-muted-foreground">Sin ranking disponible</p>
          )}
        </div>
      </div>
    </aside>
  )
}
