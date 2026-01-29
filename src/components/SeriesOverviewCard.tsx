import { Trophy, Target, Users, DollarSign, Percent, Calendar } from 'lucide-react'
import { Card } from './ui/card'
import type { Series } from '../types'

interface SeriesOverviewCardProps {
  series: Series
  events: any[]
}

export function SeriesOverviewCard({ series, events }: SeriesOverviewCardProps) {
  const totalEvents = events.length
  const totalTeams = events.reduce((sum, e) => sum + (e.teamsCount || 0), 0)
  const totalRopers = totalTeams * 2
  const totalPot = events.reduce((sum, e) => sum + (e.pot || 0), 0)
  const averageSpacing = events.length > 0
    ? Math.round(events.reduce((sum, e) => sum + ((e.rounds || 0) * 3.5), 0) / events.length)
    : 0
  const lastUpdated = events.length > 0
    ? new Date(Math.max(...events.map((e: any) => new Date(e.date).getTime()))).toLocaleDateString('es-ES', { year: 'numeric', month: 'short', day: 'numeric' })
    : 'N/A'

  const metrics = [
    { label: 'Total Events', value: totalEvents, icon: Trophy, color: 'text-primary bg-accent' },
    { label: 'Total Ropers', value: totalRopers, icon: Users, color: 'text-foreground/80 bg-muted' },
    { label: 'Teams Created', value: totalTeams, icon: Target, color: 'text-foreground/80 bg-muted' },
    { label: 'Total Pot', value: `$${totalPot.toLocaleString()}`, icon: DollarSign, color: 'text-foreground/80 bg-muted' },
    { label: 'Average Spacing', value: `${averageSpacing}%`, icon: Percent, color: 'text-foreground/80 bg-muted' },
    { label: 'Last Updated', value: lastUpdated, icon: Calendar, color: 'text-foreground/80 bg-muted' },
  ]

  return (
    <Card className="mb-6">
      <h3 className="text-foreground mb-6">Series Overview</h3>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {metrics.map((metric, index) => {
          const Icon = metric.icon
          return (
            <div key={index} className="flex items-start gap-4 p-4 bg-muted rounded-lg border border-border shadow-sm">
              <div className={`p-2.5 rounded-lg ${metric.color}`}>
                <Icon className="w-5 h-5" />
              </div>
              <div>
                <p className="text-muted-foreground mb-1">{metric.label}</p>
                <p className="text-2xl text-foreground">{metric.value}</p>
              </div>
            </div>
          )
        })}
      </div>

      {series.description && (
        <div className="mt-6 pt-6 border-t border-border">
          <p className="text-muted-foreground">{series.description}</p>
        </div>
      )}
    </Card>
  )
}
