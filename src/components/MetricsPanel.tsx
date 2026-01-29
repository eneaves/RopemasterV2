import { Trophy, Target, Users, DollarSign, Calendar } from 'lucide-react'
import { Progress } from './ui/progress'
import { Card } from './ui/card'
import type { DashboardStats } from '@/types'

interface MetricsPanelProps {
  stats?: DashboardStats
}

export function MetricsPanel({ stats }: MetricsPanelProps) {
  if (!stats) {
      return (
        <aside className="w-80 bg-card border-l border-border p-6 overflow-y-auto">
            <h3 className="text-foreground mb-6">Resumen</h3>
            <p className="text-muted-foreground">Cargando métricas...</p>
        </aside>
      )
  }

  const {
      total_series, active_series,
      total_events, active_events,
      completed_events, upcoming_events,
      total_teams, total_pot,
      upcoming_events_30d,
      global_progress
  } = stats

  const completedPercentage = Math.round(global_progress || 0)

  return (
    <aside className="w-80 bg-card border-l border-border p-6 overflow-y-auto">
      <h3 className="text-foreground mb-6">Resumen</h3>

      {/* Metrics Cards */}
      <div className="space-y-4 mb-8">
        <Card className="p-4 bg-[#FFF4E6] border-orange-100">
          <div className="flex items-center justify-between mb-2">
            <span className="text-muted-foreground">Total Series</span>
            <Trophy className="w-5 h-5 text-[#FF7A00]" />
          </div>
          <p className="text-2xl text-[#FF7A00]">{total_series}</p>
          <p className="text-muted-foreground mt-1">{active_series} activas</p>
        </Card>

        <Card className="p-4 bg-blue-50 border-blue-100">
          <div className="flex items-center justify-between mb-2">
            <span className="text-muted-foreground">Total Events</span>
            <Target className="w-5 h-5 text-blue-600" />
          </div>
          <p className="text-2xl text-blue-600">{total_events}</p>
          <p className="text-muted-foreground mt-1">{active_events} en curso</p>
        </Card>

        <Card className="p-4 bg-green-50 border-green-100">
          <div className="flex items-center justify-between mb-2">
            <span className="text-muted-foreground">Registered Teams</span>
            <Users className="w-5 h-5 text-green-600" />
          </div>
          <p className="text-2xl text-green-600">{total_teams}</p>
          <p className="text-muted-foreground mt-1">En todos los eventos</p>
        </Card>

        <Card className="p-4 bg-purple-50 border-purple-100">
          <div className="flex items-center justify-between mb-2">
            <span className="text-muted-foreground">Total Pot</span>
            <DollarSign className="w-5 h-5 text-purple-600" />
          </div>
          <p className="text-2xl text-purple-600">
            {new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', maximumFractionDigits: 0 }).format(total_pot)}
          </p>
          <p className="text-muted-foreground mt-1">Eventos activos</p>
        </Card>

        <Card className="p-4 bg-yellow-50 border-yellow-100">
          <div className="flex items-center justify-between mb-2">
            <span className="text-muted-foreground">Upcoming Events</span>
            <Calendar className="w-5 h-5 text-yellow-600" />
          </div>
          <p className="text-2xl text-yellow-600">{upcoming_events_30d}</p>
          <p className="text-muted-foreground mt-1">Próximos 30 días</p>
        </Card>
      </div>

      {/* Progress Chart */}
      <div className="mb-8">
        <h4 className="text-foreground mb-4">Progreso de Eventos</h4>
        <div className="space-y-4">
          <div>
            <div className="flex justify-between mb-2">
              <span className="text-muted-foreground">Completados</span>
              <span className="text-foreground">{completed_events}</span>
            </div>
            <Progress value={(completed_events / (total_events || 1)) * 100 || 0} className="h-2 bg-gray-100" indicatorClassName="bg-green-500 rounded-full" />
          </div>

          <div>
            <div className="flex justify-between mb-2">
              <span className="text-muted-foreground">En Curso</span>
              <span className="text-foreground">{active_events}</span>
            </div>
            <Progress value={(active_events / (total_events || 1)) * 100 || 0} className="h-2 bg-gray-100" indicatorClassName="bg-[#FF7A00] rounded-full" />
          </div>

          <div>
            <div className="flex justify-between mb-2">
              <span className="text-muted-foreground">Próximos</span>
              <span className="text-foreground">{upcoming_events}</span>
            </div>
            <Progress value={(upcoming_events / (total_events || 1)) * 100 || 0} className="h-2 bg-gray-100" indicatorClassName="bg-gray-400 rounded-full" />
          </div>
        </div>

        <div className="mt-6 p-4 bg-card rounded-lg border border-border">
          <div className="flex justify-between items-center">
            <span className="text-muted-foreground">Total Completado</span>
            <span className="text-2xl text-foreground">{completedPercentage}%</span>
          </div>
        </div>
      </div>
    </aside>
  )
}
