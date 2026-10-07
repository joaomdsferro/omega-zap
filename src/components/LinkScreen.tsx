import { useEffect, useState } from "react";
import type { AccountInfo } from "../accounts/accounts";
import type { AccountStatus } from "../accounts/useAccount";
import { formatVoiceDuration } from "../lib/format";
import { CloseIcon } from "./Icons";

interface LinkScreenProps {
  account: AccountInfo;
  status: AccountStatus;
  onRetry: () => void;
  onClose: () => void;
}

function Spinner() {
  return (
    <span
      className="block h-10 w-10 animate-spin rounded-full border-[3px] border-(--divider) border-t-(--accent)"
      aria-hidden
    />
  );
}

/** Seconds left of `seconds`, counted from when `code` last changed. */
function useCountdown(seconds: number | null, code: string | null) {
  const [left, setLeft] = useState(seconds);
  useEffect(() => {
    if (seconds === null || code === null) {
      setLeft(null);
      return;
    }
    const end = Date.now() + seconds * 1000;
    const tick = () => setLeft(Math.max(0, Math.ceil((end - Date.now()) / 1000)));
    tick();
    const id = window.setInterval(tick, 1000);
    return () => window.clearInterval(id);
  }, [seconds, code]);
  return left;
}

export function LinkScreen({ account, status, onRetry, onClose }: LinkScreenProps) {
  const { state } = status;
  const showingQr = state === "qr" && status.qrSvg !== null;
  const secondsLeft = useCountdown(showingQr ? status.qrTimeoutSecs : null, status.qrSvg);

  let qrArea;
  if (state === "unsupported") {
    qrArea = (
      <p className="px-6 text-center text-[14px] text-(--text-secondary)">
        Linking needs the desktop app. Run <code>pnpm tauri dev</code>.
      </p>
    );
  } else if (state === "qr" && status.qrSvg) {
    qrArea = (
      <div
        className="h-66 w-66 [&>svg]:h-full [&>svg]:w-full"
        role="img"
        aria-label="QR code for linking this computer"
        // The SVG is generated locally by the qrcode crate from the network's pairing payload.
        dangerouslySetInnerHTML={{ __html: status.qrSvg }}
      />
    );
  } else if (state === "qr_expired" || state === "error") {
    qrArea = (
      <div className="flex flex-col items-center gap-4 px-6 text-center">
        <p className="text-[14px] text-(--text-secondary)">
          {state === "qr_expired" ? "The QR code expired." : (status.error ?? "Something went wrong.")}
        </p>
        <button
          type="button"
          onClick={onRetry}
          className="h-10 rounded-full bg-(--accent) px-5 text-[14px] font-medium text-(--on-accent) hover:brightness-110"
        >
          {state === "qr_expired" ? "Reload QR code" : "Try again"}
        </button>
      </div>
    );
  } else {
    qrArea = (
      <div className="flex flex-col items-center gap-4">
        <Spinner />
        <p className="text-[14px] text-(--text-secondary)">
          {state === "connecting" ? `Linking… keep ${account.label} open on your phone` : "Preparing QR code…"}
        </p>
      </div>
    );
  }

  return (
    <section className="relative flex h-full flex-1 items-center justify-center overflow-y-auto border-l border-(--divider) bg-(--empty-bg) p-8">
      <button
        type="button"
        onClick={onClose}
        aria-label="Close"
        className="absolute top-4 right-4 flex h-10 w-10 items-center justify-center rounded-full text-(--icon) hover:bg-(--hover-bg)"
      >
        <CloseIcon size={22} />
      </button>

      <div
        className="flex w-full max-w-215 flex-wrap items-center justify-between gap-10 rounded-[3px] bg-(--panel-bg) px-12 py-14 animate-fade-rise"
        style={{ boxShadow: "0 17px 50px 0 rgba(11,20,26,.19), 0 12px 15px 0 rgba(11,20,26,.24)" }}
      >
        <div className="min-w-65 flex-1">
          <h2 className="text-[28px] leading-tight font-light text-(--text-primary)">
            Use {account.label} on this computer
          </h2>
          <ol className="mt-8 flex flex-col gap-4 text-[17px] leading-7 text-(--text-primary)">
            {account.linkSteps.map((step, i) => (
              <li key={step} className="flex gap-3">
                <span className="text-(--text-secondary)">{i + 1}.</span>
                <span>{step}</span>
              </li>
            ))}
          </ol>
          <p className="mt-8 text-[13px] leading-5 text-(--text-secondary)">
            {account.linkNote}
          </p>
        </div>
        <div className="flex w-66 shrink-0 flex-col items-center gap-3">
          <div className="flex h-66 w-66 items-center justify-center">{qrArea}</div>
          <p
            className="min-h-10 text-center text-[13px] leading-5 text-(--text-secondary)"
            role="timer"
            aria-live="off"
          >
            {secondsLeft === null ? null : secondsLeft > 0 ? (
              <>
                Finish linking on your phone within{" "}
                <span className="font-medium tabular-nums text-(--text-primary)">
                  {formatVoiceDuration(secondsLeft)}
                </span>
                . A new code appears when this one expires.
              </>
            ) : (
              "Getting a new code…"
            )}
          </p>
        </div>
      </div>
    </section>
  );
}
