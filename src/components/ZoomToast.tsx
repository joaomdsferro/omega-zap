export function ZoomToast({ zoom }: { zoom: number | null }) {
  if (zoom === null) return null;
  return (
    <div
      key={zoom}
      role="status"
      className="pointer-events-none fixed top-4 left-1/2 z-50 rounded-full bg-(--overlay) px-4 py-1.5 text-[14px] text-white tabular-nums animate-toast"
    >
      Zoom {Math.round(zoom * 100)}%
    </div>
  );
}
