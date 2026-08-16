import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const source = readFileSync(
  resolve(process.cwd(), 'src/features/providers/components/ProviderDetailDrawer.vue'),
  'utf8',
)

describe('ProviderDetailDrawer loading priorities', () => {
  it('plays the drawer transition when the lazily mounted component first appears', () => {
    const transitionTemplate = source
      .split('<Transition')[1]
      ?.split('>')[0]

    expect(transitionTemplate).toContain('name="drawer"')
    expect(transitionTemplate).toContain('appear')
    expect(source).toContain('class="drawer-panel relative')
    expect(source).toContain('.drawer-enter-from .drawer-panel')
    expect(source).not.toContain('.drawer-enter-from .relative')
  })

  it('loads provider detail resources together', () => {
    expect(source).toContain('await Promise.all([loadProvider(), loadEndpoints(), loadProviderKeysPage(), loadModels(), loadMappingPreview()])')
  })

  it('keeps model and mapping loading states independent', () => {
    expect(source).toContain(':loading="loadingProviderModels"')
    expect(source).toContain(':loading="loadingProviderMappingPreview"')
    expect(source).not.toContain(':loading="loadingProviderModels || loadingProviderKeys"')
    expect(source).not.toContain(':loading="loadingProviderEndpoints || loadingProviderKeys || loadingProviderModels || loadingProviderMappingPreview"')
  })

  it('does not mount closed child dialogs or blur the page behind the drawer', () => {
    expect(source).toContain('class="absolute inset-0 bg-black/30"')
    expect(source).not.toContain('backdrop-blur-sm')
    expect(source).toContain('v-if="provider && open && endpointDialogOpen"')
    expect(source).toContain('v-if="open && keyFormDialogOpen"')
    expect(source).not.toContain('oauthAccountDialogOpen')
    expect(source).not.toContain('oauthKeyEditDialogOpen')
    expect(source).toContain('v-if="open && keyPermissionsDialogOpen"')
    expect(source).toContain('v-if="open && modelFormDialogOpen && provider"')
    expect(source).toContain('v-if="open && batchAssignDialogOpen && provider"')
    expect(source).toContain('v-if="open && failoverRulesDialogOpen"')
  })

  it('marks a transport-error stop policy as a configured failover rule', () => {
    expect(source).toContain('Object.values(rules).some')
  })
})
