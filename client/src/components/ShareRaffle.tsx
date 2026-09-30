import { useState, useCallback } from 'react';

interface ShareRaffleProps {
  title: string;
  text: string;
  url: string;
}

export function ShareRaffle({ title, text, url }: ShareRaffleProps) {
  const [toast, setToast] = useState<string | null>(null);

  const handleShare = useCallback(async () => {
    if (navigator.share) {
      try {
        await navigator.share({ title, text, url });
      } catch (error) {
        if (error instanceof DOMException && error.name === 'AbortError') {
          return;
        }
        throw error;
      }
    } else {
      await navigator.clipboard.writeText(url);
      setToast('Link copied!');
      setTimeout(() => setToast(null), 3000);
    }
  }, [title, text, url]);

  return (
    <div>
      <button onClick={handleShare}>Share</button>
      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}
