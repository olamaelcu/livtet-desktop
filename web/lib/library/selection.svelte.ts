import { SvelteSet } from 'svelte/reactivity'

export class Selection {
  mode = $state(false)
  selected = new SvelteSet<string>()

  get count() {
    return this.selected.size
  }

  toggle(id: string) {
    if (this.selected.has(id)) this.selected.delete(id)
    else this.selected.add(id)
  }

  selectAll(ids: readonly string[]) {
    for (const id of ids) this.selected.add(id)
    this.mode = true
  }

  clear() {
    this.selected.clear()
  }
}
