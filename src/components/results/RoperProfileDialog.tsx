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
import { formatCurrency, formatSeconds } from './formatters'
import type { SeriesRoperProfile } from './types'

interface RoperProfileDialogProps {
  open: boolean
  profile: SeriesRoperProfile | null
  onOpenChange: (open: boolean) => void
}

export function RoperProfileDialog({
  open,
  profile,
  onOpenChange,
}: RoperProfileDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="left-auto right-0 top-0 h-screen max-w-[460px] translate-x-0 translate-y-0 overflow-y-auto rounded-none border-l p-0 sm:max-w-[460px]">
        <div className="p-6">
          <DialogHeader>
            <DialogTitle>Perfil del Roper</DialogTitle>
            <DialogDescription>
              {profile ? `${profile.roperName} · Rank #${profile.rank}` : 'Cargando perfil...'}
            </DialogDescription>
          </DialogHeader>

          {profile ? (
            <div className="mt-6 space-y-6">
              <div className="rounded-xl border border-border bg-card p-4">
                <div className="text-xs uppercase tracking-wide text-muted-foreground">Promedio global</div>
                <div className="mt-1 text-3xl font-semibold">{formatSeconds(profile.avgTime)}</div>
                <div className="mt-1 text-sm capitalize text-muted-foreground">
                  Specialty: {profile.specialty}
                </div>
              </div>

              <div className="grid grid-cols-2 gap-3">
                <div className="rounded-xl border border-border bg-card p-4">
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Eventos</div>
                  <div className="mt-1 text-2xl font-semibold">{profile.eventsPlayed}</div>
                </div>
                <div className="rounded-xl border border-border bg-card p-4">
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Victorias</div>
                  <div className="mt-1 text-2xl font-semibold">{profile.wins}</div>
                </div>
                <div className="rounded-xl border border-border bg-card p-4">
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Podios</div>
                  <div className="mt-1 text-2xl font-semibold">{profile.podiums}</div>
                </div>
                <div className="rounded-xl border border-border bg-card p-4">
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Ganancias</div>
                  <div className="mt-1 text-2xl font-semibold text-orange-700">
                    {formatCurrency(profile.earnings)}
                  </div>
                </div>
              </div>

              <div className="rounded-xl border border-border bg-card p-4">
                <div className="mb-4">
                  <h3 className="font-medium text-foreground">Historial por evento</h3>
                  <p className="text-sm text-muted-foreground">Participaciones dentro de la serie seleccionada.</p>
                </div>
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Evento</TableHead>
                      <TableHead>Compañero</TableHead>
                      <TableHead>Puesto</TableHead>
                      <TableHead>Promedio</TableHead>
                      <TableHead className="text-right">Ganancia</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {profile.history.length > 0 ? (
                      profile.history.map((entry) => (
                        <TableRow key={`${entry.eventId}-${entry.partnerName}`}>
                          <TableCell>{entry.eventName}</TableCell>
                          <TableCell>{entry.partnerName}</TableCell>
                          <TableCell>{entry.finishRank ? `#${entry.finishRank}` : '—'}</TableCell>
                          <TableCell>{formatSeconds(entry.avgTime)}</TableCell>
                          <TableCell className="text-right font-medium text-orange-700">
                            {formatCurrency(entry.earnings)}
                          </TableCell>
                        </TableRow>
                      ))
                    ) : (
                      <TableRow>
                        <TableCell colSpan={5} className="py-8 text-center text-muted-foreground">
                          Sin historial disponible
                        </TableCell>
                      </TableRow>
                    )}
                  </TableBody>
                </Table>
              </div>

              <div className="grid grid-cols-1 gap-3">
                <div className="rounded-xl border border-border bg-card p-4">
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Mejor compañero</div>
                  <div className="mt-1 font-medium">{profile.bestPartnerName ?? '—'}</div>
                </div>
                <div className="rounded-xl border border-border bg-card p-4">
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Mejor evento</div>
                  <div className="mt-1 font-medium">{profile.bestEventName ?? '—'}</div>
                </div>
                <div className="rounded-xl border border-border bg-card p-4">
                  <div className="text-xs uppercase tracking-wide text-muted-foreground">Mejor ronda</div>
                  <div className="mt-1 font-medium">{formatSeconds(profile.bestRun)}</div>
                </div>
              </div>
            </div>
          ) : (
            <div className="mt-6 rounded-xl border border-border bg-card p-6 text-sm text-muted-foreground">
              Cargando métricas del roper...
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
