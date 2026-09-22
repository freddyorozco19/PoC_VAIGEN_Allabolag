import type { BenchmarkPair } from "@/lib/types";

/** Barra horizontal que compara el valor de la empresa con la mediana del sector (SNI). */
export function BenchmarkBar({
  label,
  pair,
  scaleMax,
  unit = " %",
}: {
  label: string;
  pair: BenchmarkPair;
  scaleMax: number;
  unit?: string;
}) {
  const valuePct = Math.max(0, Math.min(100, (pair.value / scaleMax) * 100));
  const medianPct = Math.max(0, Math.min(100, (pair.median / scaleMax) * 100));

  return (
    <div className="my-[7px] grid grid-cols-[110px_1fr_70px] items-center gap-2 text-[0.85rem]">
      <span>{label}</span>
      <div className="relative h-2 rounded-full bg-surface2" title={`Median: ${pair.median}${unit}`}>
        <div
          className="absolute left-0 top-0 h-2 rounded-full bg-accent"
          style={{ width: `${valuePct}%` }}
        />
        <span
          className="absolute -top-[3px] h-[14px] w-[2px] bg-ink"
          style={{ left: `${medianPct}%` }}
        />
      </div>
      <span className="font-mono">
        {pair.value}
        {unit}
      </span>
    </div>
  );
}
