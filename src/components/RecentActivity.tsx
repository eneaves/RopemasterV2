import React from 'react'
import { CheckCircle2, Settings, Download, DollarSign, Trash2, Edit, Plus } from 'lucide-react'
import type { AuditLogItem } from '@/types'
import { formatDistanceToNow } from 'date-fns'
import { es } from 'date-fns/locale'

type Tone = 'green' | 'orange' | 'purple' | 'blue' | 'red'

const tones: Record<Tone, string> = {
  green: 'bg-emerald-50 text-emerald-700 border-emerald-200',
  orange: 'bg-amber-50 text-amber-700 border-amber-200',
  purple: 'bg-violet-50 text-violet-700 border-violet-200',
  blue: 'bg-blue-50 text-blue-700 border-blue-200',
  red: 'bg-red-50 text-red-700 border-red-200',
}

function mapActionToIconAndTone(action: string): { icon: React.ReactNode, tone: Tone, message: string } {
  const lower = action.toLowerCase()
  if (lower.includes('create')) return { icon: <Plus className="size-4" />, tone: 'green', message: 'Creado' }
  if (lower.includes('update')) return { icon: <Edit className="size-4" />, tone: 'blue', message: 'Actualizado' }
  if (lower.includes('delete')) return { icon: <Trash2 className="size-4" />, tone: 'red', message: 'Eliminado' }
  if (lower.includes('export')) return { icon: <Download className="size-4" />, tone: 'blue', message: 'Exportado' }
  if (lower.includes('lock')) return { icon: <Settings className="size-4" />, tone: 'orange', message: 'Bloqueado' }
  if (lower.includes('payoff')) return { icon: <DollarSign className="size-4" />, tone: 'purple', message: 'Payoffs' }
  
  return { icon: <CheckCircle2 className="size-4" />, tone: 'green', message: action }
}

export function RecentActivity({ items = [], onViewAll }: { items?: AuditLogItem[], onViewAll?: () => void }) {
  if (items.length === 0) {
    return (
        <div className="bg-card rounded-2xl border border-border p-6 shadow-sm">
            <h2 className="text-foreground mb-4">Actividad Reciente</h2>
            <p className="text-muted-foreground">No hay actividad reciente.</p>
        </div>
    )
  }

  return (
    <div className="bg-card rounded-2xl border border-border p-6 shadow-sm">
      <h2 className="text-foreground mb-4">Actividad Reciente</h2>

      <div className="space-y-4">
        {items.map((a) => {
            const { icon, tone, message } = mapActionToIconAndTone(a.action)
            const displayMessage = a.metadata ? `${message}: ${a.metadata}` : `${message} ${a.entity_type}`
            
            return (
            <div key={a.id} className="flex items-start gap-3 pb-4 border-b border-border last:border-0 last:pb-0">
                <div className={`p-2 rounded-lg border ${tones[tone]}`}>{icon}</div>
                <div className="flex-1">
                <p className="text-foreground">{displayMessage}</p>
                <p className="text-muted-foreground mt-1">
                    {formatDistanceToNow(new Date(a.created_at), { addSuffix: true, locale: es })}
                </p>
                </div>
            </div>
            )
        })}
      </div>

      <button 
        onClick={onViewAll}
        className="w-full mt-6 text-primary hover:opacity-90 transition-opacity"
      >
        Ver toda la actividad →
      </button>
    </div>
  )
}
