/* jsx-runtime */
import { Calendar, Trophy, MoreHorizontal, Edit, Copy, Trash } from 'lucide-react'
import { Button } from './ui/button'
import { Badge } from './ui/badge'
import { Progress } from './ui/progress'
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
} from './ui/dropdown-menu'
import type { Series } from '../types'

type Props = {
  series: Series
  onView?: () => void
  onEdit?: () => void
  onDuplicate?: () => void
  onDelete?: () => void
}

export function SeriesCard({ series, onView, onEdit, onDuplicate, onDelete }: Props) {
  const getStatusBadge = () => {
    switch (series.status) {
      case 'active':
        return <Badge className="bg-emerald-50 text-emerald-700 border-emerald-100">Active Series</Badge>
      case 'upcoming':
        return <Badge className="bg-orange-50 text-orange-700 border-orange-100">Upcoming</Badge>
      case 'archived':
        return <Badge className="bg-gray-100 text-gray-600 border-gray-200">Archived</Badge>
      default:
        return null
    }
  }

  return (
    <div className="bg-card rounded-xl border border-border p-6 shadow-sm hover:shadow-md transition-shadow duration-200">
      <div className="flex justify-between items-start mb-4">
        <div className="flex-1">
          <h3 className="text-foreground mb-2">{series.name}</h3>
          <div className="flex items-center gap-2 text-muted-foreground text-sm">
            <Calendar className="w-4 h-4" />
            <span>{series.season} • {series.dateRange}</span>
          </div>
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon" className="h-8 w-8">
              <MoreHorizontal className="h-4 w-4 text-muted-foreground" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-44">
            <DropdownMenuItem onClick={onEdit} className="cursor-pointer">
              <Edit className="mr-2 h-4 w-4" />
              Editar Serie
            </DropdownMenuItem>
            <DropdownMenuItem onClick={onDuplicate} className="cursor-pointer">
              <Copy className="mr-2 h-4 w-4" />
              Duplicar
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={onDelete} className="cursor-pointer text-red-600">
              <Trash className="mr-2 h-4 w-4" />
              Eliminar
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <div className="mb-4">{getStatusBadge()}</div>

      <div className="flex items-center gap-2 mb-4">
        <Trophy className="w-4 h-4 text-[#FF7A00]" />
        <span className="text-foreground">{series.eventsCount} {series.eventsCount === 1 ? 'evento' : 'eventos'}</span>
      </div>

      <div className="mb-4">
        <div className="flex justify-between text-muted-foreground mb-2">
          <span>Progreso</span>
          <span className="text-foreground">{Math.round(series.progress ?? 0)}%</span>
        </div>
        <Progress value={series.progress} className="h-2 bg-[rgba(255,122,0,0.12)]" indicatorClassName="bg-[#FF7A00] rounded-full" />
      </div>

      <Button onClick={onView} className="w-full bg-[#FFF4E6] text-[#FF7A00] hover:bg-[#FFE8CC] border-none rounded-lg">
        Ver Eventos
      </Button>
    </div>
  )
}
