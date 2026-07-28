import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react';
import { ShareRaffle } from '../ShareRaffle';

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('ShareRaffle', () => {
  it('calls navigator.share when available', async () => {
    const share = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { share, clipboard: { writeText: vi.fn() } });

    render(<ShareRaffle title="Test" text="Test text" url="https://example.com" />);

    await act(async () => {
      fireEvent.click(screen.getByText('Share'));
    });

    expect(share).toHaveBeenCalledWith({
      title: 'Test',
      text: 'Test text',
      url: 'https://example.com',
    });
  });

  it('falls back to clipboard when navigator.share is not available', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { share: undefined, clipboard: { writeText } });

    render(<ShareRaffle title="Test" text="Test text" url="https://example.com" />);

    await act(async () => {
      fireEvent.click(screen.getByText('Share'));
    });

    expect(writeText).toHaveBeenCalledWith('https://example.com');
  });

  it('shows toast after clipboard copy', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { share: undefined, clipboard: { writeText } });

    render(<ShareRaffle title="Test" text="Test text" url="https://example.com" />);

    await act(async () => {
      fireEvent.click(screen.getByText('Share'));
    });

    expect(screen.getByText('Link copied!')).toBeDefined();
  });

  it('handles AbortError when user dismisses share sheet', async () => {
    const share = vi
      .fn()
      .mockRejectedValue(
        new DOMException('The user aborted a request.', 'AbortError'),
      );
    vi.stubGlobal('navigator', { share, clipboard: { writeText: vi.fn() } });

    render(<ShareRaffle title="Test" text="Test text" url="https://example.com" />);

    await act(async () => {
      fireEvent.click(screen.getByText('Share'));
    });

    expect(share).toHaveBeenCalled();
    expect(screen.queryByText('Link copied!')).toBeNull();
  });
});
