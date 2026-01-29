import { useState } from 'react'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Switch } from './ui/switch'
import { toast } from 'sonner'

const tabs = [
  'Perfil de usuario',
  'Datos y almacenamiento',
  'Apariencia',
  'Notificaciones',
  'Idioma y región',
  'Sistema / Avanzado',
]

export function SettingsManagement() {
  const [active, setActive] = useState(tabs[0])
  const [name, setName] = useState('Emiliano García')
  const [email, setEmail] = useState('emiliano@ropingmanager.com')
  const [role] = useState('Administrador')
  const [clearTempOnClose, setClearTempOnClose] = useState(false)

  // Appearance state
  const [theme, setTheme] = useState<'light' | 'dark' | 'system'>('light')
  const [primaryColor, setPrimaryColor] = useState('#F97316') // Orange-500 hex

  const handleSaveAppearance = () => {
    // Here we would persist the theme settings
    toast.success('Configuración de apariencia guardada')
  }

  return (
    <div className="p-6 h-full">
      <div className="max-w-full">
        <div className="mb-6 flex items-start justify-between">
          <div>
            <h1 className="text-2xl font-semibold text-foreground">Configuración</h1>
            <p className="text-sm text-muted-foreground">Administra tus preferencias, datos y opciones generales de la aplicación</p>
          </div>
        </div>

        <div className="bg-card border border-border rounded-xl p-4 flex gap-6">
          <aside className="w-56 border-r border-border pr-4">
            <nav className="flex flex-col gap-2">
              {tabs.map((t) => (
                <button key={t} onClick={() => setActive(t)} className={`w-full text-left px-3 py-2 rounded-md ${active === t ? 'bg-orange-50 text-orange-700' : 'hover:bg-muted/50'}`}>
                  {t}
                </button>
              ))}
            </nav>
          </aside>

          <section className="flex-1">
            {active === 'Perfil de usuario' && (
              <div>
                <h2 className="text-lg font-medium">Perfil de Usuario</h2>
                <p className="text-sm text-muted-foreground">Administra tu información personal</p>

                <div className="mt-6 grid grid-cols-1 md:grid-cols-2 gap-4">
                  <div className="flex items-center gap-4">
                    <div className="w-20 h-20 rounded-full bg-orange-400 flex items-center justify-center text-white font-bold">EG</div>
                    <div>
                      <div className="font-medium">{name}</div>
                      <div className="text-sm text-muted-foreground">{email}</div>
                      <div className="mt-2 text-xs text-orange-700 font-medium bg-orange-50 inline-block px-2 py-1 rounded-md">Administrador</div>
                    </div>
                  </div>

                  <div className="space-y-3">
                    <div>
                      <label className="text-sm text-muted-foreground">Nombre completo</label>
                      <Input value={name} onChange={(e) => setName(e.target.value)} />
                    </div>

                    <div>
                      <label className="text-sm text-muted-foreground">Correo electrónico</label>
                      <Input value={email} onChange={(e) => setEmail(e.target.value)} />
                    </div>

                    <div>
                      <label className="text-sm text-muted-foreground">Rol</label>
                      <select className="w-full rounded-lg border border-border bg-card px-3 py-2 text-sm" value={role} onChange={() => {}}>
                        <option>Administrador</option>
                      </select>
                    </div>
                  </div>
                </div>

                <div className="mt-6 flex items-center gap-3">
                  <Button className="bg-orange-500 hover:bg-orange-600 text-white">Guardar cambios</Button>
                  <Button variant="outline">Cerrar sesión</Button>
                </div>
              </div>
            )}

            {active === 'Datos y almacenamiento' && (
              <div>
                <h2 className="text-lg font-medium">Gestión de Datos y Base de Datos</h2>
                <p className="text-sm text-muted-foreground">Administra tus datos y realiza respaldos</p>

                <div className="mt-6 bg-muted/50 p-4 rounded-md">
                  <div className="text-sm text-muted-foreground">Ubicación de la base de datos</div>
                  <div className="mt-2 flex gap-3 items-center">
                    <Input value={'/Users/Emiliano/roping-manager/data/main.db'} onChange={() => {}} />
                    <Button variant="outline">Cambiar ubicación</Button>
                  </div>
                </div>

                <div className="mt-4 flex gap-3">
                  <Button className="bg-orange-500 hover:bg-orange-600 text-white">Exportar respaldo</Button>
                  <Button variant="outline">Importar respaldo</Button>
                </div>

                <div className="mt-4 border-t pt-4">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-sm">Borrar datos temporales al cerrar la app</div>
                      <div className="text-xs text-muted-foreground">Limpia archivos temporales automáticamente</div>
                    </div>
                    <Switch checked={clearTempOnClose} onCheckedChange={(v) => setClearTempOnClose(Boolean(v))} />
                  </div>

                  <div className="mt-3 text-sm text-muted-foreground">Tamaño actual de la base de datos <span className="ml-2 font-medium">24 MB</span></div>
                </div>
              </div>
            )}

            {active === 'Apariencia' && (
              <div>
                <h2 className="text-lg font-medium">Apariencia</h2>
                <p className="text-sm text-muted-foreground">Personaliza el aspecto visual de la aplicación</p>

                <div className="mt-6 grid grid-cols-1 gap-4">
                  <div className="flex gap-3">
                    <button 
                      onClick={() => setTheme('light')}
                      className={`p-4 rounded-md border w-1/3 text-center transition-all ${theme === 'light' ? 'border-orange-500 ring-2 ring-orange-200' : 'border-border bg-white'}`}
                    >
                      <div className="font-medium text-slate-900">Tema Claro</div>
                    </button>
                    <button 
                      onClick={() => setTheme('dark')}
                      className={`p-4 rounded-md border w-1/3 text-center transition-all ${theme === 'dark' ? 'border-orange-500 ring-2 ring-orange-200' : 'border-border bg-slate-900 text-white'}`}
                    >
                      <div className="font-medium">Tema Oscuro</div>
                    </button>
                    <button 
                      onClick={() => setTheme('system')}
                      className={`p-4 rounded-md border w-1/3 text-center transition-all ${theme === 'system' ? 'border-orange-500 ring-2 ring-orange-200' : 'border-border bg-gradient-to-r from-white to-slate-900'}`}
                    >
                      <div className={`font-medium ${theme === 'system' ? 'text-orange-700' : 'text-slate-500'}`}>Automático</div>
                    </button>
                  </div>

                  <div>
                    <div className="text-sm text-muted-foreground">Color primario</div>
                    <div className="mt-2 flex items-center gap-2">
                      <div className="w-8 h-8 rounded-md border border-border" style={{ backgroundColor: primaryColor }} />
                      <Input value={primaryColor} onChange={(e) => setPrimaryColor(e.target.value)} className="w-32" />
                      <Button variant="outline" onClick={() => setPrimaryColor('#F97316')}>Predeterminado</Button>
                    </div>
                  </div>

                  <div className="mt-4">
                    <Button className="bg-orange-500 hover:bg-orange-600 text-white" onClick={handleSaveAppearance}>
                      Aplicar Cambios
                    </Button>
                  </div>
                </div>
              </div>
            )}

            {active === 'Notificaciones' && (
              <div>
                <h2 className="text-lg font-medium">Notificaciones</h2>
                <p className="text-sm text-muted-foreground">Configura cómo recibes notificaciones</p>

                <div className="mt-6 space-y-4">
                  <div className="flex items-center justify-between bg-muted/50 p-3 rounded-md">
                    <div>
                      <div className="font-medium">Mostrar notificaciones emergentes</div>
                      <div className="text-xs text-muted-foreground">Toasts en la esquina de la pantalla</div>
                    </div>
                    <Switch checked={true} onCheckedChange={() => {}} />
                  </div>

                  <div className="flex items-center justify-between bg-muted/50 p-3 rounded-md">
                    <div>
                      <div className="font-medium">Reproducir sonido al guardar o exportar</div>
                      <div className="text-xs text-muted-foreground">Retroalimentación auditiva</div>
                    </div>
                    <Switch checked={true} onCheckedChange={() => {}} />
                  </div>
                </div>
              </div>
            )}

            {active === 'Idioma y región' && (
              <div>
                <h2 className="text-lg font-medium">Idioma y Región</h2>
                <p className="text-sm text-muted-foreground">Configura el idioma y formato regional</p>

                <div className="mt-6 grid grid-cols-1 md:grid-cols-2 gap-4">
                  <div>
                    <label className="text-sm text-muted-foreground">Idioma de la interfaz</label>
                    <select className="w-full rounded-lg border border-border bg-card px-3 py-2 text-sm">
                      <option>Español</option>
                    </select>
                  </div>

                  <div>
                    <label className="text-sm text-muted-foreground">Formato de fecha</label>
                    <select className="w-full rounded-lg border border-border bg-card px-3 py-2 text-sm">
                      <option>DD/MM/YYYY</option>
                    </select>
                  </div>
                </div>
              </div>
            )}

            {active === 'Sistema / Avanzado' && (
              <div>
                <h2 className="text-lg font-medium">Sistema / Avanzado</h2>
                <p className="text-sm text-muted-foreground">Información del sistema y opciones avanzadas</p>

                <div className="mt-6 bg-muted/50 p-4 rounded-md">
                  <div className="flex items-center justify-between">
                    <div>
                      <div className="text-sm text-muted-foreground">Versión de la aplicación</div>
                      <div className="font-medium">v1.0.0</div>
                    </div>
                    <Button variant="outline">Buscar actualizaciones</Button>
                  </div>
                </div>

                <div className="mt-4 text-sm text-muted-foreground">Roping Manager usa SQLite y Tauri para ofrecer un entorno rápido y local sin conexión.</div>
              </div>
            )}
          </section>
        </div>
      </div>
    </div>
  )
}
