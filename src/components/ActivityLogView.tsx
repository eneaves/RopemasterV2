import React, { useEffect, useState } from 'react'
import { ArrowLeft, CheckCircle2, Settings, Download, DollarSign, Trash2, Edit, Plus, Loader2 } from 'lucide-react'
import { Button } from './ui/button'
import { getRecentActivity } from '@/lib/api'
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

interface ActivityLogViewProps {
  onBack: () => void
}

export function ActivityLogView({ onBack }: ActivityLogViewProps) {
  const [items, setItems] = useState<AuditLogItem[]>([])
  const [loading, setLoading] = useState(true)
  const [offset, setOffset] = useState(0)
  const [hasMore, setHasMore] = useState(true)
  const LIMIT = 50

  const loadMore = async () => {
    setLoading(true)
    try {
      const newItems = await getRecentActivity(LIMIT, offset)
      if (newItems.length < LIMIT) {
        setHasMore(false)
      }
      setItems(prev => [...prev, ...newItems])
      setOffset(prev => prev + LIMIT)
    } catch (e) {
      console.error('Failed to load activity:', e)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadMore()
  }, [])

  return (
    <div className="flex-1 h-full flex flex-col bg-background overflow-hidden">
      <div className="p-6 border-b border-border flex items-center gap-4">
        <Button variant="ghost" size="icon" onClick={onBack}>
          <ArrowLeft className="size-5" />
        </Button>
        <div>
          <h1 className="text-2xl font-semibold text-foreground">Registro de Actividad</h1>
          <p className="text-sm text-muted-foreground">Historial completo de acciones en el sistema</p>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-6">
        <div className="max-w-3xl mx-auto space-y-4">
          {items.map((a) => {
            const { icon, tone, message } = mapActionToIconAndTone(a.action)
            const displayMessage = a.metadata ? `${message}: ${a.metadata}` : `${message} ${a.entity_type}`
            
            return (
              <div key={a.id} className="bg-card p-4 rounded-xl border border-border shadow-sm flex items-start gap-4">
                <div className={`p-2 rounded-lg border ${tones[tone]} shrink-0`}>{icon}</div>
                <div className="flex-1 min-w-0">
                  <div className="flex justify-between items-start">
                    <p className="font-medium text-foreground truncate">{displayMessage}</p>
                    <span className="text-xs text-muted-foreground whitespace-nowrap ml-2">
                      {new Date(a.created_at).toLocaleString()}
                    </span>
                  </div>
                  <p className="text-sm text-muted-foreground mt-1">
                    {formatDistanceToNow(new Date(a.created_at), { addSuffix: true, locale: es })}
                  </p>
                  <div className="mt-2 text-xs font-mono text-muted-foreground bg-muted/50 p-1 rounded w-fit">
                    ID: {a.id} • {a.action} • {a.entity_type} #{a.entity_id}
                  </div>
                </div>
              </div>
            )
          })}

          {loading && (
            <div className="flex justify-center py-8">
              <Loader2 className="size-8 animate-spin text-primary" />
            </div>
          )}

          {!loading && hasMore && (
            <div className="flex justify-center pt-4">
              <Button onClick={loadMore} variant="outline">
                Cargar más actividad
              </Button>
            </div>
          )}

          {!loading && items.length === 0 && (
            <div className="text-center py-12 text-muted-foreground">
              No hay actividad registrada.
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
