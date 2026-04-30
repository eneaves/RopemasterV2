import { Input } from '../ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select'
import type { EventResultsFiltersState } from './types'

interface EventResultsFiltersProps {
  value: EventResultsFiltersState
  onChange: (patch: Partial<EventResultsFiltersState>) => void
}

export function EventResultsFilters({ value, onChange }: EventResultsFiltersProps) {
  return (
    <div className="mb-4 rounded-xl border border-border bg-card p-4">
      <div className="flex flex-col items-center gap-3 md:flex-row">
        <Input
          placeholder="Buscar equipo o roper..."
          value={value.query}
          onChange={(event) => onChange({ query: event.target.value })}
          className="max-w-sm"
        />
        <Select value={value.place} onValueChange={(place) => onChange({ place })}>
          <SelectTrigger className="w-[150px]">
            <SelectValue placeholder="Lugar" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="Todos">Todos</SelectItem>
            {[...Array(10)].map((_, index) => (
              <SelectItem key={index} value={String(index + 1)}>
                #{index + 1}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={value.status} onValueChange={(status) => onChange({ status: status as EventResultsFiltersState['status'] })}>
          <SelectTrigger className="w-[160px]">
            <SelectValue placeholder="Estado" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="Todos">Todos</SelectItem>
            <SelectItem value="Calificado">Calificado</SelectItem>
            <SelectItem value="No Time">No Time</SelectItem>
            <SelectItem value="DQ">DQ</SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>
  )
}
