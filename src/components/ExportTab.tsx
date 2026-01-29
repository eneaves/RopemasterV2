import { useState } from 'react'
import { Download, FileText, CheckCircle, Calendar, HardDrive } from 'lucide-react'
import { Button } from './ui/button'
import { Badge } from './ui/badge'
import { Checkbox } from './ui/checkbox'
import { Label } from './ui/label'
import { toast } from 'sonner'
import { save } from '@tauri-apps/plugin-dialog'
import { exportEvent } from '../lib/api'

interface ExportTabProps {
  event: any
  series: any
}

export function ExportTab({ event, series }: ExportTabProps) {
  const [selectedSheets, setSelectedSheets] = useState({
    overview: true,
    teams: true,
    runOrder: true,
    standings: true,
    payoffs: true,
    eventLogs: true,
  })

  const [lastExport, setLastExport] = useState({
    date: '—',
    size: '—',
    filename: '—',
  })

  const handleExport = async () => {
    const selectedCount = Object.values(selectedSheets).filter(Boolean).length
    if (selectedCount === 0) {
      toast.error('Selecciona al menos una hoja para exportar')
      return
    }

    try {
      const defaultFilename = `${(series?.name ?? 'Series').replace(/\s+/g, '')}_${(event?.name ?? 'Event').replace(/\s+/g, '_')}_2025.xlsx`
      
      const filePath = await save({
        defaultPath: defaultFilename,
        filters: [{
          name: 'Excel Workbook',
          extensions: ['xlsx']
        }]
      });

      if (!filePath) return; // User cancelled

      const toastId = toast.loading('Generando archivo Excel...')

      await exportEvent(Number(event.id), {
        overview: selectedSheets.overview,
        teams: selectedSheets.teams,
        run_order: selectedSheets.runOrder,
        standings: selectedSheets.standings,
        payoffs: selectedSheets.payoffs,
        event_logs: selectedSheets.eventLogs,
        file_path: filePath
      });

      toast.dismiss(toastId)
      toast.success(`Evento exportado: ${filePath}`)
      
      setLastExport({
        date: new Date().toLocaleString('es-ES'),
        size: 'Unknown', // We don't know the size unless we check the file
        filename: filePath.split(/[/\\]/).pop() || defaultFilename,
      })
    } catch (error) {
      console.error(error)
      toast.error('Error al exportar evento')
    }
  }

  const handleToggleSheet = (sheet: keyof typeof selectedSheets) => {
    setSelectedSheets((prev) => ({ ...prev, [sheet]: !prev[sheet] }))
  }

  const handleSelectAll = () => {
    const allSelected = Object.values(selectedSheets).every(Boolean)
    const newValue = !allSelected
    setSelectedSheets({
      overview: newValue,
      teams: newValue,
      runOrder: newValue,
      standings: newValue,
      payoffs: newValue,
      eventLogs: newValue,
    })
  }

  const sheets = [
    { id: 'overview', label: 'Overview', description: 'Resumen general del evento', icon: '📊' },
    { id: 'teams', label: 'Teams', description: 'Lista completa de equipos', icon: '👥' },
    { id: 'runOrder', label: 'Run Order', description: 'Orden de competencia por rondas', icon: '🎲' },
    { id: 'standings', label: 'Standings', description: 'Tabla de posiciones final', icon: '🏆' },
    { id: 'payoffs', label: 'Payoffs', description: 'Distribución de premios', icon: '💰' },
    { id: 'eventLogs', label: 'Event Logs', description: 'Historial de actividad', icon: '📝' },
  ] as const

  const allSelected = Object.values(selectedSheets).every(Boolean)
  const selectedCount = Object.values(selectedSheets).filter(Boolean).length

  return (
    <div className="space-y-6">
      {/* Header */}
      <div>
        <h2 className="text-foreground mb-1">Export Event</h2>
        <p className="text-muted-foreground">Genera un reporte completo del evento en formato Excel (XLSX)</p>
      </div>

      {/* Export CTA */}
      <div className="bg-accent rounded-xl border border-accent p-6 shadow-sm">
        <div className="flex items-center gap-4 mb-4">
          <div className="p-3 bg-primary rounded-xl">
            <Download className="w-6 h-6 text-primary-foreground" />
          </div>
          <div>
            <h3 className="text-foreground mb-1">Generate Full Event Report</h3>
            <p className="text-muted-foreground">Exporta toda la información del evento en un archivo Excel</p>
          </div>
        </div>

        <Button
          onClick={handleExport}
          disabled={selectedCount === 0}
          size="lg"
          className="w-full bg-primary text-primary-foreground rounded-xl shadow-sm hover:opacity-90 h-11"
        >
          <Download className="w-5 h-5 mr-2" />
          Export to Excel ({selectedCount} {selectedCount === 1 ? 'sheet' : 'sheets'})
        </Button>
      </div>

      {/* Sheet Selection */}
      <div className="bg-card rounded-xl border border-border p-6">
        <div className="flex justify-between items-center mb-4">
          <h3 className="text-foreground">Select Sheets to Export</h3>
          <Button variant="outline" size="sm" onClick={handleSelectAll} className="border-border">
            {allSelected ? 'Deselect All' : 'Select All'}
          </Button>
        </div>

        <div className="space-y-3">
          {sheets.map((sheet) => {
            const checked = selectedSheets[sheet.id as keyof typeof selectedSheets]
            return (
              <div
                key={sheet.id}
                className="flex items-start space-x-3 p-4 rounded-xl border border-border hover:bg-accent/30 transition-colors"
              >
                <Checkbox
                  id={sheet.id}
                  checked={checked}
                  onCheckedChange={() => handleToggleSheet(sheet.id as keyof typeof selectedSheets)}
                  className="mt-1"
                />
                <div className="flex-1">
                  <Label htmlFor={sheet.id} className="text-foreground cursor-pointer flex items-center gap-2">
                    <span>{sheet.icon}</span>
                    <span>{sheet.label}</span>
                  </Label>
                  <p className="text-muted-foreground mt-1">{sheet.description}</p>
                </div>
                {checked && <CheckCircle className="w-5 h-5 text-foreground/70 flex-shrink-0" />}
              </div>
            )
          })}
        </div>
      </div>

      {/* Last Export Info */}
      <div className="bg-card rounded-xl border border-border p-6">
        <h3 className="text-foreground mb-4">Last Export Information</h3>

        <div className="space-y-4">
          <div className="flex items-center gap-3 p-3 bg-muted rounded-xl border border-border">
            <FileText className="w-5 h-5 text-foreground/70" />
            <div className="flex-1">
              <p className="text-muted-foreground">Filename</p>
              <p className="text-foreground">{lastExport.filename}</p>
            </div>
          </div>

          <div className="flex items-center gap-3 p-3 bg-muted rounded-xl border border-border">
            <Calendar className="w-5 h-5 text-foreground/70" />
            <div className="flex-1">
              <p className="text-muted-foreground">Export Date</p>
              <p className="text-foreground">{lastExport.date}</p>
            </div>
          </div>

          <div className="flex items-center gap-3 p-3 bg-muted rounded-xl border border-border">
            <HardDrive className="w-5 h-5 text-foreground/70" />
            <div className="flex-1">
              <p className="text-muted-foreground">File Size</p>
              <p className="text-foreground">{lastExport.size}</p>
            </div>
          </div>
        </div>

        <div className="mt-6 p-4 bg-accent rounded-xl border border-accent flex items-center gap-3">
          <Badge className="bg-primary text-primary-foreground">OK</Badge>
          <p className="text-foreground">Event exported successfully — file saved to downloads</p>
        </div>
      </div>

      {/* Info */}
      <div className="p-4 bg-muted rounded-xl border border-border">
        <h4 className="text-foreground mb-2">💡 Export Information</h4>
        <ul className="text-muted-foreground space-y-1 ml-4 list-disc">
          <li>El archivo Excel incluirá todas las hojas seleccionadas</li>
          <li>Los datos se exportan en formato compatible con Excel 2010+</li>
          <li>Los gráficos y formatos se preservan en el export</li>
          <li>El archivo se guardará automáticamente en tu carpeta de descargas</li>
        </ul>
      </div>
    </div>
  )
}
