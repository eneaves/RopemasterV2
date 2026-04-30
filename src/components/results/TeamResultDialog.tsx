import { useEffect, useMemo, useState } from 'react'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '../ui/table'
import { getRunsExpanded } from '../../lib/api'
import { formatCurrency, formatSeconds } from './formatters'
import type { EventStandingRow, TeamRoundRow } from './types'

interface TeamResultDialogProps {
  open: boolean
  eventId: number | null
  team: EventStandingRow | null
  onOpenChange: (open: boolean) => void
}

export function TeamResultDialog({
  open,
  eventId,
  team,
  onOpenChange,
}: TeamResultDialogProps) {
  const [rounds, setRounds] = useState<TeamRoundRow[]>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!open || !eventId || !team) return
    setLoading(true)
    getRunsExpanded(eventId)
      .then((rows) => {
        const teamRows = rows
          .filter((row: any) => Number(row.team_id) === team.teamId)
          .map(
            (row: any): TeamRoundRow => ({
              round: Number(row.round),
              position: Number(row.position),
              timeSec: row.time_sec,
              penalty: Number(row.penalty ?? 0),
              totalSec: row.total_sec,
              noTime: row.no_time === 1,
              dq: row.dq === 1,
              status: row.status,
            }),
          )
        setRounds(teamRows)
      })
      .catch(() => setRounds([]))
      .finally(() => setLoading(false))
  }, [open, eventId, team])

  const attempts = useMemo(
    () => rounds.length || (team ? team.completedRuns + team.ntCount + team.dqCount : 0),
    [rounds, team],
  )

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="left-auto right-0 top-0 h-screen max-w-[440px] translate-x-0 translate-y-0 overflow-y-auto rounded-none border-l p-0 sm:max-w-[440px]">
        <div className="p-6">
          <DialogHeader>
            <DialogTitle>Detalle del Equipo</DialogTitle>
            <DialogDescription>
              {team ? `${team.headerName} & ${team.heelerName}` : 'Sin equipo seleccionado'}
            </DialogDescription>
          </DialogHeader>

          {team ? (
            <div className="mt-6 space-y-6">
              <div className="grid grid-cols-2 gap-3">
                <div className="rounded-xl border border-border bg-card p-4">
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Posición</div>
                  <div className="mt-1 text-2xl font-semibold">#{team.rank}</div>
                </div>
                <div className="rounded-xl border border-border bg-card p-4">
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Payoff</div>
                  <div className="mt-1 text-2xl font-semibold text-orange-700">
                    {formatCurrency(team.payoff)}
                  </div>
                </div>
                <div className="rounded-xl border border-border bg-card p-4">
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Tiempo total</div>
                  <div className="mt-1 text-2xl font-semibold">{formatSeconds(team.totalTime)}</div>
                </div>
                <div className="rounded-xl border border-border bg-card p-4">
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Promedio</div>
                  <div className="mt-1 text-2xl font-semibold">{formatSeconds(team.avgTime)}</div>
                </div>
              </div>

              <div className="rounded-xl border border-border bg-card p-4">
                <div className="mb-4 flex items-center justify-between">
                  <div>
                    <h3 className="font-medium text-foreground">Rondas</h3>
                    <p className="text-sm text-muted-foreground">
                      {attempts} intentos registrados
                    </p>
                  </div>
                  {loading ? <span className="text-sm text-muted-foreground">Cargando...</span> : null}
                </div>

                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Ronda</TableHead>
                      <TableHead>Base</TableHead>
                      <TableHead>Penalty</TableHead>
                      <TableHead>Total</TableHead>
                      <TableHead>Estado</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {rounds.length > 0 ? (
                      rounds.map((round) => (
                        <TableRow key={`${round.round}-${round.position}`}>
                          <TableCell>#{round.round}</TableCell>
                          <TableCell>{formatSeconds(round.timeSec)}</TableCell>
                          <TableCell>{round.penalty.toFixed(2)}s</TableCell>
                          <TableCell>{formatSeconds(round.totalSec)}</TableCell>
                          <TableCell>
                            {round.dq ? 'DQ' : round.noTime ? 'No Time' : round.status}
                          </TableCell>
                        </TableRow>
                      ))
                    ) : (
                      <TableRow>
                        <TableCell colSpan={5} className="py-8 text-center text-muted-foreground">
                          Sin detalle por ronda disponible
                        </TableCell>
                      </TableRow>
                    )}
                  </TableBody>
                </Table>
              </div>
            </div>
          ) : null}
        </div>
      </DialogContent>
    </Dialog>
  )
}
