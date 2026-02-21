import { useState, useEffect, useMemo } from 'react'
import { Button } from './ui/button'
import { Badge } from './ui/badge'
import { Card } from './ui/card'
import { EventCaptureView } from './EventCaptureView'
import { Select, SelectTrigger, SelectContent, SelectItem, SelectValue } from './ui/select'
import { listTeams } from '../lib/api'
import type { Event, Series } from '../types'
import { useSeriesEvents } from '@/providers/SeriesEventsProvider'

export function CaptureManagement() {
  const { series: seriesList, getEventsForSeries } = useSeriesEvents()
  const [eventTeamCounts, setEventTeamCounts] = useState<Record<string, number>>({})
  const [selectedSeriesId, setSelectedSeriesId] = useState<string | null>(null)
  const [selectedEventId, setSelectedEventId] = useState<string | null>(null)
  
  const [showCaptureView, setShowCaptureView] = useState(false)

  const eventsList = useMemo(() => getEventsForSeries(selectedSeriesId), [getEventsForSeries, selectedSeriesId])

  useEffect(() => {
    if (!selectedSeriesId && seriesList.length > 0) {
      const active = seriesList.find((s: Series | any) => s.status === 'active')
      const fallback = active ?? seriesList[0]
      if (fallback) {
        setSelectedSeriesId(String(fallback.id))
      }
    }
  }, [seriesList, selectedSeriesId])

  useEffect(() => {
    if (!selectedEventId && eventsList.length > 0) {
      const active = eventsList.find((e: Event | any) => {
        const status = String(e.status ?? '').toLowerCase()
        return status === 'active' || status === 'completed'
      })
      const fallback = active ?? eventsList[0]
      if (fallback) {
        setSelectedEventId(String(fallback.id))
      }
      return
    }

    if (eventsList.length === 0 && selectedEventId) {
      setSelectedEventId(null)
    } else if (selectedEventId) {
      const exists = eventsList.some((event) => String(event.id) === selectedEventId)
      if (!exists) {
        setSelectedEventId(eventsList.length ? String(eventsList[0].id) : null)
      }
    }
  }, [eventsList, selectedEventId])

  useEffect(() => {
    if (!selectedEventId) return
    listTeams(parseInt(selectedEventId)).then((teams) => {
      setEventTeamCounts((prev) => ({
        ...prev,
        [selectedEventId]: teams.length,
      }))
    }).catch(() => {})
  }, [selectedEventId])

  const selectedSeries = seriesList.find(s => s.id.toString() === selectedSeriesId)
  const selectedEvent = eventsList.find(e => e.id.toString() === selectedEventId)
  const teamsCount = selectedEventId ? eventTeamCounts[selectedEventId] ?? selectedEvent?.teamsCount ?? selectedEvent?.teams_count ?? '—' : '—'

  if (showCaptureView && selectedEvent && selectedSeries) {
    return <EventCaptureView event={selectedEvent} series={selectedSeries} onBack={() => setShowCaptureView(false)} />
  }

  return (
    <div className="p-6 h-full bg-background">
      <h1 className="text-foreground mb-2">Captura de Tiempos</h1>
      <p className="text-muted-foreground mb-6">Selecciona una serie y un evento para comenzar a capturar tiempos en tiempo real</p>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        <div className="lg:col-span-2">
          <Card className="mb-6">
            <div className="flex items-start gap-4 mb-4">
              <div className="rounded-lg bg-primary text-primary-foreground p-3">
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" className="text-background"><path d="M12 7v5l3 3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/></svg>
              </div>
              <div>
                <h2 className="text-lg font-semibold">Iniciar Captura</h2>
                <p className="text-sm text-muted-foreground">Paso 1: Selecciona serie y evento</p>
              </div>
            </div>

            <div className="grid grid-cols-1 gap-4 mb-4">
              <div>
                <label className="block text-sm text-muted-foreground mb-2">Serie</label>
                <Select value={selectedSeriesId || ''} onValueChange={setSelectedSeriesId}>
                  <SelectTrigger className="w-full rounded-xl h-11 bg-card border-border">
                    <div className="flex items-center justify-between w-full">
                      <div className="flex items-center gap-2">
                        <SelectValue placeholder="Selecciona una serie" />
                        {selectedSeries && selectedSeries.status === 'active' && (
                          <Badge className="bg-emerald-50 text-emerald-700">Activa</Badge>
                        )}
                      </div>
                    </div>
                  </SelectTrigger>
                  <SelectContent>
                    {seriesList.map((s) => (
                      <SelectItem key={s.id} value={s.id.toString()}>
                        {s.name}
                        {s.status === 'active' && <span className="ml-2"> <Badge className="bg-emerald-50 text-emerald-700">Activa</Badge></span>}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <div>
                <label className="block text-sm text-muted-foreground mb-2">Evento</label>
                <Select 
                  value={selectedEventId || ''} 
                  onValueChange={setSelectedEventId}
                  disabled={!selectedSeriesId}
                >
                  <SelectTrigger className="w-full rounded-xl h-11 bg-card border-border">
                    <div className="flex items-center justify-between w-full">
                      <div className="flex items-center gap-2">
                        <SelectValue placeholder={selectedSeriesId ? "Selecciona un evento" : "Primero selecciona una serie"} />
                        {selectedEvent && selectedEvent.status === 'active' && <Badge className="bg-emerald-50 text-emerald-700">Activa</Badge>}
                      </div>
                    </div>
                  </SelectTrigger>
                  <SelectContent>
                    {eventsList.map((ev) => (
                      <SelectItem key={ev.id} value={ev.id.toString()}>
                        {ev.name} 
                        {ev.status === 'active' && <span className="ml-2"><Badge className="bg-emerald-50 text-emerald-700">Activa</Badge></span>}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            <div className="bg-orange-50 border border-orange-100 rounded-md p-4 mb-4">
              <h3 className="font-medium mb-2">Detalles del evento</h3>
              <div className="grid grid-cols-2 gap-4 text-sm text-muted-foreground">
                <div>
                  <div className="text-xs">Nombre</div>
                  <div className="text-foreground">{selectedEvent ? selectedEvent.name : '—'}</div>
                </div>
                <div>
                  <div className="text-xs">Fecha</div>
                  <div className="text-foreground">{selectedEvent ? selectedEvent.date : '—'}</div>
                </div>
                <div>
                  <div className="text-xs">Rondas</div>
                  <div className="text-foreground">{selectedEvent ? selectedEvent.rounds : '—'}</div>
                </div>
                <div>
                  <div className="text-xs">Equipos</div>
                  <div className="text-foreground">{selectedEvent ? teamsCount : '—'}</div>
                </div>
              </div>
            </div>

            <div className="mb-4">
              <Button
                onClick={() => {
                  if (selectedEvent) setShowCaptureView(true)
                }}
                size="lg"
                className="w-full"
                disabled={!selectedEvent}
              >
                <span className="mr-2">⏱</span> Iniciar Captura de Tiempos
              </Button>
            </div>

            <div className="bg-blue-50 border border-blue-100 rounded-md p-4">
              <div className="text-sm font-semibold text-blue-700">Importante:</div>
              <div className="text-sm text-muted-foreground">Al guardar el primer run, el evento se bloqueará automáticamente. <br/>Una vez bloqueado, no podrás regenerar el draw ni modificar equipos.</div>
            </div>
          </Card>

          <div className="grid grid-cols-3 gap-4 mb-6">
            <Card className="p-4 text-center"> 
              <div className="text-sm text-muted-foreground">Series activas</div>
              <div className="text-2xl font-semibold">{seriesList.filter(s => s.status === 'active').length}</div>
            </Card>
            <Card className="p-4 text-center"> 
              <div className="text-sm text-muted-foreground">Eventos totales</div>
              <div className="text-2xl font-semibold">{eventsList.length}</div>
            </Card>
            <Card className="p-4 text-center"> 
              <div className="text-sm text-muted-foreground">Eventos activos</div>
              <div className="text-2xl font-semibold">{eventsList.filter(e => e.status === 'active').length}</div>
            </Card>
          </div>

          <Card>
            <h3 className="text-lg font-medium mb-4">Flujo de captura</h3>
            <ol className="space-y-3 list-none">
              <li className="flex items-start gap-4"><div className="h-8 w-8 rounded-full bg-orange-500 text-white flex items-center justify-center">1</div><div className="text-muted-foreground">Selecciona una serie y un evento</div></li>
              <li className="flex items-start gap-4"><div className="h-8 w-8 rounded-full bg-orange-500 text-white flex items-center justify-center">2</div><div className="text-muted-foreground">Haz clic en "Iniciar Captura de Tiempos"</div></li>
              <li className="flex items-start gap-4"><div className="h-8 w-8 rounded-full bg-orange-500 text-white flex items-center justify-center">3</div><div className="text-muted-foreground">Selecciona la ronda y el equipo desde la lista</div></li>
              <li className="flex items-start gap-4"><div className="h-8 w-8 rounded-full bg-orange-500 text-white flex items-center justify-center">4</div><div className="text-muted-foreground">Usa el cronómetro para capturar tiempos o ingresa manualmente</div></li>
              <li className="flex items-start gap-4"><div className="h-8 w-8 rounded-full bg-orange-500 text-white flex items-center justify-center">5</div><div className="text-muted-foreground">Guarda cada run y visualiza resultados en tiempo real</div></li>
            </ol>
          </Card>
        </div>

        {/* Right column: placeholder or contextual help */}
        <aside className="hidden lg:block">
          <div className="bg-card border border-border rounded-xl p-4">
            <h4 className="text-sm font-medium mb-2">Ayuda rápida</h4>
            <p className="text-sm text-muted-foreground">Selecciona serie y evento para ver detalles y comenzar la captura. El panel de captura ofrece atajos de teclado y guardado rápido.</p>
          </div>
        </aside>
      </div>
    </div>
  )
}
