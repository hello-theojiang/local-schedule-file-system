import { defineConfig } from '@playwright/test';
import path from 'node:path';
import os from 'node:os';

export const DIR = path.join(os.tmpdir(), 'agenda-e2e');
export const PORT = 8431;
const bin = path.resolve('../target/release/agenda');

export default defineConfig({
  testDir: 'tests',
  timeout: 45_000,
  workers: 1,
  retries: 0,
  reporter: [['list']],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    locale: 'fr-FR',
    timezoneId: 'Europe/Paris',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  webServer: {
    command: `node make-sample.mjs ${DIR} && ${bin} --dir ${DIR} serve --addr 127.0.0.1:${PORT}`,
    url: `http://127.0.0.1:${PORT}/health`,
    reuseExistingServer: false,
    timeout: 20_000,
    env: { TZ: 'Europe/Paris', XDG_STATE_HOME: path.join(os.tmpdir(), 'agenda-e2e-state') },
  },
});
