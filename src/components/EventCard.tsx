import {
  Calendar,
  Edit,
  Users,
  Trophy,
  DollarSign,
  Lock,
  MoreVertical,
  Eye,
  Shuffle,
  Video,
  BarChart3,
  Download,
  Copy,
  Trash2,
} from 'lucide-react'
import { Button } from './ui/button'
import { Badge } from './ui/badge'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from './ui/dropdown-menu'

interface EventCardProps {
  event: any
  onViewTeams: () => void
  onGenerateDraw: () => void
  onRecordRuns: () => void
  onViewStandings: () => void
  onComputePayoffs: () => void
  onExport: () => void
  onEdit?: () => void
  onDuplicate: () => void
  onDelete: () => void
}

export function EventCard({
  event,
  onViewTeams,
  onGenerateDraw,
  onRecordRuns,
  onViewStandings,
  onComputePayoffs,
  onExport,
  onDuplicate,
  onDelete,
  onEdit,
}: EventCardProps) {
  const getStatusBadge = (status: string) => {
    switch (status) {
      case 'active':
        return <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200 hover:bg-emerald-50">🟢 Active</Badge>
      case 'locked':
        return <Badge className="bg-accent text-primary border-accent hover:bg-accent"><Lock className="mr-1 size-3" />Locked</Badge>
      case 'draft':
        return <Badge className="bg-muted text-muted-foreground border-border hover:bg-muted">⚪ Draft</Badge>
      default:
        return null
    }
  }

  return (
    <div className="bg-card rounded-xl border border-border p-6 shadow-sm hover:shadow-md hover:border-primary/60 transition-all">
      {/* Header */}
      <div className="flex justify-between items-start mb-4">
        <div className="flex-1">
          <h3 className="text-foreground mb-2">{event.name}</h3>
          <div className="flex items-center gap-2 text-muted-foreground">
            <Calendar className="size-4" />
            <span>
              {new Date(event.date).toLocaleDateString('es-ES', {
                year: 'numeric',
                month: 'long',
                day: 'numeric',
              })}
            </span>
          </div>
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon" className="size-8 hover:bg-accent">
              <MoreVertical className="size-4 text-muted-foreground" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-48">
            <DropdownMenuItem onClick={onEdit} className="cursor-pointer">
              <Edit className="mr-2 size-4" />
              Editar evento
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={onDelete} className="cursor-pointer text-red-600 focus:text-red-600">
              <Trash2 className="mr-2 size-4" />
              Borrar evento
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {/* Status */}
      <div className="flex items-center gap-2 mb-4">
        {getStatusBadge(event.status)}
        {event.payoffStatus === 'finalized' && (
          <Badge className="bg-blue-50 text-blue-700 border-blue-200 hover:bg-blue-50">🏁 Payoffs Done</Badge>
        )}
      </div>

      {/* Metrics */}
      <div className="grid grid-cols-2 gap-4 mb-6">
        <div className="flex items-center gap-2">
          <div className="p-2 bg-emerald-50 rounded-xl">
            <Users className="size-4 text-emerald-600" />
          </div>
          <div>
            <p className="text-muted-foreground">Teams</p>
            <p className="text-foreground">{event.teamsCount}</p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <div className="p-2 bg-blue-50 rounded-xl">
            <Trophy className="size-4 text-blue-600" />
          </div>
          <div>
            <p className="text-muted-foreground">Rounds</p>
            <p className="text-foreground">{event.rounds}</p>
          </div>
        </div>

        <div className="flex items-center gap-2 col-span-2">
          <div className="p-2 bg-violet-50 rounded-xl">
            <DollarSign className="size-4 text-violet-600" />
          </div>
          <div>
            <p className="text-muted-foreground">Pot</p>
            <p className="text-foreground">${(event.pot || 0).toLocaleString()}</p>
          </div>
        </div>
      </div>

      {/* Action */}
      <Button onClick={onViewTeams} className="w-full bg-primary text-primary-foreground rounded-xl hover:opacity-90 h-11">
        <Eye className="mr-2 size-4" />
        Open Event
      </Button>
    </div>
  )
}
