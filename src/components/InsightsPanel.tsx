import React, { useMemo, useEffect, useState } from 'react'
import { Target, CheckCircle, Settings, Lock, FileText, Info } from 'lucide-react'
import { Button } from './ui/button'
import { Progress } from './ui/progress'
import { getSeriesLogs } from '@/lib/api'
import type { SeriesLog } from '../types'

interface InsightsPanelProps {
  events: any[]
  seriesId?: number
}

interface ActivityItem {
  id: string
  message: string
  timestamp: string
  icon: React.ReactNode
  color: string
}

export function InsightsPanel({ events, seriesId }: InsightsPanelProps) {
  const [logs, setLogs] = useState<SeriesLog[]>([])

  useEffect(() => {
    if (seriesId) {
      getSeriesLogs(seriesId).then((data) => {
        setLogs(data)
      }).catch(console.error)
    }
  }, [seriesId])

  function normalizeStatus(s?: string | null) {
    const v = String(s ?? 'draft').toLowerCase()
    if (v === 'finalized') return 'completed'
    if (v === 'upcoming') return 'draft'
    return v
  }

  const {
    totalEvents,
    activeEvents,
    lockedEvents,
    draftEvents,
    activePercentage,
    lockedPercentage,
    draftPercentage,
  } = useMemo(() => {
  const norm = (e: any) => normalizeStatus(e?.status)

    const _totalEvents = events.length

    const _activeEvents = events.filter((e) => norm(e) === 'active').length
    const _lockedEvents = events.filter((e) => norm(e) === 'locked').length
    const _draftEvents = events.filter((e) => norm(e) === 'draft').length

    const pct = (n: number) => (_totalEvents ? Math.round((n * 100) / _totalEvents) : 0)

    return {
      totalEvents: _totalEvents,
      activeEvents: _activeEvents,
      lockedEvents: _lockedEvents,
      draftEvents: _draftEvents,
      activePercentage: pct(_activeEvents),
      lockedPercentage: pct(_lockedEvents),
      draftPercentage: pct(_draftEvents),
    }
  }, [events])

  const activities: ActivityItem[] = logs.length > 0 ? logs.slice(0, 10).map(log => {
    let icon = <Info className="size-4" />
    let color = 'text-foreground/80 bg-muted'

    if (log.action.includes('create')) {
        icon = <CheckCircle className="size-4" />
        color = 'text-emerald-600 bg-emerald-50'
    } else if (log.action.includes('update')) {
        icon = <Settings className="size-4" />
        color = 'text-blue-600 bg-blue-50'
    } else if (log.action.includes('delete')) {
        icon = <Lock className="size-4" />
        color = 'text-red-600 bg-red-50'
    }

    return {
        id: String(log.id),
        message: log.details,
        timestamp: new Date(log.timestamp).toLocaleString(),
        icon,
        color
    }
  }) : [
    { id: '1', message: 'No activity yet', timestamp: '', icon: <Info className="size-4" />, color: 'text-foreground/80 bg-muted' }
  ]

  return (
    <div className="w-80 bg-card border-l border-border p-6 overflow-y-auto">
  <h3 className="text-foreground mb-6">Resumen</h3>

      {/* Total Events */}
      <div className="mb-8">
        <div className="flex items-center justify-between mb-4">
          <span className="text-foreground">Total Events</span>
          <Target className="size-5 text-primary" />
        </div>
        <p className="text-4xl text-foreground mb-2">{totalEvents}</p>
        <p className="text-muted-foreground">
          {activeEvents} active • {lockedEvents} locked • {draftEvents} draft
        </p>
      </div>

      {/* Breakdown */}
      <div className="mb-8">
        <h4 className="text-foreground mb-4">Status Breakdown</h4>

        <div className="space-y-4">
          <div>
            <div className="flex justify-between mb-2">
              <span className="text-muted-foreground">Active</span>
              <span className="text-foreground">{activeEvents}</span>
            </div>
            <Progress value={activePercentage} className="h-2 bg-gray-100" indicatorClassName="bg-emerald-500 rounded-full" />
          </div>

          <div>
            <div className="flex justify-between mb-2">
              <span className="text-muted-foreground">Locked</span>
              <span className="text-foreground">{lockedEvents}</span>
            </div>
            <Progress value={lockedPercentage} className="h-2 bg-gray-100" indicatorClassName="bg-[#FF7A00] rounded-full" />
          </div>

          <div>
            <div className="flex justify-between mb-2">
              <span className="text-muted-foreground">Draft</span>
              <span className="text-foreground">{draftEvents}</span>
            </div>
            <Progress value={draftPercentage} className="h-2 bg-gray-100" indicatorClassName="bg-gray-400 rounded-full" />
          </div>
        </div>

        {/* Pie Chart placeholder */}
        <div className="mt-6 p-6 bg-muted rounded-xl border border-border shadow-sm">
          <div className="flex items-center justify-center">
              <div className="relative w-32 h-32 flex items-center justify-center">
              <svg viewBox="0 0 100 100" className="w-full h-full -rotate-90">
                {/* Draft */}
                <circle cx="50" cy="50" r="40" fill="none" stroke="currentColor"
                  className="text-muted-foreground/40"
                  strokeWidth="20"
                  strokeLinecap="round"
                  strokeDasharray={`${draftPercentage * 2.51} ${251 - draftPercentage * 2.51}`}
                  strokeDashoffset="0" />
                {/* Active */}
                <circle cx="50" cy="50" r="40" fill="none" stroke="currentColor"
                  className="text-emerald-500"
                  strokeWidth="20"
                  strokeLinecap="round"
                  strokeDasharray={`${activePercentage * 2.51} ${251 - activePercentage * 2.51}`}
                  strokeDashoffset={`${-draftPercentage * 2.51}`} />
                {/* Locked */}
                <circle cx="50" cy="50" r="40" fill="none" stroke="currentColor"
                  className="text-primary"
                  strokeWidth="20"
                  strokeLinecap="round"
                  strokeDasharray={`${lockedPercentage * 2.51} ${251 - lockedPercentage * 2.51}`}
                  strokeDashoffset={`${-(draftPercentage + activePercentage) * 2.51}`} />
              </svg>
              <div className="absolute inset-0 flex flex-col items-center justify-center text-center">
                <p className="text-2xl text-foreground leading-none">{totalEvents}</p>
                <p className="text-muted-foreground text-sm">events</p>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Activity */}
      <div className="mb-6">
        <h4 className="text-foreground mb-4">Activity Feed</h4>
          <div className="space-y-3">
          {activities.map((a) => (
            <div key={a.id} className="flex items-start gap-3 pb-3 border-b border-border last:border-0 last:pb-0">
              <div className={`p-2 rounded-xl flex-shrink-0 ${a.color}`}>{a.icon}</div>
              <div className="flex-1 min-w-0">
                <p className="text-foreground">{a.message}</p>
                <p className="text-muted-foreground mt-0.5">{a.timestamp}</p>
              </div>
            </div>
          ))}
        </div>

        <Button variant="ghost" className="w-full mt-4 text-primary hover:bg-accent">
          <FileText className="mr-2 size-4" />
          View Logs
        </Button>
      </div>
    </div>
  )
}
