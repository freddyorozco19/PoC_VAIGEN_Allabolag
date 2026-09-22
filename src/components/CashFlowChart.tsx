import type { CashWeek } from "@/lib/types";

/** Gráfico de barras SVG de la caja proyectada, calculada a partir del saldo inicial y entradas/salidas semanales. */
export function CashFlowChart({
  weeks,
  startBalance,
}: {
  weeks: CashWeek[];
  startBalance: number;
}) {
  let balance = startBalance;
  const points = weeks.map((w) => {
    balance += w.inflow - w.outflow;
    return balance;
  });
  const min = Math.min(...points);
  const max = Math.max(...points);

  const W = 560;
  const H = 200;
  const pl = 44;
  const pb = 22;
  const pt = 14;
  const lo = Math.min(0, min);
  const hi = Math.ceil(max / 100) * 100;
  const bw = (W - pl - 10) / weeks.length;

  const y = (v: number) => H - pb - ((H - pb - pt) * (v - lo)) / (hi - lo);
  const gridValues = [lo, Math.round((lo + hi) / 2 / 100) * 100, hi];
  const lowestWeekIndex = points.indexOf(min);

  return (
    <div>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        width="100%"
        role="img"
        aria-label="Prognos för kassa, 13 veckor"
        className="[&_text]:fill-ink2 [&_text]:font-mono [&_text]:text-[10px]"
      >
        {gridValues.map((v) => (
          <g key={v}>
            <line x1={pl} x2={W - 10} y1={y(v)} y2={y(v)} stroke="var(--line)" />
            <text x={pl - 6} y={y(v) + 3} textAnchor="end">
              {v}
            </text>
          </g>
        ))}
        {points.map((v, i) => {
          const x = pl + i * bw + bw * 0.15;
          const w = bw * 0.7;
          const y0 = y(0);
          const y1 = y(v);
          return (
            <g key={i}>
              <rect
                x={x}
                y={Math.min(y0, y1)}
                width={w}
                height={Math.abs(y1 - y0)}
                fill={v < 150 ? "var(--bad)" : "var(--bar)"}
              />
              <text x={x + w / 2} y={H - 6} textAnchor="middle">
                v{weeks[i]!.week}
              </text>
            </g>
          );
        })}
      </svg>
      <p className="mt-2 text-[0.85rem]">
        Kassan kommer nära <strong>{min} tkr</strong> i vecka {weeks[lowestWeekIndex]!.week}.
      </p>
    </div>
  );
}
