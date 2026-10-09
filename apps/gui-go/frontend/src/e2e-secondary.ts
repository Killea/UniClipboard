// E2E-only reporter for the quick-panel window. Bundled only
// when VITE_GUI_GO_E2E=1; the main window uses e2e-driver.ts.
import { openUrl } from '@tauri-apps/plugin-opener'
import { Call } from '@wailsio/runtime'

// The page's own external-link call (the one the shared frontend uses) so a scenario can drive the real URL-open chain
// with `panel-js`; the promise outcome goes to the scenario's loopback listener.
;(window as unknown as { __ucE2eOpenUrl: (url: string, report: string) => void }).__ucE2eOpenUrl = (
  url,
  report
) => {
  openUrl(url).then(
    () => void fetch(`${report}?v=ok`, { mode: 'no-cors' }),
    err => void fetch(`${report}?v=${encodeURIComponent(String(err))}`, { mode: 'no-cors' })
  )
}

export async function reportMounted(window: string, step: string, expectText?: string) {
  const deadline = Date.now() + 30000
  const root = document.getElementById('root')!
  while (Date.now() < deadline) {
    const text = document.body.innerText
    if (root.children.length > 0 && (!expectText || text.includes(expectText))) break
    await new Promise(resolve => setTimeout(resolve, 100))
  }
  const text = document.body.innerText
  const ok = root.children.length > 0 && (!expectText || text.includes(expectText))
  await Call.ByName('main.EvidenceService.Record', {
    window,
    step,
    ok,
    detail: { path: location.pathname + location.search, text: text.slice(0, 160) },
  })
}

// The quick panel must follow the shared content lock: report what it shows once it is made visible.
export function reportPanelOnShow() {
  void import('@wailsio/runtime').then(({ Events }) =>
    Events.On('quick-panel://prepare-show', async () => {
      await new Promise(resolve => setTimeout(resolve, 1500))
      const text = document.body.innerText
      await Call.ByName('main.EvidenceService.Record', {
        window: 'quick-panel',
        step: 'quick-panel-shown-state',
        ok: !text.includes('解锁后才能查看'),
        detail: { text: text.slice(0, 160) },
      })
    })
  )
}
