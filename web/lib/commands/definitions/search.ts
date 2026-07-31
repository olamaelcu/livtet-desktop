// Commands active while <CommandScope id="search"> is mounted. v1 ships
// only sequences; search.focus / search.clear and selection.* land in
// the follow-up that ships the selection feature.

import { asCommandId, type Command } from '../types'

export const searchCommands: readonly Command[] = [
  {
    id: asCommandId('go.top'),
    label: 'Scroll to top',
    description: 'Jump the search grid back to the top.',
    category: 'Navigation',
    scope: 'search',
    run: () => {
      // The grid uses <wa-scroller>; scroll the first one we find.
      document.querySelector('wa-scroller')?.scrollTo({ top: 0, behavior: 'smooth' })
    },
  },
  {
    id: asCommandId('go.bottom'),
    label: 'Scroll to bottom',
    description: 'Jump the search grid to the bottom.',
    category: 'Navigation',
    scope: 'search',
    run: () => {
      const scroller = document.querySelector('wa-scroller')
      if (!scroller) return
      scroller.scrollTo({
        top: scroller.scrollHeight,
        behavior: 'smooth',
      })
    },
  },
]
