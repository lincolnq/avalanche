import { onMount } from "solid-js";
import { useApp } from "../../state/AppContext";

const styles = `
  .splash {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background: #FFF1E9;
    color: #1F1815;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  }
  .splash-wordmark {
    font-size: 48px;
    font-weight: 700;
    letter-spacing: -1px;
    margin-bottom: 12px;
    color: #2A1620;
  }
  .splash-tagline {
    font-size: 15px;
    color: #6E6258;
    margin-bottom: 48px;
  }
  .splash-actions {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: 280px;
  }
  .splash-btn {
    padding: 14px 24px;
    border-radius: 12px;
    font-size: 15px;
    font-weight: 600;
    cursor: pointer;
    transition: opacity 0.15s;
  }
  .splash-btn:hover { opacity: 0.85; }
  .splash-btn-primary {
    background: #6B3E50;
    color: #fff;
    border: none;
  }
`;

export default function SplashView() {
  const { restoreAccounts, createAccount, store } = useApp();

  onMount(async () => {
    await restoreAccounts();
  });

  async function handleEnterInvite() {
    // For Day 1, directly create a mock account (Day 2 adds the full flow).
    const link = window.prompt("Paste your invite link:");
    if (!link) return;
    try {
      await createAccount(
        "https://mock.avalancheapp.net",
        "Mock Server",
        "Alice",
        null
      );
    } catch (e) {
      window.alert(`Error: ${String(e)}`);
    }
  }

  return (
    <>
      <style>{styles}</style>
      <div class="splash">
        <div class="splash-wordmark">Avalanche</div>
        <div class="splash-tagline">Secure messaging for organizers</div>
        <div class="splash-actions">
          <button class="splash-btn splash-btn-primary" onClick={handleEnterInvite}>
            Enter Invite Link
          </button>
        </div>
      </div>
    </>
  );
}
