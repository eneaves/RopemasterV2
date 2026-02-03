import { useState } from 'react'
import { Edit, Lock, Plus, MapPin, Calendar, Layers, Trophy } from 'lucide-react'
import { Button } from './ui/button'
import { Badge } from './ui/badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from './ui/tabs'
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from './ui/breadcrumb'
import { EventMetricsCard } from './EventMetricsCard'
import { TeamsTab } from './TeamsTab'
import { DrawTab } from './DrawTab'
import { CaptureRunsTab } from './CaptureRunsTab'
import { StandingsTab } from './StandingsTab'
import { PayoffsTab } from './PayoffsTab'
import { ExportTab } from './ExportTab'
import { EventCaptureView } from './EventCaptureView'
import { toast } from 'sonner'
import { NewEventModal } from './NewEventModal'
import { updateEvent } from '@/lib/api'

import type { Event, Series } from '../types'

interface EventDetailsProps {
  event: Event
  series: Series
  onBack: () => void
  // Permite abrir el modal de nuevo evento desde esta vista
  onCreateEvent?: () => void
  // Permite controlar la pestaña inicial desde el llamador
  initialTab?: string
  // Callback para notificar cuando el evento debe actualizarse
  onEventUpdated?: () => void
}

export function EventDetails({ event, series, onBack, onCreateEvent, initialTab, onEventUpdated }: EventDetailsProps) {
  const [activeTab, setActiveTab] = useState(initialTab ?? 'teams')
  const [isLocked, setIsLocked] = useState(event.status === 'locked')
  const [payoffsFinalized, setPayoffsFinalized] = useState(false)
  const [showCaptureView, setShowCaptureView] = useState(false)
  const [isEditOpen, setIsEditOpen] = useState(false)

  const handleEditEvent = () => setIsEditOpen(true)

  const getStatusBadge = () => {
    if (payoffsFinalized) {
      return <Badge className="bg-violet-50 text-violet-700 border-violet-200">Payoffs Finalized</Badge>
    }
    if (event?.status === 'draft') {
      return <Badge className="bg-muted text-muted-foreground border-border hover:bg-muted">Draft</Badge>
    }
    if (isLocked || event?.status === 'locked') {
      return (
        <Badge className="bg-accent text-primary border-accent">
          <Lock className="mr-1 h-3 w-3" /> Locked
        </Badge>
      )
    }
    return <Badge className="bg-emerald-50 text-emerald-700 border-emerald-200">Active</Badge>
  }

  if (showCaptureView) {
    return <EventCaptureView event={event} series={series} onBack={() => setShowCaptureView(false)} />
  }

  return (
    <div className="flex-1 bg-background h-full overflow-y-auto">
      <div className="p-6 lg:p-8">
        {/* Breadcrumb */}
        <Breadcrumb className="mb-6">
          <BreadcrumbList>
            <BreadcrumbItem>
              <BreadcrumbLink onClick={onBack} className="cursor-pointer text-muted-foreground hover:text-foreground">
                Dashboard
              </BreadcrumbLink>
            </BreadcrumbItem>
            <BreadcrumbSeparator />
            <BreadcrumbItem>
              <BreadcrumbLink onClick={onBack} className="cursor-pointer text-muted-foreground hover:text-foreground">
                Series
              </BreadcrumbLink>
            </BreadcrumbItem>
            <BreadcrumbSeparator />
            <BreadcrumbItem>
              <BreadcrumbLink onClick={onBack} className="cursor-pointer text-muted-foreground hover:text-foreground">
                {series.name}
              </BreadcrumbLink>
            </BreadcrumbItem>
            <BreadcrumbSeparator />
            <BreadcrumbItem>
              <BreadcrumbPage className="text-foreground">{event.name}</BreadcrumbPage>
            </BreadcrumbItem>
          </BreadcrumbList>
        </Breadcrumb>

        {/* Header */}
        <div className="mb-8">
          <div className="flex flex-col md:flex-row justify-between items-start gap-4 mb-4">
            <div className="space-y-1">
              <div className="flex items-center gap-2 mb-2">
                 <Badge variant="secondary" className="bg-primary/10 text-primary hover:bg-primary/20 border-primary/10 transition-colors">
                    <Trophy className="w-3 h-3 mr-1" />
                    {series.name}
                 </Badge>
                 <span className="text-muted-foreground text-xs font-medium">Temporada {series.season}</span>
              </div>
              
              <h1 className="text-3xl md:text-4xl font-bold text-foreground tracking-tight">
                {event.name}
              </h1>
              
              <div className="flex flex-wrap items-center gap-4 text-sm text-muted-foreground mt-2">
                  <div className="flex items-center gap-1.5 bg-muted/50 px-2 py-1 rounded-md">
                     <Layers className="w-4 h-4 text-foreground/70" />
                     <span className="font-medium text-foreground/80">{event.rounds} Rondas</span>
                  </div>
                  
                  {event.location && (
                    <div className="flex items-center gap-1.5">
                        <MapPin className="w-4 h-4" />
                        <span>{event.location}</span>
                    </div>
                  )}
                  
                  <div className="flex items-center gap-1.5">
                    <Calendar className="w-4 h-4" />
                    <span>{new Date(event.date + 'T00:00:00').toLocaleDateString(undefined, {
                        year: 'numeric',
                        month: 'long',
                        day: 'numeric'
                    })}</span>
                  </div>
              </div>
            </div>

            <div className="flex gap-3 items-center flex-wrap">
              {getStatusBadge()}
              {onCreateEvent && (
                <Button onClick={onCreateEvent} className="bg-primary text-primary-foreground rounded-xl shadow-sm hover:opacity-90">
                  <Plus className="mr-2 h-4 w-4" /> Nuevo evento
                </Button>
              )}

              <Button onClick={handleEditEvent} variant="outline" className="border-border text-foreground hover:bg-background rounded-xl">
                <Edit className="h-4 w-4 mr-2" /> Edit Event
              </Button>
            </div>
          </div>
        </div>

        <EventMetricsCard event={event} isLocked={isLocked} />

        <Tabs value={activeTab} onValueChange={setActiveTab} className="w-full flex flex-col gap-6">
          <div className="flex items-center overflow-x-auto pb-2 -mx-6 px-6 lg:mx-0 lg:px-0 lg:pb-0 scrollbar-hide">
              <TabsList className="bg-background border border-border/60 shadow-sm p-1.5 rounded-full h-auto inline-flex items-center gap-1 min-w-max">
                {[
                  { val: 'teams', label: 'Equipos' },
                  { val: 'draw', label: 'Sorteo' },
                  { val: 'capture', label: 'Captura' },
                  { val: 'standings', label: 'Resultados' },
                  { val: 'payoffs', label: 'Pagos' },
                  { val: 'export', label: 'Exportar' },
                ].map((t) => (
                  <TabsTrigger
                    key={t.val}
                    value={t.val}
                    className="
                      rounded-full px-5 py-2.5 text-sm font-medium transition-all
                      data-[state=active]:bg-primary data-[state=active]:text-primary-foreground data-[state=active]:shadow-md
                      text-muted-foreground hover:text-foreground hover:bg-muted/50
                    "
                  >
                    {t.label}
                  </TabsTrigger>
                ))}
            </TabsList>
          </div>

          <div className="bg-card rounded-xl border border-border shadow-sm overflow-hidden flex-1 min-h-[500px]">
            <div className="p-6 h-full">
              <TabsContent value="teams" className="mt-0 h-full">
                <TeamsTab event={event} isLocked={isLocked} onTeamsUpdated={onEventUpdated} />
              </TabsContent>

              <TabsContent value="draw" className="mt-0 h-full">
                <DrawTab event={event} isLocked={isLocked} />
              </TabsContent>

              <TabsContent value="capture" className="mt-0 h-full">
                <CaptureRunsTab event={event} isLocked={isLocked} onLock={() => setIsLocked(true)} />
              </TabsContent>

              <TabsContent value="standings" className="mt-0 h-full">
                <StandingsTab event={event} />
              </TabsContent>

              <TabsContent value="payoffs" className="mt-0 h-full">
                <PayoffsTab event={event} onFinalize={() => setPayoffsFinalized(true)} isFinalized={payoffsFinalized} />
              </TabsContent>

              <TabsContent value="export" className="mt-0 h-full">
                <ExportTab event={event} series={series} />
              </TabsContent>
            </div>
          </div>
        </Tabs>
      </div>

      {/* Edit Event Modal */}
      <NewEventModal
        isOpen={isEditOpen}
        onClose={() => setIsEditOpen(false)}
        initialEvent={event}
        seriesId={series.id}
        onUpdateEvent={async (id: string, patch: any) => {
          try {
            await updateEvent(Number(id), patch)
            toast.success('Evento actualizado')
            if (patch.status) setIsLocked(patch.status === 'locked')
          } catch (err: any) {
            toast.error(err?.toString?.() ?? 'No se pudo actualizar el evento')
          } finally {
            setIsEditOpen(false)
          }
        }}
      />
    </div>
  )
}
