export type NavItem = { href: string; label: string; icon: string }

export const NAV_ITEMS: readonly NavItem[] = [
  { href: '/library', label: 'Library', icon: 'book' },
  { href: '/reading-lists', label: 'Reading lists', icon: 'list-ul' },
  { href: '/catalog', label: 'Catalogs', icon: 'globe' },
  { href: '/plugins', label: 'Plugins', icon: 'puzzle-piece' },
  { href: '/settings', label: 'Settings', icon: 'gear' },
]
