import { load } from '@tauri-apps/plugin-store';
import { browser } from '$app/environment';
import type { ActiveThemeSettings } from './types';
import { presets } from './presets';
import { tokensForActiveTheme } from './token-map';

const DEFAULTS: ActiveThemeSettings = {
	mode: 'auto',
	presetId: 'livtet-light',
	overrides: {},
};

let saveTimer: ReturnType<typeof setTimeout> | null = null;

class ActiveTheme {
	settings = $state<ActiveThemeSettings>({ ...DEFAULTS });
	store: Awaited<ReturnType<typeof load>> | null = null;

	async load() {
		if (!this.store && browser) {
			this.store = await load('theme.store');
		}
		const saved = await this.store?.get<ActiveThemeSettings>('activeThemeSettings');
		if (saved) this.settings = saved;
	}

	update(patch: Partial<ActiveThemeSettings>) {
		this.settings = { ...this.settings, ...patch };
		this.persist();
	}

	persist() {
		if (!this.store) return;
		if (saveTimer) clearTimeout(saveTimer);
		saveTimer = setTimeout(() => {
			this.store?.set('activeThemeSettings', $state.snapshot(this.settings));
		}, 500);
	}

	get resolved() {
		return presets.find((p) => p.id === this.settings.presetId) ?? presets[0];
	}

	get tokens() {
		return tokensForActiveTheme(this.settings, this.resolved);
	}
}

export const activeTheme = new ActiveTheme();
