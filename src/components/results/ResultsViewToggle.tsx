import { cn } from '../../lib/utils'
import type { ResultsView } from './types'

interface ResultsViewToggleProps {
  value: ResultsView
  onChange: (value: ResultsView) => void
}

const items: Array<{ value: ResultsView; label: string }> = [
  { value: 'event', label: 'Evento' },
  { value: 'seriesRopers', label: 'Serie / Ropers' },
]

export function ResultsViewToggle({ value, onChange }: ResultsViewToggleProps) {
  return (
    <div className="inline-flex h-10 items-center rounded-xl border border-border bg-card p-1">
      {items.map((item) => (
        <button
          key={item.value}
          type="button"
          onClick={() => onChange(item.value)}
          className={cn(
            'inline-flex h-8 items-center justify-center rounded-lg px-3 text-sm font-medium transition-colors',
            value === item.value
              ? 'bg-orange-50 text-orange-700 shadow-sm'
              : 'text-muted-foreground hover:text-foreground',
          )}
        >
          {item.label}
        </button>
      ))}
    </div>
  )
}
