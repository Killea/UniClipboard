import { render, screen } from '@testing-library/react'
import { fetchSponsors } from '@/api/sponsors'
import AboutSection from '@/components/setting/AboutSection'

vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('0.4.0-alpha.6'),
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

vi.mock('@/api/sponsors', () => ({
  fetchSponsors: vi.fn(() => new Promise(() => undefined)),
}))

const mockFetchSponsors = vi.mocked(fetchSponsors)

describe('AboutSection', () => {
  it('shows the app name, version and self-maintained declaration', async () => {
    render(<AboutSection />)

    expect(screen.getByText('settings.sections.about.appName')).toBeInTheDocument()
    expect(await screen.findByText('settings.sections.about.version')).toBeInTheDocument()
    expect(screen.getByText('settings.sections.about.selfMaintained')).toBeInTheDocument()
    expect(
      screen.getByRole('link', { name: 'settings.sections.about.links.repository' })
    ).toHaveAttribute('href', 'https://github.com/Killea/UniClipboard')
  })

  it('shows sponsor and GitHub star actions in the sponsors group', async () => {
    mockFetchSponsors.mockResolvedValueOnce([
      {
        id: 'sponsor-1',
        name: 'Ada Lovelace',
        tier: 'regular',
      },
    ])

    render(<AboutSection />)

    const sponsorLink = await screen.findByRole('link', {
      name: 'settings.sections.about.sponsors.becomeSponsor',
    })
    const starLink = screen.getByRole('link', {
      name: 'settings.sections.about.sponsors.githubStar',
    })

    expect(sponsorLink).toHaveAttribute('href', 'https://afdian.com/a/mkdir700')
    expect(starLink).toHaveAttribute('href', 'https://github.com/UniClipboard/UniClipboard')
  })

  it('renders no update controls', () => {
    render(<AboutSection />)

    expect(screen.queryByRole('switch')).not.toBeInTheDocument()
    expect(screen.queryByRole('combobox')).not.toBeInTheDocument()
    expect(
      screen.queryByRole('button', { name: 'settings.sections.about.checkUpdate' })
    ).not.toBeInTheDocument()
  })
})
