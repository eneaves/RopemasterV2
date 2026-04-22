import { Users, DollarSign, Gauge, CreditCard } from 'lucide-react'
import type { Event } from '../types'

function formatCurrency(n: number | undefined | null) {
  if (n === undefined || n === null) return '—'
  return new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', maximumFractionDigits: 0 }).format(n)
}

interface EventMetricsCardProps {
  event: Event
  isLocked?: boolean
}

export function EventMetricsCard({ event }: EventMetricsCardProps) {
  const totalTeams = Number(event.teamsCount ?? event.teams_count ?? 0)
  const entryFeePerRoper = Number(event.entryFee ?? event.entry_fee ?? 0) || null
  const potValue = Number(event.pot ?? event.total_pot ?? 0)

  const projectedPot =
    potValue === 0 && entryFeePerRoper && totalTeams > 0
      ? entryFeePerRoper * totalTeams * 2
      : potValue

  return (
    <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-3">
      {/* Total Teams */}
      <div className="bg-card border border-border rounded-xl p-5 shadow-sm flex flex-col justify-between hover:border-primary/50 transition-colors group">
        <div className="flex justify-between items-start mb-2">
            <span className="text-sm font-medium text-muted-foreground group-hover:text-primary/80 transition-colors">Total Equipos</span>
            <div className="bg-blue-50 text-blue-600 p-2 rounded-lg">
                <Users className="w-5 h-5" />
            </div>
        </div>
        <div>
            <span className="text-3xl font-bold text-foreground tracking-tight">{totalTeams}</span>
        </div>
      </div>

      {/* Pot / Prize Pool */}
      <div className="bg-card border border-border rounded-xl p-5 shadow-sm flex flex-col justify-between hover:border-emerald-500/50 transition-colors group">
         <div className="flex justify-between items-start mb-2">
            <span className="text-sm font-medium text-muted-foreground group-hover:text-emerald-600/80 transition-colors">Bolsa Acumulada</span>
            <div className="bg-emerald-50 text-emerald-600 p-2 rounded-lg">
                <DollarSign className="w-5 h-5" />
            </div>
        </div>
        <div>
            <span className="text-3xl font-bold text-foreground tracking-tight tabular-nums">
                {formatCurrency(projectedPot)}
            </span>
             {event.pot === 0 && event.teamsCount > 0 && (
                <span className="text-[10px] text-muted-foreground block mt-1 font-medium bg-muted/50 px-1.5 py-0.5 rounded w-fit">
                    Proyección (Est.)
                </span>
             )}
        </div>
      </div>

      {/* Entry Fee */}
      <div className="bg-card border border-border rounded-xl p-5 shadow-sm flex flex-col justify-between hover:border-primary/50 transition-colors group">
         <div className="flex justify-between items-start mb-2">
            <span className="text-sm font-medium text-muted-foreground group-hover:text-primary/80 transition-colors">Inscripción</span>
            <div className="bg-violet-50 text-violet-600 p-2 rounded-lg">
                <CreditCard className="w-5 h-5" />
            </div>
        </div>
        <div>
            <span className="text-3xl font-bold text-foreground tracking-tight tabular-nums">
                {formatCurrency(entryFeePerRoper)}
            </span>
            <span className="text-xs text-muted-foreground ml-1 font-medium">/ roper</span>
        </div>
      </div>

      {/* Rating Cap / Info */}
      <div className="bg-card border border-border rounded-xl p-5 shadow-sm flex flex-col justify-between hover:border-amber-500/50 transition-colors group">
         <div className="flex justify-between items-start mb-2">
            <span className="text-sm font-medium text-muted-foreground group-hover:text-amber-600/80 transition-colors">Clasificación</span>
            <div className="bg-amber-50 text-amber-600 p-2 rounded-lg">
                <Gauge className="w-5 h-5" />
            </div>
        </div>
        <div>
            <span className="text-3xl font-bold text-foreground tracking-tight">
                {event.maxTeamRating ? `#${event.maxTeamRating}` : 'Abierto'}
            </span>
            <span className="text-xs text-muted-foreground ml-2 font-medium">Cap</span>
        </div>
      </div>
    </div>
  )
}
