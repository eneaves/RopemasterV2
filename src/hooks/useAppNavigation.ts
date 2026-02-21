import { useCallback } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'

export const MENU_ROUTE_MAP = {
  dashboard: '/',
  eventos: '/eventos',
  equipos: '/equipos',
  ropers: '/ropers',
  captura: '/captura',
  resultados: '/resultados',
  payoffs: '/payoffs',
  exportar: '/exportar',
  settings: '/settings',
  activity: '/activity',
} as const

export type MenuRouteKey = keyof typeof MENU_ROUTE_MAP

const NORMALIZED_PATH_MAP = Object.entries(MENU_ROUTE_MAP).reduce<Record<string, MenuRouteKey>>((acc, [key, path]) => {
  acc[normalizePath(path)] = key as MenuRouteKey
  return acc
}, {})

function normalizePath(pathname: string) {
  if (!pathname) return '/'
  const normalized = pathname.endsWith('/') && pathname !== '/' ? pathname.slice(0, -1) : pathname
  return normalized || '/'
}

export function useAppNavigation() {
  const navigate = useNavigate()
  return useCallback(
    (item: MenuRouteKey) => {
      const target = MENU_ROUTE_MAP[item] ?? MENU_ROUTE_MAP.dashboard
      navigate(target)
    },
    [navigate],
  )
}

export function useActiveMenuKey() {
  const location = useLocation()
  const key = NORMALIZED_PATH_MAP[normalizePath(location.pathname)]
  return key ?? 'dashboard'
}

export function pathForMenu(item: MenuRouteKey) {
  return MENU_ROUTE_MAP[item] ?? MENU_ROUTE_MAP.dashboard
}
