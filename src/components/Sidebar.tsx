import {
  LayoutDashboard,
  Calendar,
  Users,
  Trophy,
  BarChart3,
  DollarSign,
  Download,
  Settings,
  Clock,
} from 'lucide-react'

interface SidebarProps {
  activeItem: string
  onItemClick: (item: string) => void
}

export function Sidebar({ activeItem, onItemClick }: SidebarProps) {
  const menuItems = [
    { id: 'dashboard', label: 'Dashboard', icon: LayoutDashboard },
    { id: 'ropers', label: 'Ropers', icon: Trophy },
    { id: 'eventos', label: 'Eventos', icon: Calendar },
    { id: 'equipos', label: 'Equipos', icon: Users },
    { id: 'captura', label: 'Captura', icon: Clock },
    { id: 'resultados', label: 'Resultados', icon: BarChart3 },
    { id: 'payoffs', label: 'Payoffs', icon: DollarSign },
    { id: 'exportar', label: 'Exportar', icon: Download },
    { id: 'settings', label: 'Settings', icon: Settings },
  ]

  return (
    <aside role="navigation" aria-label="Primary" className="w-64 h-full bg-[var(--sidebar)] border-r border-[var(--sidebar-border)]">
      <div className="p-6">
        <div className="flex items-center gap-3 mb-10">
          <div className="size-10 rounded-xl flex items-center justify-center shadow-sm bg-[var(--sidebar-primary)]">
            <span className="text-[var(--sidebar-primary-foreground)] font-medium">RM</span>
          </div>
          <div>
            <h2 className="text-sm text-foreground">Roping Manager</h2>
            <p className="text-xs text-[var(--sidebar-foreground)]/80">Gestión de eventos</p>
          </div>
        </div>

        <div className="mb-4">
          <h3 className="text-[10px] uppercase tracking-wider mb-3 px-3 text-[var(--sidebar-foreground)]/70">
            Administración
          </h3>
          <nav className="space-y-1">
            {menuItems.map((item) => {
              const Icon = item.icon
              const isActive = activeItem === item.id
              return (
                <button
                  key={item.id}
                  type="button"
                  aria-current={isActive ? 'page' : undefined}
                  onClick={() => onItemClick(item.id)}
                  className={[
                    'w-full flex items-center gap-3 px-3 py-2.5 rounded-xl text-left transition-colors',
                    isActive
                      ? 'bg-[var(--sidebar-accent)] text-[var(--sidebar-accent-foreground)] shadow-sm'
                      : 'text-[var(--sidebar-foreground)] hover:bg-white hover:text-foreground',
                  ].join(' ')}
                >
                  <Icon
                    aria-hidden
                    className={[
                      'size-5',
                      isActive ? 'text-[var(--sidebar-accent-foreground)]' : '',
                    ].join(' ')}
                  />
                  <span className="text-sm">{item.label}</span>
                </button>
              )
            })}
          </nav>
        </div>
      </div>
      <div className="border-t border-[var(--sidebar-border)] p-4 text-[var(--sidebar-foreground)]/70 text-xs">
        v1.0.0 · © 2025
      </div>
    </aside>
  )
}
