/**
 * Smoke test for `RewardsCommunityTab` — exercises the `role.unlocked`
 * branch (line 248) added by PR #2095's dark-mode pass so the diff
 * coverage gate has the touched line covered.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { RewardsSnapshot } from '../../../types/rewards';

const { openUrl, callCoreRpc, setOAuthReturnRoute } = vi.hoisted(() => ({
  openUrl: vi.fn(),
  callCoreRpc: vi.fn(),
  setOAuthReturnRoute: vi.fn(),
}));

vi.mock('../../../utils/openUrl', () => ({ openUrl }));
vi.mock('../../../services/coreRpcClient', () => ({ callCoreRpc }));
vi.mock('../../../utils/oauthReturnRoute', () => ({ setOAuthReturnRoute }));

function buildSnapshot(): RewardsSnapshot {
  return {
    discord: {
      linked: true,
      discordId: 'discord-1',
      username: 'cooluser',
      inviteUrl: 'https://discord.gg/example',
      membershipStatus: 'member',
    },
    summary: {
      unlockedCount: 1,
      totalCount: 2,
      assignedDiscordRoleCount: 1,
      plan: 'FREE',
      hasActiveSubscription: false,
    },
    metrics: {
      currentStreakDays: 3,
      longestStreakDays: 5,
      cumulativeTokens: 1234,
      featuresUsedCount: 2,
      trackedFeaturesCount: 5,
      lastEvaluatedAt: null,
      lastSyncedAt: null,
    },
    achievements: [
      {
        id: 'role-1',
        title: 'Pioneer',
        description: 'Joined early.',
        actionLabel: 'View',
        unlocked: true,
        progressLabel: '1/1',
        roleId: 'discord-role-1',
        discordRoleStatus: 'assigned',
        creditAmountUsd: null,
      },
      {
        id: 'role-2',
        title: 'Veteran',
        description: 'Long streak.',
        actionLabel: 'View',
        unlocked: false,
        progressLabel: '0/1',
        roleId: 'discord-role-2',
        discordRoleStatus: 'not_assigned',
        creditAmountUsd: null,
      },
    ],
  };
}

describe('RewardsCommunityTab — role card branches', () => {
  it('renders both unlocked and locked roles (covers the `role.unlocked` ring branch)', async () => {
    const { default: RewardsCommunityTab } = await import('../RewardsCommunityTab');
    render(
      <MemoryRouter>
        <RewardsCommunityTab error={null} isLoading={false} snapshot={buildSnapshot()} />
      </MemoryRouter>
    );

    // Both role titles are rendered — each goes through the ternary on
    // line 248 (ring-primary-100 for unlocked, ring-black/[0.04] for locked).
    expect(screen.getByText('Pioneer')).toBeInTheDocument();
    expect(screen.getByText('Veteran')).toBeInTheDocument();
  });
});

describe('RewardsCommunityTab — Connect Discord', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  function notLinkedSnapshot(): RewardsSnapshot {
    const snapshot = buildSnapshot();
    return {
      ...snapshot,
      discord: {
        linked: false,
        discordId: null,
        username: null,
        inviteUrl: 'https://discord.gg/example',
        membershipStatus: 'not_linked',
      },
    };
  }

  it('starts the OAuth flow and opens the consent URL on connect', async () => {
    callCoreRpc.mockResolvedValueOnce({ result: { oauthUrl: 'https://discord.com/oauth' } });
    const { default: RewardsCommunityTab } = await import('../RewardsCommunityTab');
    render(
      <MemoryRouter>
        <RewardsCommunityTab error={null} isLoading={false} snapshot={notLinkedSnapshot()} />
      </MemoryRouter>
    );

    fireEvent.click(screen.getByTestId('rewards-connect-discord'));

    await waitFor(() => expect(openUrl).toHaveBeenCalledWith('https://discord.com/oauth'));
    // Return route is persisted only after the consent URL launches.
    await waitFor(() => expect(setOAuthReturnRoute).toHaveBeenCalledWith('/rewards'));
    expect(callCoreRpc).toHaveBeenCalledWith({
      method: 'openhuman.auth.oauth_connect',
      params: { provider: 'discord' },
    });
  });

  it('surfaces an error when the RPC returns no oauthUrl', async () => {
    callCoreRpc.mockResolvedValueOnce({ result: {} });
    const { default: RewardsCommunityTab } = await import('../RewardsCommunityTab');
    render(
      <MemoryRouter>
        <RewardsCommunityTab error={null} isLoading={false} snapshot={notLinkedSnapshot()} />
      </MemoryRouter>
    );

    fireEvent.click(screen.getByTestId('rewards-connect-discord'));

    await waitFor(() =>
      expect(screen.getByTestId('rewards-connect-discord-error')).toBeInTheDocument()
    );
    expect(openUrl).not.toHaveBeenCalled();
  });

  it('surfaces an error when the connect RPC rejects', async () => {
    callCoreRpc.mockRejectedValueOnce(new Error('rpc down'));
    const { default: RewardsCommunityTab } = await import('../RewardsCommunityTab');
    render(
      <MemoryRouter>
        <RewardsCommunityTab error={null} isLoading={false} snapshot={notLinkedSnapshot()} />
      </MemoryRouter>
    );

    fireEvent.click(screen.getByTestId('rewards-connect-discord'));

    await waitFor(() =>
      expect(screen.getByTestId('rewards-connect-discord-error')).toBeInTheDocument()
    );
    // A failed initiation must not persist any return route (it's only set after launch).
    expect(setOAuthReturnRoute).not.toHaveBeenCalled();
  });

  it('renders the connected username pill and footer when linked', async () => {
    const { default: RewardsCommunityTab } = await import('../RewardsCommunityTab');
    render(
      <MemoryRouter>
        <RewardsCommunityTab error={null} isLoading={false} snapshot={buildSnapshot()} />
      </MemoryRouter>
    );

    expect(screen.getByTestId('rewards-discord-connected')).toHaveTextContent('cooluser');
    expect(screen.getByTestId('rewards-discord-username')).toHaveTextContent('cooluser');
    expect(screen.queryByTestId('rewards-connect-discord')).not.toBeInTheDocument();
  });
});
