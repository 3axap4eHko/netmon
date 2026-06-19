import { invoke } from '@tauri-apps/api/core';

let settings: Record<string, unknown> = {};

export async function hydrateSettings(): Promise<void> {
  try {
    const raw = await invoke<string | null>('get_ui_settings');
    settings = raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
  } catch {
    settings = {};
  }
}

export function loadStored<T>(key: string, fallback: T): T {
  const value = settings[key];
  return value === undefined || value === null ? fallback : (value as T);
}

export function saveStored(key: string, value: unknown): void {
  if (value === null || value === undefined) {
    delete settings[key];
  } else {
    settings[key] = value;
  }
  schedulePersist();
}

let persistTimer: ReturnType<typeof setTimeout> | null = null;

function schedulePersist(): void {
  if (persistTimer !== null) {
    clearTimeout(persistTimer);
  }
  persistTimer = setTimeout(() => {
    persistTimer = null;
    invoke('set_ui_settings', { json: JSON.stringify(settings) }).catch(() => {
      // Best-effort persistence; a failed write is retried on the next save.
    });
  }, 200);
}
