<script lang="ts">
  import { toast } from 'svelte-sonner'
  import { commands } from '$lib/bindings'

  let selected = $state<string>('')

  $effect(() => {
    commands.getLanguagePreference().then((r) => {
      if (r.status === 'ok') selected = r.data.language ?? ''
    })
  })

  async function onChange(e: Event) {
    const target = e.target as HTMLSelectElement | null
    if (!target) return
    const lang = target.value || null
    const r = await commands.setLanguagePreference(lang)
    if (r.status === 'ok') {
      selected = r.data.language ?? ''
      if (lang) {
        toast.success(`Language preference set to ${lang}`)
      } else {
        toast.success('Language filter cleared')
      }
    } else {
      toast.error(r.error)
    }
  }

  const languages = [
    { code: '', label: 'None (all languages)' },
    { code: 'ara', label: 'Arabic' },
    { code: 'zho', label: 'Chinese' },
    { code: 'ces', label: 'Czech' },
    { code: 'nld', label: 'Dutch' },
    { code: 'eng', label: 'English' },
    { code: 'fra', label: 'French' },
    { code: 'deu', label: 'German' },
    { code: 'ita', label: 'Italian' },
    { code: 'jpn', label: 'Japanese' },
    { code: 'pol', label: 'Polish' },
    { code: 'por', label: 'Portuguese' },
    { code: 'rus', label: 'Russian' },
    { code: 'spa', label: 'Spanish' },
    { code: 'swe', label: 'Swedish' },
    { code: 'tur', label: 'Turkish' },
  ]
</script>

<section class="card">
  <header>
    <h2>Search Language</h2>
    <p class="helper">
      Filter remote search results to only show books in one language.
      OpenLibrary returns results in every language by default — set your
      preferred language here.
    </p>
  </header>

  <select class="lang-select" value={selected} onchange={onChange}>
    {#each languages as lang}
      <option value={lang.code}>{lang.label}</option>
    {/each}
  </select>
</section>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    padding: 1.25rem;
    border: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.1));
    border-radius: var(--wa-border-radius-m, 6px);
    background: var(--wa-color-surface-default, white);
    max-width: 32rem;
  }

  header h2 {
    margin: 0 0 0.25rem 0;
    font-size: 1.125rem;
  }

  .helper {
    margin: 0;
    font-size: 0.875rem;
    color: var(--wa-color-text-quiet, currentColor);
  }

  .lang-select {
    padding: 0.5rem 0.625rem;
    font-size: 0.9375rem;
    border: 1px solid var(--wa-color-input-border, rgba(0, 0, 0, 0.15));
    border-radius: var(--wa-border-radius-s, 4px);
    background: var(--wa-color-input-background, white);
    color: var(--wa-color-text-default, inherit);
    max-width: 16rem;
    cursor: pointer;
  }

  .lang-select:focus {
    outline: 2px solid var(--wa-color-brand-fill, #0059f7);
    outline-offset: -1px;
  }
</style>
