import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '../ui/table'
import { formatCurrency, formatSeconds } from './formatters'
import type { SeriesRoperStatRow } from './types'

interface SeriesRopersTableProps {
  rows: SeriesRoperStatRow[]
  onSelectRoper: (roperId: number) => void
}

export function SeriesRopersTable({ rows, onSelectRoper }: SeriesRopersTableProps) {
  return (
    <div className="mb-6 rounded-xl border border-border bg-card p-4">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Rank</TableHead>
            <TableHead>Roper</TableHead>
            <TableHead>Specialty</TableHead>
            <TableHead>Eventos</TableHead>
            <TableHead>Parejas</TableHead>
            <TableHead>Corridas</TableHead>
            <TableHead>Tiempo promedio</TableHead>
            <TableHead>Mejor ronda</TableHead>
            <TableHead>Victorias</TableHead>
            <TableHead>Podios</TableHead>
            <TableHead>NT</TableHead>
            <TableHead>DQ</TableHead>
            <TableHead className="text-right">Ganancias</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.length > 0 ? (
            rows.map((row) => (
              <TableRow
                key={row.roperId}
                className="cursor-pointer"
                onClick={() => onSelectRoper(row.roperId)}
              >
                <TableCell className="font-medium">#{row.rank}</TableCell>
                <TableCell>{row.roperName}</TableCell>
                <TableCell className="capitalize">{row.specialty}</TableCell>
                <TableCell>{row.eventsPlayed}</TableCell>
                <TableCell>{row.partnersCount}</TableCell>
                <TableCell>{row.validRuns}</TableCell>
                <TableCell>{formatSeconds(row.avgTime)}</TableCell>
                <TableCell>{formatSeconds(row.bestRun)}</TableCell>
                <TableCell>{row.wins}</TableCell>
                <TableCell>{row.podiums}</TableCell>
                <TableCell>{row.ntCount}</TableCell>
                <TableCell>{row.dqCount}</TableCell>
                <TableCell className="text-right font-medium text-orange-700">
                  {formatCurrency(row.earnings)}
                </TableCell>
              </TableRow>
            ))
          ) : (
            <TableRow>
              <TableCell colSpan={13} className="py-8 text-center text-muted-foreground">
                No hay ropers para mostrar con los filtros actuales
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}
