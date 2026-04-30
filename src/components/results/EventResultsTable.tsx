import { Badge } from '../ui/badge'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '../ui/table'
import { formatCurrency, formatSeconds } from './formatters'
import type { EventStandingRow } from './types'

interface EventResultsTableProps {
  rows: EventStandingRow[]
  onSelectTeam: (teamId: number) => void
}

export function EventResultsTable({ rows, onSelectTeam }: EventResultsTableProps) {
  return (
    <div className="mb-6 rounded-xl border border-border bg-card p-4">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Pos</TableHead>
            <TableHead>Header</TableHead>
            <TableHead>Heeler</TableHead>
            <TableHead>Total Rondas</TableHead>
            <TableHead>Tiempo Total</TableHead>
            <TableHead>Promedio</TableHead>
            <TableHead>Best run</TableHead>
            <TableHead>NT</TableHead>
            <TableHead>DQ</TableHead>
            <TableHead>Estado</TableHead>
            <TableHead className="text-right">Payoff</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.length > 0 ? (
            rows.map((row) => (
              <TableRow
                key={row.teamId}
                className={`cursor-pointer ${row.rank === 1 ? 'bg-orange-50' : ''}`}
                onClick={() => onSelectTeam(row.teamId)}
              >
                <TableCell>
                  <span
                    className={`inline-flex items-center justify-center rounded-full px-2 py-1 text-sm ${
                      row.rank === 1
                        ? 'bg-orange-500 text-white'
                        : row.rank === 2
                          ? 'bg-gray-400 text-white'
                          : row.rank === 3
                            ? 'bg-amber-600 text-white'
                            : 'bg-muted text-muted-foreground'
                    }`}
                  >
                    #{row.rank}
                  </span>
                </TableCell>
                <TableCell>{row.headerName}</TableCell>
                <TableCell>{row.heelerName}</TableCell>
                <TableCell>{row.completedRuns}</TableCell>
                <TableCell>{formatSeconds(row.totalTime)}</TableCell>
                <TableCell>{formatSeconds(row.avgTime)}</TableCell>
                <TableCell>{formatSeconds(row.bestTime)}</TableCell>
                <TableCell>{row.ntCount}</TableCell>
                <TableCell>{row.dqCount}</TableCell>
                <TableCell>
                  {row.status === 'Calificado' && (
                    <Badge className="border-emerald-200 bg-emerald-50 text-emerald-700">
                      Calificado
                    </Badge>
                  )}
                  {row.status === 'No Time' && (
                    <Badge className="border-red-200 bg-red-50 text-red-700">No Time</Badge>
                  )}
                  {row.status === 'DQ' && (
                    <Badge className="border-red-200 bg-red-50 text-red-700">DQ</Badge>
                  )}
                </TableCell>
                <TableCell className="text-right font-medium text-orange-700">
                  {row.payoff ? formatCurrency(row.payoff) : '—'}
                </TableCell>
              </TableRow>
            ))
          ) : (
            <TableRow>
              <TableCell colSpan={11} className="py-8 text-center text-muted-foreground">
                No se encontraron resultados
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}
