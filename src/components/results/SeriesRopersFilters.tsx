import { Input } from '../ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../ui/select'
import type { SeriesRopersFiltersState } from './types'

interface SeriesRopersFiltersProps {
  value: SeriesRopersFiltersState
  onChange: (patch: Partial<SeriesRopersFiltersState>) => void
}

export function SeriesRopersFilters({ value, onChange }: SeriesRopersFiltersProps) {
  return (
    <div className="mb-4 rounded-xl border border-border bg-card p-4">
      <div className="flex flex-col gap-3 xl:flex-row xl:items-center">
        <Input
          placeholder="Buscar roper..."
          value={value.query}
          onChange={(event) => onChange({ query: event.target.value })}
          className="xl:max-w-sm"
        />
        <Select
          value={value.specialty}
          onValueChange={(specialty) =>
            onChange({ specialty: specialty as SeriesRopersFiltersState['specialty'] })
          }
        >
          <SelectTrigger className="w-[160px]">
            <SelectValue placeholder="Specialty" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="Todos">Todos</SelectItem>
            <SelectItem value="header">Header</SelectItem>
            <SelectItem value="heeler">Heeler</SelectItem>
            <SelectItem value="both">Both</SelectItem>
          </SelectContent>
        </Select>
        <Select value={value.minRuns} onValueChange={(minRuns) => onChange({ minRuns: minRuns as SeriesRopersFiltersState['minRuns'] })}>
          <SelectTrigger className="w-[160px]">
            <SelectValue placeholder="Mín. corridas" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="1">Mín. 1 corrida</SelectItem>
            <SelectItem value="3">Mín. 3 corridas</SelectItem>
            <SelectItem value="5">Mín. 5 corridas</SelectItem>
          </SelectContent>
        </Select>
        <Select value={value.scope} onValueChange={(scope) => onChange({ scope: scope as SeriesRopersFiltersState['scope'] })}>
          <SelectTrigger className="w-[170px]">
            <SelectValue placeholder="Vista" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="Todos">Todos</SelectItem>
            <SelectItem value="Top 10">Solo top 10</SelectItem>
            <SelectItem value="Con podio">Solo con podio</SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>
  )
}
